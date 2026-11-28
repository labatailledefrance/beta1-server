//! Tiles : publication, terrain, activation (A-01, P-04, H-06).
//!
//! Une tile s'active sur demande (message [`ActiverTile`]) : sa lecture et son
//! décodage partent en tâche de fond, et le résultat entre en service au tick
//! `demande + DELAI_ACTIVATION`, **que le tick attend** si le disque est lent.
//! Comme un commit à une hauteur de bloc fixée d'avance : le contenu du monde
//! à un tick donné ne dépend ni des threads, ni du disque, ni de la charge.
//!
//! Une tile active est une entité ([`TileActive`]) ; [`TilesActives`] les
//! range par `TileId`, pour tout parcours qui doit être déterministe.
//!
//! Pour l'instant, les tiles actives sont celles du scénario ; viendront
//! l'activation par les unités, la désactivation et la sauvegarde (A-13).

pub mod publication;
pub mod terrain;

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on};

use beta1_common::geo::TileId;

use crate::app::{Phase, Tick};
pub use publication::Publication;
pub use terrain::Terrain;

/// Ticks entre la demande d'activation et la mise en service (0,5 s à 20 Hz).
pub const DELAI_ACTIVATION: u64 = 10;

/// Mise en service des tiles : un plugin qui réagit à une nouvelle tile active se range après.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MiseEnService;

/// Demande d'activation d'une tile.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiverTile(pub TileId);

/// Tile active.
#[derive(Component, Debug, Clone)]
pub struct TileActive {
    /// Tile.
    pub id: TileId,
    /// Terrain, partagé (immuable).
    pub terrain: Arc<Terrain>,
    /// Tick de mise en service.
    pub depuis: u64,
}

/// Tiles actives, triées : la seule façon déterministe de les parcourir.
#[derive(Resource, Default, Debug)]
pub struct TilesActives(pub BTreeMap<TileId, Entity>);

/// Publication lue (`None` : monde sans données, tout est mer).
#[derive(Resource, Clone)]
pub struct Donnees(pub Option<Arc<Publication>>);

/// Tiles du scénario, activées au premier tick.
#[derive(Resource, Clone, Default)]
struct TilesDuScenario(Vec<TileId>);

struct Chargement {
    pret_au: u64,
    tache: Task<Result<Terrain, String>>,
}

/// Chargements en cours, par tile.
#[derive(Resource, Default)]
struct Chargements(BTreeMap<TileId, Chargement>);

/// Plugin des tiles.
pub struct TilesPlugin {
    /// Publication ouverte, ou aucune.
    pub publication: Option<Arc<Publication>>,
    /// Tiles actives au démarrage.
    pub scenario: Vec<TileId>,
}

impl Plugin for TilesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ActiverTile>()
            .insert_resource(Donnees(self.publication.clone()))
            .insert_resource(TilesDuScenario(self.scenario.clone()))
            .init_resource::<TilesActives>()
            .init_resource::<Chargements>()
            .add_systems(FixedUpdate, demander_le_scenario.in_set(Phase::Entrees).run_if(premier_tick))
            .add_systems(FixedUpdate, (lancer, mettre_en_service.in_set(MiseEnService)).chain().in_set(Phase::Tiles));
    }
}

fn premier_tick(tick: Res<Tick>) -> bool {
    tick.0 == 1
}

fn demander_le_scenario(scenario: Res<TilesDuScenario>, mut demandes: MessageWriter<ActiverTile>) {
    demandes.write_batch(scenario.0.iter().map(|t| ActiverTile(*t)));
}

/// Lance la lecture des tiles demandées, ni actives ni déjà en chargement.
fn lancer(
    tick: Res<Tick>,
    donnees: Res<Donnees>,
    actives: Res<TilesActives>,
    mut chargements: ResMut<Chargements>,
    mut demandes: MessageReader<ActiverTile>,
) {
    for &ActiverTile(t) in demandes.read() {
        if actives.0.contains_key(&t) || chargements.0.contains_key(&t) {
            continue;
        }
        let publication = donnees.0.clone();
        let tache = IoTaskPool::get().spawn(async move { charger(publication.as_deref(), t) });
        chargements.0.insert(t, Chargement { pret_au: tick.0 + DELAI_ACTIVATION, tache });
    }
}

/// Lit et décode une tile (tâche de fond).
pub fn charger(publication: Option<&Publication>, t: TileId) -> Result<Terrain, String> {
    let octets = match publication {
        Some(p) => p.lire(t).map_err(|e| e.to_string())?,
        None => None,
    };
    match octets {
        Some(b) => Terrain::decoder(t, &b).map_err(|e| format!("tile ({}, {}) : {e}", t.ix, t.iy)),
        None => Ok(Terrain::mer(t)),
    }
}

/// Met en service, dans l'ordre des tiles, celles dont le tick est venu ; attend leur lecture au besoin.
fn mettre_en_service(
    tick: Res<Tick>,
    mut chargements: ResMut<Chargements>,
    mut actives: ResMut<TilesActives>,
    mut commands: Commands,
    mut sortie: MessageWriter<AppExit>,
) {
    let prets: Vec<TileId> = chargements.0.iter().filter(|(_, c)| c.pret_au <= tick.0).map(|(t, _)| *t).collect();
    for t in prets {
        let Some(c) = chargements.0.remove(&t) else { continue };
        match block_on(c.tache) {
            Ok(terrain) => {
                let e = commands.spawn(TileActive { id: t, terrain: Arc::new(terrain), depuis: tick.0 }).id();
                actives.0.insert(t, e);
            }
            Err(e) => {
                // Une publication contrôlée ne devrait jamais en arriver là : on s'arrête
                // plutôt que de simuler un monde différent de celui des données.
                error!("{e}");
                sortie.write(AppExit::error());
            }
        }
    }
}
