//! Zombies des tiles actives (ZOMBIES.md, P-04, P-26).
//!
//! Les zombies d'une tile active sont rangés dans un composant [`Zombies`]
//! posé sur l'entité de la tile : une liste et une grille d'occupation (au
//! plus [`PAR_CASE`] zombies par case de 2 m). C'est le rangement de P-26,
//! qui tenait 27 000 zombies en 2,3 ms par tick : une entité par zombie
//! coûterait plus cher en mémoire et en ordonnancement, pour rien.
//!
//! Un tick, en phase Mouvement :
//! 1. **décision**, en parallèle par tile : chaque zombie ne lit que sa tile ;
//! 2. **application**, dans l'ordre des tiles puis des zombies ;
//! 3. **passages de tile**, triés, après tous les déplacements.
//!
//! Le résultat ne dépend donc pas du nombre de threads. Le hasard est un
//! hachage de (graine, identifiant, tick).
//!
//! Pour l'instant, les zombies errent : la perception, le relais de l'alerte
//! et les champs de poursuite viendront avec les humains à percevoir.

pub mod placement;

use bevy::prelude::*;

use beta1_common::geo::Pos;
use beta1_common::protocol::TICKS_PER_SECOND;
use beta1_common::rng::{hash3, uniform};

use crate::app::{Phase, ScenarioEnCours, Tick};
use crate::catalogue::{Catalogue, IdZombie, TypeZombie};
use crate::tiles::terrain::{self, P_EAU, P_ZOMBIE, Terrain};
use crate::tiles::{MiseEnService, TileActive, TilesActives};

/// Zombies au plus par case de 2 m.
pub const PAR_CASE: u8 = 3;
/// Durée d'un tick (s).
pub const DT: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// État d'un zombie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etat {
    /// Il erre, au hasard, à petite vitesse.
    Errance,
}

/// Un zombie.
#[derive(Debug, Clone, PartialEq)]
pub struct Zombie {
    /// Identifiant unique dans le monde.
    pub id: u32,
    /// Type (catalogue).
    pub genre: IdZombie,
    /// Position locale dans sa tile (m).
    pub local: [f32; 2],
    /// Cap (radians).
    pub cap: f32,
    /// État.
    pub etat: Etat,
}

/// Décision d'un zombie pour ce tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Decision {
    cap: f32,
    pas: [f32; 2],
}

/// Zombies d'une tile active.
#[derive(Component, Debug, Clone)]
pub struct Zombies {
    /// Zombies, dans l'ordre d'arrivée.
    pub liste: Vec<Zombie>,
    /// Zombies par case.
    pub occupation: Vec<u8>,
    /// Zombies de la tile sans place (comptés, jamais perdus).
    pub sans_place: u32,
    decisions: Vec<Decision>,
}

/// Prochain identifiant de zombie.
#[derive(Resource, Debug, Default)]
pub struct ProchainZombie(pub u32);

/// Plugin des zombies.
pub struct ZombiesPlugin;

impl Plugin for ZombiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProchainZombie>()
            .add_systems(FixedUpdate, peupler.in_set(Phase::Tiles).after(MiseEnService))
            .add_systems(FixedUpdate, (decider, deplacer).chain().in_set(Phase::Mouvement));
    }
}

/// Zombies d'une tile au départ : habitants × densité, dans les zones peuplées.
pub fn population(scenario: &crate::catalogue::Scenario, t: &Terrain) -> u32 {
    if !scenario.zones_peuplees.contains(&t.id.zone()) {
        return 0;
    }
    (t.habitants as f32 * scenario.densite_zombies).round() as u32
}

/// Place les zombies des tiles qui viennent d'entrer en service, dans l'ordre des tiles.
fn peupler(
    scenario: Res<ScenarioEnCours>,
    actives: Res<TilesActives>,
    tiles: Query<&TileActive, Without<Zombies>>,
    mut prochain: ResMut<ProchainZombie>,
    mut commands: Commands,
) {
    for (t, &e) in &actives.0 {
        let Ok(tile) = tiles.get(e) else { continue };
        let (n, graine, genre) = match &scenario.0 {
            Some(s) => (population(s, &tile.terrain), s.graine, s.type_zombie),
            None => (0, 0, IdZombie(0)),
        };
        let places = placement::poser(&tile.terrain, n, hash3(graine, t.ix as u32 as u64, t.iy as u32 as u64));
        let liste = places
            .zombies
            .into_iter()
            .map(|(local, cap)| {
                let id = prochain.0;
                prochain.0 = prochain.0.wrapping_add(1);
                Zombie { id, genre, local, cap, etat: Etat::Errance }
            })
            .collect();
        commands.entity(e).insert(Zombies {
            liste,
            occupation: places.occupation,
            sans_place: places.sans_place,
            decisions: Vec::new(),
        });
    }
}

/// Décision en errance : nouveau cap de temps en temps, petit pas devant soi.
fn decision_errance(z: &Zombie, tz: &TypeZombie, terrain: &Terrain, tick: u64, graine: u64) -> Decision {
    let mut cap = z.cap;
    if (z.id as u64 + tick).is_multiple_of(tz.periode_cap as u64) {
        cap = uniform(hash3(graine, z.id as u64, tick)) * std::f32::consts::TAU;
    }
    let mut v = tz.vitesse_errance;
    if terrain::case(z.local).is_some_and(|k| terrain.passage[k] & P_EAU != 0) {
        v *= tz.facteur_eau;
    }
    // Chacun sa vitesse, de -écart à +écart.
    v *= 1.0 - tz.ecart_vitesse + 2.0 * tz.ecart_vitesse * (z.id % 64) as f32 / 64.0;
    let (s, c) = cap.sin_cos();
    Decision { cap, pas: [c * v * DT, s * v * DT] }
}

/// Phase 1 : décisions, en parallèle par tile (lecture de sa seule tile).
fn decider(
    tick: Res<Tick>,
    catalogue: Res<Catalogue>,
    scenario: Res<ScenarioEnCours>,
    mut tiles: Query<(&TileActive, &mut Zombies)>,
) {
    let graine = scenario.0.as_ref().map_or(0, |s| s.graine);
    // Une tile par lot : une tile, c'est déjà des milliers de zombies.
    let lots = bevy::ecs::batching::BatchingStrategy::fixed(1);
    tiles.par_iter_mut().batching_strategy(lots).for_each(|(tile, mut z)| {
        let z = &mut *z;
        z.decisions.clear();
        for zombie in &z.liste {
            let tz = catalogue.zombie(zombie.genre);
            z.decisions.push(decision_errance(zombie, tz, &tile.terrain, tick.0, graine));
        }
    });
}

/// Passage de tile demandé : (tile de départ, rang du zombie, arrivée, case de départ).
struct Sortie {
    depart: Entity,
    rang: usize,
    arrivee: Pos,
    case: usize,
}

/// Phases 2 et 3 : déplacements dans l'ordre des tiles, puis passages de tile, triés.
fn deplacer(actives: Res<TilesActives>, mut tiles: Query<(&TileActive, &mut Zombies)>) {
    let mut sorties: Vec<Sortie> = Vec::new();
    for &e in actives.0.values() {
        let Ok((tile, mut z)) = tiles.get_mut(e) else { continue };
        let z = &mut *z;
        let terrain = &tile.terrain;
        for (rang, (zombie, d)) in z.liste.iter_mut().zip(&z.decisions).enumerate() {
            zombie.cap = d.cap;
            let Some(k0) = terrain::case(zombie.local) else { continue };
            let mut bouge = false;
            let mut sort = false;
            // Le pas entier, sinon glisser le long d'un obstacle (x seul, puis y seul).
            for pas in [d.pas, [d.pas[0], 0.0], [0.0, d.pas[1]]] {
                if pas == [0.0, 0.0] {
                    continue;
                }
                let n = Pos { tile: tile.id, local: [zombie.local[0] + pas[0], zombie.local[1] + pas[1]] }.normalized();
                if n.tile != tile.id {
                    sorties.push(Sortie { depart: e, rang, arrivee: n, case: k0 });
                    sort = true;
                    break;
                }
                let Some(k1) = terrain::case(n.local) else { continue };
                if terrain.passage[k1] & P_ZOMBIE == 0 || (k1 != k0 && z.occupation[k1] >= PAR_CASE) {
                    continue;
                }
                z.occupation[k0] -= 1;
                z.occupation[k1] += 1;
                zombie.local = n.local;
                zombie.cap = d.pas[1].atan2(d.pas[0]);
                bouge = true;
                break;
            }
            if !bouge && !sort {
                // Bloqué : il se détourne.
                zombie.cap += 2.0;
            }
        }
    }
    // Passages de tile, dans l'ordre des tiles de départ puis des zombies.
    let mut partis: Vec<(Entity, usize)> = Vec::new();
    for s in &sorties {
        let arrivee = actives.0.get(&s.arrivee.tile).copied();
        let place = arrivee.and_then(|a| {
            let (tile, z) = tiles.get(a).ok()?;
            let k1 = terrain::case(s.arrivee.local)?;
            (tile.terrain.passage[k1] & P_ZOMBIE != 0 && z.occupation[k1] < PAR_CASE).then_some((a, k1))
        });
        let Ok((_, mut depart)) = tiles.get_mut(s.depart) else { continue };
        let Some((a, k1)) = place else {
            // Bord d'une tile inactive, ou case pleine : il se détourne (bord réfléchissant, P-04).
            depart.liste[s.rang].cap += 2.0;
            continue;
        };
        depart.occupation[s.case] -= 1;
        let mut zombie = depart.liste[s.rang].clone();
        zombie.local = s.arrivee.local;
        partis.push((s.depart, s.rang));
        if let Ok((_, mut z)) = tiles.get_mut(a) {
            z.occupation[k1] += 1;
            z.liste.push(zombie);
        }
    }
    // Retrait des partis, en ordre décroissant de rang pour garder les indices valides.
    partis.sort_by_key(|p| std::cmp::Reverse(p.1));
    for (e, rang) in partis {
        if let Ok((_, mut z)) = tiles.get_mut(e) {
            z.liste.remove(rang);
        }
    }
}
