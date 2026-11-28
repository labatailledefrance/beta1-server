//! Placement des zombies d'une tile à sa première activation (P-04, P-26).
//!
//! Chaque essai est un hachage de (graine de la tile, rang du zombie, essai) :
//! le résultat ne dépend que de la tile, jamais de l'ordre ni du moment de
//! l'activation. Les zombies se placent de préférence près du bâti (12 m),
//! là où vivaient les habitants, et toujours sur une case de terre ouverte
//! aux piétons : jamais en mer ni dans une cour fermée (P-26 l'acceptait dans
//! ses derniers essais).

use beta1_common::rng::{hash3, uniform};
use beta1_common::tile::sections::GRID;

use super::PAR_CASE;
use crate::tiles::terrain::{self, P_BATI, P_PIETON, P_ZOMBIE, Terrain};

/// Essais par zombie ; les premiers exigent la proximité du bâti.
const ESSAIS: u64 = 300;
const ESSAIS_PRES_DU_BATI: u64 = 200;
/// Proximité du bâti, en cases (6 cases = 12 m).
const PRES: i32 = 6;

/// Zombies placés : (position locale, cap), occupation des cases, zombies sans place.
pub struct Places {
    /// Position locale (m) et cap (radians) de chaque zombie placé, dans l'ordre des rangs.
    pub zombies: Vec<([f32; 2], f32)>,
    /// Zombies par case.
    pub occupation: Vec<u8>,
    /// Zombies sans place (comptés, jamais perdus).
    pub sans_place: u32,
}

/// Place `n` zombies sur `terrain`.
pub fn poser(terrain: &Terrain, n: u32, graine_tile: u64) -> Places {
    let mut p = Places { zombies: Vec::with_capacity(n as usize), occupation: vec![0; GRID * GRID], sans_place: 0 };
    let bati = |i: i32, j: i32| {
        (0..GRID as i32).contains(&i)
            && (0..GRID as i32).contains(&j)
            && terrain.passage(i as usize, j as usize) & P_BATI != 0
    };
    for rang in 0..n as u64 {
        let mut place = None;
        for essai in 0..ESSAIS {
            let h = hash3(graine_tile, rang, essai);
            let a = (h & 0xFFFF) as f32 / 65_536.0;
            let b = (h >> 16 & 0xFFFF) as f32 / 65_536.0;
            let local = [a * 1000.0 - 500.0, b * 1000.0 - 500.0];
            let Some(c) = terrain::case(local) else { continue };
            let passage = terrain.passage[c];
            if passage & (P_ZOMBIE | P_PIETON) != P_ZOMBIE | P_PIETON || p.occupation[c] >= PAR_CASE {
                continue;
            }
            if essai < ESSAIS_PRES_DU_BATI {
                let (i, j) = ((c % GRID) as i32, (c / GRID) as i32);
                let pres = (-PRES..=PRES).any(|d| bati(i + d, j) || bati(i, j + d));
                // Loin du bâti : un essai sur 32 seulement est retenu.
                if !pres && h >> 32 & 31 != 0 {
                    continue;
                }
            }
            place = Some((local, uniform(h.rotate_left(17)) * std::f32::consts::TAU, c));
            break;
        }
        match place {
            Some((local, cap, c)) => {
                p.occupation[c] += 1;
                p.zombies.push((local, cap));
            }
            None => p.sans_place += 1,
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use beta1_common::geo::TileId;

    #[test]
    fn jamais_en_mer() {
        let p = poser(&Terrain::mer(TileId { ix: 0, iy: 0 }), 100, 7);
        assert!(p.zombies.is_empty());
        assert_eq!(p.sans_place, 100);
    }

    #[test]
    fn capacite_des_cases_respectee() {
        // Une bande de terre de 2 cases sur 500 : 3 000 places, 5 000 zombies.
        let mut t = Terrain::mer(TileId { ix: 0, iy: 0 });
        t.passage[..2 * GRID].iter_mut().for_each(|p| *p = P_ZOMBIE | P_PIETON);
        let p = poser(&t, 5000, 7);
        assert!(p.occupation.iter().all(|o| *o <= PAR_CASE));
        assert_eq!(p.occupation.iter().map(|o| *o as usize).sum::<usize>(), p.zombies.len());
        assert_eq!(p.zombies.len() as u32 + p.sans_place, 5000);
        assert!(p.sans_place >= 2000);
    }

    #[test]
    fn meme_graine_meme_placement() {
        let mut t = Terrain::mer(TileId { ix: 0, iy: 0 });
        t.passage.iter_mut().for_each(|p| *p = P_ZOMBIE | P_PIETON);
        let (a, b) = (poser(&t, 50, 1), poser(&t, 50, 1));
        assert_eq!(a.zombies.len(), 50);
        assert_eq!(a.zombies, b.zombies);
        assert_ne!(a.zombies, poser(&t, 50, 2).zombies);
    }
}
