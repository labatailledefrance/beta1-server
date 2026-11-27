//! Réseau : WebSocket sans TLS (l'ingress Traefik porte TLS et les rate limits
//! par adresse IP, H-01), un thread par client.
//!
//! Chaque client a une **send queue bornée** : un client trop lent perd des
//! états au lieu de faire grossir la mémoire du serveur (P-24).

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use bevy::prelude::*;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, accept_with_config};

use beta1_common::protocol::{MAX_CLIENT_MESSAGE, ServerMessage, handshake};

use crate::app::{Phase, Tick};

/// Profondeur de la send queue d'un client, en messages.
pub const QUEUE_DEPTH: usize = 16;

/// État partagé entre la simulation et les threads réseau.
#[derive(Default)]
pub struct Shared {
    tick: AtomicU64,
    clients: Mutex<Vec<Sender<Arc<Vec<u8>>>>>,
}

impl Shared {
    /// Publie le tick courant (lu au handshake).
    pub fn publish_tick(&self, t: u64) {
        self.tick.store(t, Ordering::Relaxed);
    }
    /// Envoie un message à tous les clients ; retire ceux qui sont partis.
    pub fn broadcast(&self, bytes: Vec<u8>) {
        let m = Arc::new(bytes);
        let mut c = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        c.retain(|s| !matches!(s.try_send(m.clone()), Err(TrySendError::Disconnected(_))));
    }
    /// Nombre de clients connectés.
    pub fn client_count(&self) -> usize {
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
    fn subscribe(&self) -> Receiver<Arc<Vec<u8>>> {
        let (s, r) = bounded(QUEUE_DEPTH);
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).push(s);
        r
    }
}

/// Accepte les connexions en tâche de fond.
pub fn listen(listener: TcpListener, shared: Arc<Shared>) {
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let s = shared.clone();
            thread::spawn(move || {
                if let Err(e) = serve(stream, &s) {
                    debug!("client : {e}");
                }
            });
        }
    });
}

fn serve(stream: TcpStream, shared: &Shared) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_nodelay(true)?;
    let config =
        WebSocketConfig::default().max_message_size(Some(MAX_CLIENT_MESSAGE)).max_frame_size(Some(MAX_CLIENT_MESSAGE));
    let mut ws = accept_with_config(stream, Some(config))?;
    let first = match ws.read()? {
        Message::Binary(b) => b,
        _ => return Ok(()),
    };
    let reply = handshake(&first, shared.tick.load(Ordering::Relaxed));
    let accepted = matches!(reply, ServerMessage::Welcome { .. });
    ws.send(Message::Binary(reply.encode().into()))?;
    if !accepted {
        ws.close(None)?;
        return Ok(());
    }
    for m in shared.subscribe() {
        ws.send(Message::Binary(m.as_ref().clone().into()))?;
    }
    Ok(())
}

/// Plugin réseau : en phase Sortie, publie le tick courant et broadcast l'état.
pub struct ReseauPlugin(pub Arc<Shared>);

impl Plugin for ReseauPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SharedRes(self.0.clone())).add_systems(FixedUpdate, broadcast_state.in_set(Phase::Sortie));
    }
}

#[derive(Resource)]
struct SharedRes(Arc<Shared>);

fn broadcast_state(tick: Res<Tick>, shared: Res<SharedRes>) {
    shared.0.publish_tick(tick.0);
    // État à 10 Hz.
    if tick.0.is_multiple_of(2) {
        shared.0.broadcast(ServerMessage::State { tick: tick.0 }.encode());
    }
}
