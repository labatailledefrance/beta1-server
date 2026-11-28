//! Terrain d'une tile pour la simulation : grille de passage de 500 × 500
//! cases de 2 m, occupation du sol, part de bâti (règles de P-26).
//!
//! Une tile ne dépend que de la publication : le moment où on la lit ne change
//! rien au résultat. Les modifications du terrain (cratères, murs, B-05)
//! viendront en différences, par-dessus.

use beta1_common::geo::TileId;
use beta1_common::tile::codes::{OCC_SALT, OCC_SEA, OCC_WATER};
use beta1_common::tile::sections::{self, CELLS, GRID};
use beta1_common::tile::{self, SectionId};

/// Un piéton (humain, unité à pied) passe.
pub const P_PIETON: u8 = 1;
/// Un zombie passe (partout sauf dans le bâti : il entre dans l'eau et sur l'estran).
pub const P_ZOMBIE: u8 = 2;
/// Bâti ou obstacle : personne ne passe, la vue est coupée.
pub const P_BATI: u8 = 4;
/// Eau (mer, cours d'eau, plan d'eau) : déplacement ralenti.
pub const P_EAU: u8 = 8;

/// Côté d'une case, en mètres.
pub const CASE: f32 = 2.0;

/// Terrain d'une tile, immuable.
#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    /// Tile.
    pub id: TileId,
    /// La tile est dans les données ; sinon, mer.
    pub dans_les_donnees: bool,
    /// Passage de chaque case (bits `P_*`), indice `j * 500 + i`, j = 0 au sud.
    pub passage: Vec<u8>,
    /// Occupation du sol (`beta1_common::tile::sections::occupation`).
    pub occupation: Vec<u8>,
    /// Part de la tile couverte de bâti, de 0 à 1.
    pub part_bati: f32,
    /// Habitants initiaux.
    pub habitants: u32,
}

/// Indice de la case d'une position locale (m, centrée sur la tile).
pub fn case(local: [f32; 2]) -> Option<usize> {
    let (i, j) = ((local[0] + 500.0) / CASE, (local[1] + 500.0) / CASE);
    let g = GRID as f32;
    ((0.0..g).contains(&i) && (0.0..g).contains(&j)).then(|| j as usize * GRID + i as usize)
}

impl Terrain {
    /// Tile hors des données : de la mer, où seul le zombie passe (jusqu'à l'estran, G-02).
    pub fn mer(id: TileId) -> Terrain {
        Terrain {
            id,
            dans_les_donnees: false,
            passage: vec![P_ZOMBIE | P_EAU; CELLS],
            occupation: vec![OCC_SEA; CELLS],
            part_bati: 0.0,
            habitants: 0,
        }
    }

    /// Décode une tile ; vérifie son empreinte (un disque peut se corrompre).
    pub fn decoder(id: TileId, octets: &[u8]) -> Result<Terrain, String> {
        let t = tile::decode(octets, true).map_err(|_| "tile illisible ou empreinte fausse".to_owned())?;
        if t.header.tile() != Some(id) {
            return Err(format!("en-tête de la tile différent : {:?}", t.header.tile()));
        }
        let section = |s: SectionId| t.section(s).map_err(|_| format!("section {} illisible", s.name()));
        let occupation = match section(SectionId::Occupation)? {
            Some(b) => sections::occupation(&b).map_err(|_| "occupation invalide".to_owned())?.to_vec(),
            None => vec![OCC_SEA; CELLS],
        };
        let franchissable = match section(SectionId::Passable)? {
            Some(b) => Some(sections::passable(&b).map_err(|_| "franchissable invalide".to_owned())?),
            None => None,
        };
        let exclues = match section(SectionId::Zone)? {
            Some(b) => sections::zone(&b).map_err(|_| "zone invalide".to_owned())?.excluded,
            None => None,
        };
        let mut passage = vec![0u8; CELLS];
        for j in 0..GRID {
            for i in 0..GRID {
                let k = j * GRID + i;
                let libre = franchissable.as_ref().is_none_or(|f| f.get(i, j));
                let dans_la_zone = exclues.as_ref().is_none_or(|e| !e.get(i, j));
                let o = occupation[k];
                // Estran (OCC_FORESHORE) : à terre tant que les marées ne sont pas simulées (A-11).
                let mer = o & (OCC_SEA | OCC_SALT) != 0;
                let douce = o & OCC_WATER != 0;
                passage[k] = if !libre {
                    P_BATI
                } else {
                    let eau = if mer || douce { P_EAU } else { 0 };
                    let pieton = if !mer && dans_la_zone { P_PIETON } else { 0 };
                    P_ZOMBIE | eau | pieton
                };
            }
        }
        Ok(Terrain {
            id,
            dans_les_donnees: true,
            passage,
            occupation,
            part_bati: f32::from(t.header.built_share) / 65_535.0,
            habitants: t.header.inhabitants,
        })
    }

    /// Passage de la case (i, j).
    pub fn passage(&self, i: usize, j: usize) -> u8 {
        self.passage[j * GRID + i]
    }

    /// Nombre de cases qui ont tous les bits de `bits`.
    pub fn compter(&self, bits: u8) -> usize {
        self.passage.iter().filter(|p| *p & bits == bits).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases() {
        assert_eq!(case([-500.0, -500.0]), Some(0));
        assert_eq!(case([499.9, -500.0]), Some(499));
        assert_eq!(case([-500.0, -498.0]), Some(500));
        assert_eq!(case([499.9, 499.9]), Some(CELLS - 1));
        assert_eq!(case([500.0, 0.0]), None);
        assert_eq!(case([0.0, -500.1]), None);
    }

    #[test]
    fn mer() {
        let t = Terrain::mer(TileId { ix: 1, iy: 2 });
        assert_eq!(t.compter(P_ZOMBIE | P_EAU), CELLS);
        assert_eq!(t.compter(P_PIETON), 0);
    }
}
