//! Handshake, état et zombies vus, sur un vrai socket ; sélection des zombies.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::Arc;

use tungstenite::{Message, connect};

use beta1_common::geo::{Pos, TileId};
use beta1_common::protocol::{BUDGET_ZOMBIES, ClientMessage, FINGERPRINT, RejectReason, ServerMessage};
use beta1_server::reseau::{Instantane, Shared, listen, zombies_vus};

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

const BREST: TileId = TileId { ix: 146, iy: 6839 };
const CAMERA: Pos = Pos { tile: BREST, local: [0.0, 0.0] };

fn server() -> (String, Arc<Shared>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", l.local_addr().unwrap());
    let s = Arc::new(Shared::new(CAMERA));
    s.publish_tick(5);
    listen(l, s.clone());
    (url, s)
}

fn read(ws: &mut Ws) -> ServerMessage {
    loop {
        if let Message::Binary(b) = ws.read().unwrap() {
            return ServerMessage::decode(&b).unwrap();
        }
    }
}

fn bonjour(url: &str, s: &Shared) -> Ws {
    let (mut ws, _) = connect(url).unwrap();
    ws.send(Message::Binary(ClientMessage::Hello { fingerprint: FINGERPRINT }.encode().into())).unwrap();
    assert_eq!(read(&mut ws), ServerMessage::Welcome { tick: 5, camera: CAMERA });
    while s.client_count() == 0 {
        std::thread::yield_now();
    }
    ws
}

/// Instantané : une ligne de zombies tous les 10 m vers l'est, depuis le centre de Brest.
fn ligne(n: usize) -> Instantane {
    let mut tiles = BTreeMap::new();
    for k in 0..n {
        let p = CAMERA.offset([k as f32 * 10.0, 0.0]);
        tiles.entry(p.tile).or_insert_with(Vec::new).push((p.local, 0.0));
    }
    Instantane { tick: 8, tiles }
}

#[test]
fn bon_fingerprint_puis_etats() {
    let (url, s) = server();
    let mut ws = bonjour(&url, &s);
    s.broadcast(ServerMessage::State { tick: 6 }.encode());
    assert_eq!(read(&mut ws), ServerMessage::State { tick: 6 });
}

#[test]
fn vue_puis_zombies() {
    let (url, s) = server();
    let mut ws = bonjour(&url, &s);
    ws.send(Message::Binary(ClientMessage::Vue { centre: CAMERA, rayon: 95.0 }.encode().into())).unwrap();
    // La vue arrive avant l'état suivant : on réessaie jusqu'à recevoir des zombies.
    let inst = Arc::new(ligne(50));
    loop {
        s.broadcast_etat(ServerMessage::State { tick: 8 }.encode(), inst.clone());
        assert_eq!(read(&mut ws), ServerMessage::State { tick: 8 });
        // Tant que le serveur n'a pas lu la vue, il n'envoie pas de zombies : on renvoie un état.
        let ServerMessage::Zombies { total, zombies, .. } = read_ou_rien(&mut ws) else { continue };
        // Zombies à 0, 10, … 90 m : 10 dans le rayon de 95 m.
        assert_eq!((total, zombies.len()), (10, 10));
        break;
    }
}

/// Le message suivant s'il arrive vite, sinon un état factice (pour réessayer).
fn read_ou_rien(ws: &mut Ws) -> ServerMessage {
    if let tungstenite::stream::MaybeTlsStream::Plain(t) = ws.get_mut() {
        t.set_read_timeout(Some(std::time::Duration::from_millis(200))).unwrap();
    }
    match ws.read() {
        Ok(Message::Binary(b)) => ServerMessage::decode(&b).unwrap(),
        _ => ServerMessage::State { tick: 0 },
    }
}

#[test]
fn selection_par_distance_et_budget() {
    // 3 000 zombies sur 30 km, dans le rayon de 1 km : 101 (0 à 1 000 m).
    let inst = ligne(3000);
    let ServerMessage::Zombies { tick, ancre, total, zombies } = zombies_vus(&inst, CAMERA, 1000.0) else { panic!() };
    assert_eq!((tick, ancre, total, zombies.len()), (8, BREST, 101, 101));
    // Les plus proches d'abord.
    let d: Vec<f32> = zombies.iter().map(|z| CAMERA.distance(z.pos(ancre))).collect();
    assert!(d.windows(2).all(|w| w[0] <= w[1]), "{d:?}");
    assert!(d[100] > 999.0 && d[100] < 1000.1);
    // Au-delà du budget : les plus proches seulement, le total compte tout.
    let mut dense = Instantane { tick: 1, tiles: BTreeMap::new() };
    let pts: Vec<([f32; 2], f32)> =
        (0..5000).map(|k| ([(k % 100) as f32 * 2.0 - 100.0, (k / 100) as f32 * 2.0 - 50.0], 0.0)).collect();
    dense.tiles.insert(BREST, pts);
    let ServerMessage::Zombies { total, zombies, .. } = zombies_vus(&dense, CAMERA, 1000.0) else { panic!() };
    assert_eq!((total, zombies.len()), (5000, BUDGET_ZOMBIES));
    let loin = zombies.iter().map(|z| CAMERA.distance(z.pos(BREST))).fold(0.0f32, f32::max);
    // 1 200 points d'une grille de 2 m tiennent dans un disque de √(1200 × 4 / π) ≈ 39 m.
    assert!(loin < 40.0, "le plus loin des retenus est à {loin} m");
}

#[test]
fn mauvais_fingerprint_rejete() {
    let (url, s) = server();
    let (mut ws, _) = connect(&url).unwrap();
    ws.send(Message::Binary(ClientMessage::Hello { fingerprint: [0; 32] }.encode().into())).unwrap();
    assert_eq!(read(&mut ws), ServerMessage::Rejected { reason: RejectReason::Version });
    assert_eq!(s.client_count(), 0);
}

#[test]
fn message_trop_long_rejete_sans_abonnement() {
    let (url, s) = server();
    let (mut ws, _) = connect(&url).unwrap();
    let _ = ws.send(Message::Binary(vec![1; 100_000].into()));
    let _ = ws.read();
    assert_eq!(s.client_count(), 0);
}

#[test]
fn message_invalide_apres_handshake_coupe() {
    let (url, s) = server();
    let mut ws = bonjour(&url, &s);
    ws.send(Message::Binary(vec![0xEE, 1, 2].into())).unwrap();
    // Le serveur ferme : la lecture finit par échouer ou rendre Close.
    loop {
        match ws.read() {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
}
