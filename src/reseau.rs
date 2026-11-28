//! Réseau : WebSocket sans TLS (l'ingress Traefik porte TLS et les rate limits
//! par adresse IP, H-01), un thread par client.
//!
//! Chaque client a une **send queue bornée** : un client trop lent perd des
//! états au lieu de faire grossir la mémoire du serveur (P-24).
//!
//! À 10 Hz, la simulation publie l'état et un **instantané des zombies**
//! (partagé, jamais recopié par client). Le thread de chaque client en tire
//! lui-même ce que sa vue demande : les zombies proches, les plus proches
//! d'abord, dans le budget du protocole. Le coût par client reste hors de la
//! simulation, comme un proxy qui filtre un flux commun pour chaque abonné.

use std::collections::BTreeMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, accept_with_config};

use beta1_common::geo::{Pos, TileId};
use beta1_common::protocol::{BUDGET_ZOMBIES, ClientMessage, MAX_CLIENT_MESSAGE, ServerMessage, ZombieVu, handshake};

use crate::app::{Phase, Tick};
use crate::tiles::TilesActives;
use crate::zombies::Zombies;

/// Profondeur de la send queue d'un client, en messages.
pub const QUEUE_DEPTH: usize = 16;
/// Attente maximale d'un message du client avant de vider sa send queue.
const ATTENTE_LECTURE: Duration = Duration::from_millis(5);

/// Zombies des tiles actives à un tick : (position locale, cap) par tile.
#[derive(Debug, Default)]
pub struct Instantane {
    /// Tick.
    pub tick: u64,
    /// Zombies, par tile.
    pub tiles: BTreeMap<TileId, Vec<([f32; 2], f32)>>,
}

/// Ce que la simulation envoie à chaque client.
#[derive(Clone)]
struct Envoi {
    octets: Arc<Vec<u8>>,
    zombies: Option<Arc<Instantane>>,
}

/// État partagé entre la simulation et les threads réseau.
#[derive(Default)]
pub struct Shared {
    tick: AtomicU64,
    camera: Mutex<Option<Pos>>,
    clients: Mutex<Vec<Sender<Envoi>>>,
}

impl Shared {
    /// État partagé, avec la caméra de départ des clients.
    pub fn new(camera: Pos) -> Shared {
        Shared { camera: Mutex::new(Some(camera)), ..Default::default() }
    }
    /// Publie le tick courant (lu au handshake).
    pub fn publish_tick(&self, t: u64) {
        self.tick.store(t, Ordering::Relaxed);
    }
    /// Envoie un message à tous les clients ; retire ceux qui sont partis.
    pub fn broadcast(&self, bytes: Vec<u8>) {
        self.envoyer(Envoi { octets: Arc::new(bytes), zombies: None });
    }
    /// Envoie l'état et l'instantané des zombies, que chaque client filtre selon sa vue.
    pub fn broadcast_etat(&self, bytes: Vec<u8>, zombies: Arc<Instantane>) {
        self.envoyer(Envoi { octets: Arc::new(bytes), zombies: Some(zombies) });
    }
    fn envoyer(&self, e: Envoi) {
        let mut c = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        c.retain(|s| !matches!(s.try_send(e.clone()), Err(TrySendError::Disconnected(_))));
    }
    /// Nombre de clients connectés.
    pub fn client_count(&self) -> usize {
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
    fn camera(&self) -> Pos {
        self.camera
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or(Pos { tile: TileId { ix: 0, iy: 0 }, local: [0.0; 2] })
    }
    fn subscribe(&self) -> Receiver<Envoi> {
        let (s, r) = bounded(QUEUE_DEPTH);
        self.clients.lock().unwrap_or_else(PoisonError::into_inner).push(s);
        r
    }
}

/// Zombies de l'instantané à moins de `rayon` de `centre`, les plus proches d'abord, dans le budget.
pub fn zombies_vus(inst: &Instantane, centre: Pos, rayon: f32) -> ServerMessage {
    let ancre = centre.tile;
    let r2 = rayon * rayon;
    // Tiles qui peuvent toucher le disque : la vue fait au plus 1 km de rayon.
    let n = (rayon / 1000.0).ceil() as i32 + 1;
    let mut vus: Vec<(f32, u32, ZombieVu)> = Vec::new();
    let mut rang = 0u32;
    for dx in -n..=n {
        for dy in -n..=n {
            let t = TileId { ix: ancre.ix + dx, iy: ancre.iy + dy };
            let Some(zombies) = inst.tiles.get(&t) else { continue };
            for &(local, cap) in zombies {
                let p = Pos { tile: t, local };
                let v = centre.vector_to(p);
                let d2 = v[0] * v[0] + v[1] * v[1];
                if d2 <= r2
                    && let Some(z) = ZombieVu::new(ancre, p, cap)
                {
                    vus.push((d2, rang, z));
                }
                rang += 1;
            }
        }
    }
    let total = vus.len() as u32;
    let plus_proches = |a: &(f32, u32, ZombieVu), b: &(f32, u32, ZombieVu)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1));
    if vus.len() > BUDGET_ZOMBIES {
        vus.select_nth_unstable_by(BUDGET_ZOMBIES, plus_proches);
        vus.truncate(BUDGET_ZOMBIES);
    }
    vus.sort_unstable_by(plus_proches);
    ServerMessage::Zombies { tick: inst.tick, ancre, total, zombies: vus.into_iter().map(|v| v.2).collect() }
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
    let reply = handshake(&first, shared.tick.load(Ordering::Relaxed), shared.camera());
    let accepted = matches!(reply, ServerMessage::Welcome { .. });
    ws.send(Message::Binary(reply.encode().into()))?;
    if !accepted {
        ws.close(None)?;
        return Ok(());
    }
    let file = shared.subscribe();
    ws.get_mut().set_read_timeout(Some(ATTENTE_LECTURE))?;
    let mut vue: Option<(Pos, f32)> = None;
    loop {
        match ws.read() {
            Ok(Message::Binary(b)) => match ClientMessage::decode(&b) {
                Ok(ClientMessage::Vue { centre, rayon }) => vue = Some((centre, rayon)),
                // Hello en double ou octets invalides : on coupe.
                Ok(ClientMessage::Hello { .. }) | Err(_) => {
                    ws.close(None)?;
                    return Ok(());
                }
            },
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
        loop {
            match file.try_recv() {
                Ok(envoi) => {
                    ws.send(Message::Binary(envoi.octets.as_ref().clone().into()))?;
                    if let (Some(inst), Some((centre, rayon))) = (&envoi.zombies, vue) {
                        ws.send(Message::Binary(zombies_vus(inst, centre, rayon).encode().into()))?;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }
}

/// Plugin réseau : en phase Sortie, publie le tick, l'état et l'instantané des zombies.
pub struct ReseauPlugin(pub Arc<Shared>);

impl Plugin for ReseauPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SharedRes(self.0.clone())).add_systems(FixedUpdate, broadcast_state.in_set(Phase::Sortie));
    }
}

#[derive(Resource)]
struct SharedRes(Arc<Shared>);

fn broadcast_state(tick: Res<Tick>, shared: Res<SharedRes>, actives: Res<TilesActives>, zombies: Query<&Zombies>) {
    shared.0.publish_tick(tick.0);
    // État à 10 Hz.
    if !tick.0.is_multiple_of(2) {
        return;
    }
    let mut inst = Instantane { tick: tick.0, tiles: BTreeMap::new() };
    for (t, &e) in &actives.0 {
        if let Ok(z) = zombies.get(e) {
            inst.tiles.insert(*t, z.liste.iter().map(|z| (z.local, z.cap)).collect());
        }
    }
    shared.0.broadcast_etat(ServerMessage::State { tick: tick.0 }.encode(), Arc::new(inst));
}
