//! Organisation du serveur : phases, détection d'ambiguïté, déterminisme.

use std::path::Path;

use bevy::prelude::*;

use beta1_server::app::{self, Phase, Tick};
use beta1_server::catalogue::{self, Catalogue};
use beta1_server::determinisme::StateHash;

fn base() -> Catalogue {
    catalogue::charger(&Path::new(env!("CARGO_MANIFEST_DIR")).join("mods"), &["base".to_owned()]).unwrap()
}

#[derive(Resource, Default)]
struct Compteur(u64);

fn ecrit_a(mut c: ResMut<Compteur>) {
    c.0 += 1;
}

fn ecrit_b(mut c: ResMut<Compteur>) {
    c.0 *= 2;
}

#[test]
fn serveur_sans_ambiguite() {
    let mut a = app::app(base(), 2, false);
    app::verifier(&mut a).unwrap();
}

#[test]
fn ambiguite_refusee() {
    let mut a = app::app(base(), 2, false);
    a.init_resource::<Compteur>().add_systems(FixedUpdate, (ecrit_a, ecrit_b).in_set(Phase::Combat));
    let e = app::verifier(&mut a).unwrap_err();
    assert!(e.contains("ecrit_a") && e.contains("ecrit_b"), "{e}");
}

#[test]
fn ordre_declare_accepte() {
    let mut a = app::app(base(), 2, false);
    a.init_resource::<Compteur>().add_systems(FixedUpdate, (ecrit_a, ecrit_b).chain().in_set(Phase::Combat));
    app::verifier(&mut a).unwrap();
}

#[test]
fn phases_dans_l_ordre() {
    #[derive(Resource, Default)]
    struct Trace(Vec<&'static str>);
    let mut a = app::app(base(), 2, false);
    a.init_resource::<Trace>().add_systems(
        FixedUpdate,
        (
            (|mut t: ResMut<Trace>| t.0.push("sortie")).in_set(Phase::Sortie),
            (|mut t: ResMut<Trace>| t.0.push("entrees")).in_set(Phase::Entrees),
            (|mut t: ResMut<Trace>| t.0.push("combat")).in_set(Phase::Combat),
        ),
    );
    app::avancer_de(&mut a, 1);
    assert_eq!(a.world().resource::<Trace>().0, ["entrees", "combat", "sortie"]);
}

#[test]
fn hash_d_etat_independant_des_threads() {
    let hash = |threads| {
        let mut a = app::app(base(), threads, false);
        app::avancer_de(&mut a, 200);
        assert_eq!(a.world().resource::<Tick>().0, 200);
        a.world().resource::<StateHash>().0
    };
    assert_eq!(hash(1), hash(8));
}

#[test]
fn hash_d_etat_part_du_catalogue() {
    let c = base();
    let mut a = app::app(c.clone(), 1, false);
    app::avancer_de(&mut a, 0);
    // Sans publication de tiles : graine = hash3(catalogue, 0, 0).
    assert_eq!(a.world().resource::<StateHash>().0, beta1_common::rng::hash3(c.hash_court(), 0, 0));
}
