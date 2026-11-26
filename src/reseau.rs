//! Réseau : WebSocket sans TLS (l'ingress Traefik porte TLS et les limites par
//! adresse IP, H-01), un thread par client.
//!
//! Chaque client a une **file d'envoi bornée** : un client trop lent perd des
//! états au lieu de faire grossir la mémoire du serveur (P-24).

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, accept_with_config};

use beta1_common::protocole::{DuServeur, TAILLE_MAX_DU_CLIENT, poignee_de_main};

/// Profondeur de la file d'envoi d'un client, en messages.
pub const FILE: usize = 16;

/// État partagé entre la simulation et les threads réseau.
#[derive(Default)]
pub struct Partage {
    tick: AtomicU64,
    clients: Mutex<Vec<Sender<Arc<Vec<u8>>>>>,
}

impl Partage {
    /// Publie le tick courant (lu à la poignée de main).
    pub fn publier_tick(&self, t: u64) {
        self.tick.store(t, Ordering::Relaxed);
    }
    /// Envoie un message à tous les clients ; retire ceux qui sont partis.
    pub fn diffuser(&self, octets: Vec<u8>) {
        let m = Arc::new(octets);
        let mut c = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        c.retain(|s| !matches!(s.try_send(m.clone()), Err(TrySendError::Disconnected(_))));
    }
    /// Nombre de clients connectés.
    pub fn nb_clients(&self) -> usize {
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
    fn inscrire(&self) -> Receiver<Arc<Vec<u8>>> {
        let (s, r) = bounded(FILE);
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).push(s);
        r
    }
}

/// Accepte les connexions en tâche de fond.
pub fn demarrer(ecoute: TcpListener, partage: Arc<Partage>) {
    thread::spawn(move || {
        for flux in ecoute.incoming().flatten() {
            let p = partage.clone();
            thread::spawn(move || {
                if let Err(e) = servir(flux, &p) {
                    bevy::log::debug!("client : {e}");
                }
            });
        }
    });
}

fn servir(flux: TcpStream, p: &Partage) -> Result<(), Box<dyn std::error::Error>> {
    flux.set_nodelay(true)?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(TAILLE_MAX_DU_CLIENT))
        .max_frame_size(Some(TAILLE_MAX_DU_CLIENT));
    let mut ws = accept_with_config(flux, Some(config))?;
    let premier = match ws.read()? {
        Message::Binary(b) => b,
        _ => return Ok(()),
    };
    let reponse = poignee_de_main(&premier, p.tick.load(Ordering::Relaxed));
    let accepte = matches!(reponse, DuServeur::Bienvenue { .. });
    ws.send(Message::Binary(reponse.encoder().into()))?;
    if !accepte {
        ws.close(None)?;
        return Ok(());
    }
    let file = p.inscrire();
    for m in file {
        ws.send(Message::Binary(m.as_ref().clone().into()))?;
    }
    Ok(())
}
