//! Zombies : peuplement à l'activation, errance, passages de tile, déterminisme.

mod commun;

use std::path::Path;
use std::sync::Arc;

use bevy::prelude::*;

use beta1_common::geo::TileId;
use beta1_server::app::{self, Monde};
use beta1_server::determinisme::StateHash;
use beta1_server::tiles::terrain::{self, P_PIETON, P_ZOMBIE};
use beta1_server::tiles::{DELAI_ACTIVATION, Publication, TileActive, TilesActives};
use beta1_server::zombies::{PAR_CASE, Zombies};
use commun::{dossier, monde, ouvrir};

const A: TileId = TileId { ix: 145, iy: 6838 };
/// Au nord de A : sa lisière sud est de la mer (tile d'essai), où passent les zombies.
const NORD: TileId = TileId { ix: 145, iy: 6839 };

/// Monde de deux tiles d'essai, avec `densite` zombies par habitant (42 habitants par tile).
fn monde_dense(tiles: Vec<TileId>, densite: f32) -> Monde {
    let mut m = monde(Some(ouvrir(&[&[A, NORD]])), tiles);
    if let Some(s) = &mut m.scenario {
        s.densite_zombies = densite;
    }
    m
}

/// (tile, zombies) de chaque tile active, dans l'ordre.
fn etat(a: &mut App) -> Vec<(TileId, Zombies)> {
    let actives: Vec<(TileId, Entity)> = a.world().resource::<TilesActives>().0.iter().map(|(t, e)| (*t, *e)).collect();
    actives.into_iter().filter_map(|(t, e)| a.world().get::<Zombies>(e).map(|z| (t, z.clone()))).collect()
}

/// Invariants : chaque zombie sur une case où il passe, occupation cohérente et bornée.
fn verifier_invariants(a: &mut App) {
    let actives: Vec<Entity> = a.world().resource::<TilesActives>().0.values().copied().collect();
    for e in actives {
        let tile = a.world().get::<TileActive>(e).unwrap().clone();
        let Some(z) = a.world().get::<Zombies>(e) else { continue };
        let mut occ = vec![0u8; z.occupation.len()];
        for zombie in &z.liste {
            let k = terrain::case(zombie.local).expect("position locale dans la tile");
            assert!(tile.terrain.passage[k] & P_ZOMBIE != 0, "zombie {} sur une case interdite", zombie.id);
            occ[k] += 1;
        }
        assert_eq!(occ, z.occupation, "occupation incohérente sur {:?}", tile.id);
        assert!(z.occupation.iter().all(|o| *o <= PAR_CASE));
    }
}

fn total(a: &mut App) -> usize {
    etat(a).iter().map(|(_, z)| z.liste.len()).sum()
}

#[test]
fn peuplement_a_la_mise_en_service() {
    let mut a = app::app(monde_dense(vec![A], 10.0), 2, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    let e = etat(&mut a);
    assert_eq!(e.len(), 1);
    let (_, z) = &e[0];
    // 42 habitants × 10.
    assert_eq!(z.liste.len() as u32 + z.sans_place, 420);
    assert_eq!(z.sans_place, 0);
    let tile = a.world().resource::<TilesActives>().0[&A];
    let terrain = a.world().get::<TileActive>(tile).unwrap().terrain.clone();
    for zombie in &z.liste {
        let k = terrain::case(zombie.local).unwrap();
        assert!(terrain.passage[k] & P_PIETON != 0, "placé hors de la terre ouverte");
    }
    // Identifiants uniques, attribués dans l'ordre.
    let ids: Vec<u32> = z.liste.iter().map(|z| z.id).collect();
    assert_eq!(ids, (0..420).collect::<Vec<_>>());
    verifier_invariants(&mut a);
}

#[test]
fn outre_mer_sans_zombies() {
    // Même tile, mais en zone UTM 40S (La Réunion) : hors des zones peuplées.
    let reunion = TileId::from_zone(3, 300, 7650).unwrap();
    let d = dossier();
    commun::publication(&d, &[&[reunion]]);
    let p = Arc::new(Publication::ouvrir(&d, 1).unwrap());
    let mut a = app::app(monde(Some(p), vec![reunion]), 1, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    let e = etat(&mut a);
    assert_eq!(e.len(), 1);
    assert!(e[0].1.liste.is_empty());
}

#[test]
fn errance_et_passages_de_tile() {
    // Densité forte : certains zombies, placés loin du bâti, errent jusqu'à la lisière nord de A.
    let mut a = app::app(monde_dense(vec![A, NORD], 60.0), 4, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    let depart: Vec<(TileId, Vec<[f32; 2]>)> =
        etat(&mut a).into_iter().map(|(t, z)| (t, z.liste.iter().map(|z| z.local).collect())).collect();
    let n = total(&mut a);
    assert_eq!(n, 2 * 42 * 60);
    // 3 000 ticks : 2 min 30 s de simulation.
    for _ in 0..30 {
        app::avancer_de(&mut a, 100);
        verifier_invariants(&mut a);
        assert_eq!(total(&mut a), n, "un zombie s'est perdu ou dédoublé");
    }
    let fin = etat(&mut a);
    // Ils ont bougé.
    let bouges = fin[0].1.liste.iter().zip(&depart[0].1).filter(|(z, p)| z.local != **p).count();
    assert!(bouges > n / 4, "{bouges} zombies ont bougé");
    // Des zombies de A sont passés dans la tile du nord (identifiants < 2 520).
    let venus_de_a = fin.iter().find(|(t, _)| *t == NORD).unwrap().1.liste.iter().filter(|z| z.id < 2520).count();
    assert!(venus_de_a > 0, "aucun passage de A vers le nord");
}

#[test]
fn bord_d_une_tile_inactive() {
    // A seule : personne ne sort, la tile du nord est inactive.
    let mut a = app::app(monde_dense(vec![A], 60.0), 2, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    let n = total(&mut a);
    app::avancer_de(&mut a, 3000);
    assert_eq!(total(&mut a), n);
    verifier_invariants(&mut a);
}

#[test]
fn hash_avec_zombies_independant_des_threads() {
    let hash = |threads| {
        let mut a = app::app(monde_dense(vec![A, NORD], 20.0), threads, false);
        app::avancer_de(&mut a, 600);
        a.world().resource::<StateHash>().0
    };
    assert_eq!(hash(1), hash(8));
}

/// Vraies tiles de Brest, si elles sont là : peuplement et coût d'un tick.
#[test]
fn zombies_de_brest() {
    let sortie = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/generated/essai-ci/sortie");
    if !sortie.join("paquets").is_dir() {
        eprintln!("pas de tiles de Brest : essai sauté");
        return;
    }
    let d = dossier();
    std::fs::create_dir_all(d.join("departements")).unwrap();
    std::os::unix::fs::symlink(sortie.canonicalize().unwrap(), d.join("departements/D029")).unwrap();
    std::fs::write(d.join("MANIFESTE"), "publication b0e57\nversion 0.1.0\nD029 00\n").unwrap();
    std::fs::write(d.join("COMPLET"), "").unwrap();
    let p = Arc::new(Publication::ouvrir(&d, 4).unwrap());
    let c = commun::base();
    let m = Monde::new(c, Some(p), "officiel").unwrap();
    let mut a = app::app(m, 8, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    let n = total(&mut a);
    let sans: u32 = etat(&mut a).iter().map(|(_, z)| z.sans_place).sum();
    let debut = std::time::Instant::now();
    app::avancer_de(&mut a, 200);
    let ms = debut.elapsed().as_secs_f64() * 1000.0 / 200.0;
    eprintln!("Brest, 16 tiles : {n} zombies placés, {sans} sans place, {ms:.2} ms par tick (profil de test)");
    assert!(n > 10_000);
    verifier_invariants(&mut a);
}
