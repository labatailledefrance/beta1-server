//! Hash d'état (A-12) : même valeur quel que soit le nombre de threads, ce
//! que la CI vérifie à 1 et 16 threads.
//!
//! Il part du hash du catalogue et de l'identifiant de la publication de
//! tiles : deux serveurs aux mods ou aux données différents n'ont jamais le
//! même hash d'état. Chaque plugin y versera son état, dans un ordre
//! fixe (entités triées par identifiant, jamais par ordre d'itération).

use bevy::prelude::*;

use beta1_common::rng::hash3;

use crate::app::{Phase, Tick};
use crate::catalogue::Catalogue;
use crate::tiles::{Donnees, TilesActives};

/// Hash de l'état, mis à jour à la fin de chaque tick.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHash(pub u64);

/// Plugin du hash d'état ; demande les ressources [`Catalogue`] et [`Donnees`].
pub struct DeterminismePlugin;

impl Plugin for DeterminismePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StateHash>().add_systems(FixedUpdate, hacher.in_set(Phase::Sortie));
    }

    fn finish(&self, app: &mut App) {
        let catalogue = app.world().resource::<Catalogue>().hash_court();
        let publication = match &app.world().resource::<Donnees>().0 {
            Some(p) => u64::from_le_bytes(blake3::hash(p.id.as_bytes()).as_bytes()[..8].try_into().unwrap_or([0; 8])),
            None => 0,
        };
        app.insert_resource(StateHash(hash3(catalogue, publication, 0)));
    }
}

fn hacher(tick: Res<Tick>, actives: Res<TilesActives>, mut hash: ResMut<StateHash>) {
    let mut h = hash3(hash.0, tick.0, actives.0.len() as u64);
    for t in actives.0.keys() {
        h = hash3(h, t.ix as u32 as u64, t.iy as u32 as u64);
    }
    hash.0 = h;
}
