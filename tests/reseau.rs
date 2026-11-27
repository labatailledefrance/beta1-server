//! Handshake et broadcast, sur un vrai socket.

use std::net::TcpListener;
use std::sync::Arc;

use tungstenite::{Message, connect};

use beta1_common::protocol::{ClientMessage, FINGERPRINT, RejectReason, ServerMessage};
use beta1_server::reseau::{Shared, listen};

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn server() -> (String, Arc<Shared>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", l.local_addr().unwrap());
    let s = Arc::new(Shared::default());
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

#[test]
fn bon_fingerprint_puis_etats() {
    let (url, s) = server();
    let (mut ws, _) = connect(&url).unwrap();
    ws.send(Message::Binary(ClientMessage::Hello { fingerprint: FINGERPRINT }.encode().into())).unwrap();
    assert_eq!(read(&mut ws), ServerMessage::Welcome { tick: 5 });
    while s.client_count() == 0 {
        std::thread::yield_now();
    }
    s.broadcast(ServerMessage::State { tick: 6 }.encode());
    assert_eq!(read(&mut ws), ServerMessage::State { tick: 6 });
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
