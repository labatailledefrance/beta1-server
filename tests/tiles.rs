//! Tiles : publication, terrain, activation à tick fixé. Tiles fabriquées avec
//! les encodeurs de beta1-common ; plus un essai sur les vraies tiles de Brest,
//! quand elles sont là (`data/generated/essai-ci/sortie`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

use beta1_common::geo::TileId;
use beta1_common::tile::codes::{OCC_SEA, OCC_WATER};
use beta1_common::tile::sections::{BITMAP_LEN, CELLS, GRID};
use beta1_common::tile::{self, Header, Section, SectionId};
use beta1_server::app::{self, Monde};
use beta1_server::catalogue;
use beta1_server::determinisme::StateHash;
use beta1_server::tiles::terrain::{P_BATI, P_EAU, P_PIETON, P_ZOMBIE};
use beta1_server::tiles::{DELAI_ACTIVATION, Publication, Terrain, TileActive, TilesActives};

fn dossier() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "tiles-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn section(id: SectionId, brut: &[u8]) -> Section {
    Section { id, raw_len: brut.len(), data: compress_to_vec(brut, CompressionLevel::Fastest) }
}

/// Tile d'essai : bâti à l'ouest (i < 100), rivière sur la ligne j = 250, mer au sud (j = 0).
fn tile_essai(t: TileId) -> Vec<u8> {
    let mut franchissable = vec![0u64; BITMAP_LEN / 8];
    let mut occupation = vec![0u8; CELLS];
    for j in 0..GRID {
        for i in 0..GRID {
            let k = j * GRID + i;
            if i >= 100 {
                franchissable[k >> 6] |= 1 << (k & 63);
            }
            if j == 250 {
                occupation[k] = OCC_WATER;
            }
            if j == 0 {
                occupation[k] = OCC_SEA;
            }
        }
    }
    let bits: Vec<u8> = franchissable.iter().flat_map(|w| w.to_le_bytes()).collect();
    let h = Header {
        zone: t.zone(),
        ix: t.zone_ix(),
        iy: t.iy,
        flags: 3,
        built_share: 13_107,
        inhabitants: 42,
        buildings: 1,
    };
    tile::encode(&h, &[section(SectionId::Passable, &bits), section(SectionId::Occupation, &occupation)])
}

/// Publication d'un département `D029`, un paquet par groupe de tiles.
fn publication(racine: &Path, paquets: &[&[TileId]]) {
    std::fs::write(racine.join("MANIFESTE"), "publication abcd\nversion 0.1.0\nD029 00\ngraphe 00\n").unwrap();
    let d = racine.join("departements/D029/paquets");
    std::fs::create_dir_all(&d).unwrap();
    for (k, ts) in paquets.iter().enumerate() {
        let tiles: Vec<(TileId, Vec<u8>)> = ts.iter().map(|t| (*t, tile_essai(*t))).collect();
        let v: Vec<(i32, i32, &[u8])> = tiles.iter().map(|(t, b)| (t.ix, t.iy, b.as_slice())).collect();
        std::fs::write(d.join(format!("bloc_{k}.p07")), tile::encode_pack(&v)).unwrap();
    }
    std::fs::write(racine.join("COMPLET"), "").unwrap();
}

const A: TileId = TileId { ix: 145, iy: 6838 };
const B: TileId = TileId { ix: 146, iy: 6838 };
const MER: TileId = TileId { ix: 10, iy: 6000 };

fn ouvrir(paquets: &[&[TileId]]) -> Arc<Publication> {
    let d = dossier();
    publication(&d, paquets);
    Arc::new(Publication::ouvrir(&d, 4).unwrap())
}

#[test]
fn index_de_la_publication() {
    let p = ouvrir(&[&[A], &[B]]);
    assert_eq!(p.id, "abcd");
    assert_eq!(p.departements, ["D029"]);
    assert_eq!((p.len(), p.nb_paquets()), (2, 2));
    assert!(p.contient(A) && p.contient(B) && !p.contient(MER));
    assert_eq!(p.lire(A).unwrap().unwrap(), tile_essai(A));
    assert!(p.lire(MER).unwrap().is_none());
}

#[test]
fn publication_incomplete_refusee() {
    let d = dossier();
    publication(&d, &[&[A]]);
    std::fs::remove_file(d.join("COMPLET")).unwrap();
    let e = Publication::ouvrir(&d, 1).unwrap_err();
    assert!(e.to_string().contains("COMPLET"), "{e}");
}

#[test]
fn tile_en_double_refusee() {
    let d = dossier();
    publication(&d, &[&[A], &[A, B]]);
    let e = Publication::ouvrir(&d, 2).unwrap_err();
    assert!(e.to_string().contains("déjà dans un autre paquet"), "{e}");
}

#[test]
fn terrain_decode() {
    let t = Terrain::decoder(A, &tile_essai(A)).unwrap();
    assert!(t.dans_les_donnees);
    assert_eq!(t.habitants, 42);
    assert!((t.part_bati - 0.2).abs() < 1e-4);
    // Bâti à l'ouest : personne ne passe.
    assert_eq!(t.passage(50, 100), P_BATI);
    // Terre libre : tout le monde.
    assert_eq!(t.passage(300, 100), P_ZOMBIE | P_PIETON);
    // Rivière : eau, piétons et zombies passent (ralentis).
    assert_eq!(t.passage(300, 250), P_ZOMBIE | P_PIETON | P_EAU);
    // Mer : seul le zombie.
    assert_eq!(t.passage(300, 0), P_ZOMBIE | P_EAU);
    assert_eq!(t.compter(P_BATI), 100 * GRID);
}

#[test]
fn terrain_refuse_une_tile_corrompue_ou_deplacee() {
    let mut b = tile_essai(A);
    let n = b.len();
    b[n / 2] ^= 1;
    assert!(Terrain::decoder(A, &b).unwrap_err().contains("empreinte"));
    assert!(Terrain::decoder(B, &tile_essai(A)).unwrap_err().contains("en-tête"));
}

fn monde(publication: Option<Arc<Publication>>, tiles: Vec<TileId>) -> Monde {
    let c = catalogue::charger(&Path::new(env!("CARGO_MANIFEST_DIR")).join("mods"), &["base".to_owned()]).unwrap();
    Monde { catalogue: c, publication, tiles_initiales: tiles }
}

fn actives(a: &App) -> Vec<TileId> {
    a.world().resource::<TilesActives>().0.keys().copied().collect()
}

#[test]
fn activation_a_tick_fixe() {
    let p = ouvrir(&[&[A, B]]);
    let mut a = app::app(monde(Some(p), vec![B, MER, A]), 2, false);
    // Demandées au tick 1, en service au tick 1 + DELAI.
    app::avancer_de(&mut a, DELAI_ACTIVATION);
    assert!(actives(&a).is_empty());
    app::avancer_de(&mut a, 1);
    // Triées par TileId (ix, puis iy).
    assert_eq!(actives(&a), [MER, A, B]);
    let mut q = a.world_mut().query::<&TileActive>();
    let mut vues: Vec<(TileId, bool, u64)> =
        q.iter(a.world()).map(|t| (t.id, t.terrain.dans_les_donnees, t.depuis)).collect();
    vues.sort_by_key(|v| v.0);
    let tick = DELAI_ACTIVATION + 1;
    assert_eq!(vues, [(MER, false, tick), (A, true, tick), (B, true, tick)]);
}

#[test]
fn hash_avec_tiles_independant_des_threads() {
    let p = ouvrir(&[&[A, B]]);
    let hash = |threads| {
        let mut a = app::app(monde(Some(p.clone()), vec![A, B]), threads, false);
        app::avancer_de(&mut a, 40);
        a.world().resource::<StateHash>().0
    };
    let sans = {
        let mut a = app::app(monde(None, vec![A, B]), 1, false);
        app::avancer_de(&mut a, 40);
        a.world().resource::<StateHash>().0
    };
    assert_eq!(hash(1), hash(8));
    assert_ne!(hash(1), sans, "la publication entre dans le hash");
}

#[test]
fn tile_corrompue_arrete_le_serveur() {
    let d = dossier();
    publication(&d, &[&[A]]);
    let paquet = d.join("departements/D029/paquets/bloc_0.p07");
    let mut b = std::fs::read(&paquet).unwrap();
    let n = b.len();
    b[n - 40] ^= 0xFF;
    std::fs::write(&paquet, b).unwrap();
    let p = Arc::new(Publication::ouvrir(&d, 1).unwrap());
    let mut a = app::app(monde(Some(p), vec![A]), 1, false);
    app::avancer_de(&mut a, DELAI_ACTIVATION + 1);
    assert!(actives(&a).is_empty());
    assert!(matches!(a.should_exit(), Some(AppExit::Error(_))));
}

/// Vraies tiles de Brest (sortie de l'essai de la chaîne), si elles sont là.
#[test]
fn tiles_de_brest() {
    let sortie = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/generated/essai-ci/sortie");
    if !sortie.join("paquets").is_dir() {
        eprintln!("pas de tiles de Brest dans {} : essai sauté", sortie.display());
        return;
    }
    let d = dossier();
    std::fs::create_dir_all(d.join("departements")).unwrap();
    std::os::unix::fs::symlink(sortie.canonicalize().unwrap(), d.join("departements/D029")).unwrap();
    std::fs::write(d.join("MANIFESTE"), "publication b0e57\nversion 0.1.0\nD029 00\n").unwrap();
    std::fs::write(d.join("COMPLET"), "").unwrap();
    let p = Publication::ouvrir(&d, 4).unwrap();
    assert_eq!(p.len(), 25);
    let centre = TileId { ix: 146, iy: 6839 };
    let t = beta1_server::tiles::charger(Some(&p), centre).unwrap();
    let (bati, pieton, eau) = (t.compter(P_BATI), t.compter(P_PIETON), t.compter(P_EAU));
    eprintln!(
        "Brest ({}, {}) : bâti {bati}, piéton {pieton}, eau {eau}, part de bâti {:.2}",
        centre.ix, centre.iy, t.part_bati
    );
    assert!(bati > 1000 && pieton > 10_000, "bâti {bati}, piéton {pieton}");
}
