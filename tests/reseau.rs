//! Poignée de main et diffusion, sur un vrai socket.

use std::net::TcpListener;
use std::sync::Arc;

use tungstenite::{Message, connect};

use beta1_common::protocole::{DuClient, DuServeur, EMPREINTE, Raison};
use beta1_server::reseau::{Partage, demarrer};

fn serveur() -> (String, Arc<Partage>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let adresse = format!("ws://{}", l.local_addr().unwrap());
    let p = Arc::new(Partage::default());
    p.publier_tick(5);
    demarrer(l, p.clone());
    (adresse, p)
}

fn lire(ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>) -> DuServeur {
    loop {
        if let Message::Binary(b) = ws.read().unwrap() {
            return DuServeur::decoder(&b).unwrap();
        }
    }
}

#[test]
fn bonne_empreinte_puis_etats() {
    let (a, p) = serveur();
    let (mut ws, _) = connect(&a).unwrap();
    ws.send(Message::Binary(DuClient::Bonjour { empreinte: EMPREINTE }.encoder().into())).unwrap();
    assert_eq!(lire(&mut ws), DuServeur::Bienvenue { tick: 5 });
    while p.nb_clients() == 0 {
        std::thread::yield_now();
    }
    p.diffuser(DuServeur::Etat { tick: 6 }.encoder());
    assert_eq!(lire(&mut ws), DuServeur::Etat { tick: 6 });
}

#[test]
fn mauvaise_empreinte_refusee() {
    let (a, p) = serveur();
    let (mut ws, _) = connect(&a).unwrap();
    ws.send(Message::Binary(DuClient::Bonjour { empreinte: [0; 32] }.encoder().into())).unwrap();
    assert_eq!(lire(&mut ws), DuServeur::Refus { raison: Raison::Version });
    assert_eq!(p.nb_clients(), 0);
}

#[test]
fn message_trop_long_refuse_sans_inscription() {
    let (a, p) = serveur();
    let (mut ws, _) = connect(&a).unwrap();
    let _ = ws.send(Message::Binary(vec![1; 100_000].into()));
    let _ = ws.read();
    assert_eq!(p.nb_clients(), 0);
}
