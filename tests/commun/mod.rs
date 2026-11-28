//! Outils des tests : tiles fabriquées avec les encodeurs de beta1-common.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ruzstd::encoding::{CompressionLevel, compress_to_vec};

use beta1_common::geo::TileId;
use beta1_common::tile::codes::{OCC_SEA, OCC_WATER};
use beta1_common::tile::sections::{BITMAP_LEN, CELLS, GRID};
use beta1_common::tile::{self, Header, Section, SectionId};
use beta1_server::app::Monde;
use beta1_server::catalogue::{self, Catalogue};
use beta1_server::tiles::Publication;

/// Catalogue du mod `base` du dépôt.
pub fn base() -> Catalogue {
    catalogue::charger(&Path::new(env!("CARGO_MANIFEST_DIR")).join("mods"), &["base".to_owned()]).unwrap()
}

pub fn dossier() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "essai-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn section(id: SectionId, brut: &[u8]) -> Section {
    Section { id, raw_len: brut.len(), data: compress_to_vec(brut, CompressionLevel::Fastest) }
}

/// Tile d'essai : bâti à l'ouest (i < 100), rivière sur la ligne j = 250, mer au sud (j = 0).
pub fn tile_essai(t: TileId) -> Vec<u8> {
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
pub fn publication(racine: &Path, paquets: &[&[TileId]]) {
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

/// Ouvre une publication fabriquée.
pub fn ouvrir(paquets: &[&[TileId]]) -> Arc<Publication> {
    let d = dossier();
    publication(&d, paquets);
    Arc::new(Publication::ouvrir(&d, 4).unwrap())
}

/// Monde du scénario officiel, avec ces tiles actives au départ.
pub fn monde(publication: Option<Arc<Publication>>, tiles: Vec<TileId>) -> Monde {
    let c = base();
    let mut s = c.scenario("officiel").unwrap().clone();
    s.tiles_actives = tiles;
    Monde { catalogue: c, publication, scenario: Some(s) }
}
