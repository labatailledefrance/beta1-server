//! Hash d'état (A-12) : même valeur quel que soit le nombre de threads, ce
//! que la CI vérifie à 1 et 16 threads.
//!
//! Il part du hash du catalogue : deux serveurs aux mods différents n'ont
//! jamais le même hash d'état. Chaque plugin y versera son état, dans un ordre
//! fixe (entités triées par identifiant, jamais par ordre d'itération).

use bevy::prelude::*;

use beta1_common::rng::hash3;

use crate::app::{Phase, Tick};
use crate::catalogue::Catalogue;

/// Hash de l'état, mis à jour à la fin de chaque tick.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHash(pub u64);

/// Plugin du hash d'état ; demande la ressource [`Catalogue`].
pub struct DeterminismePlugin;

impl Plugin for DeterminismePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StateHash>().add_systems(FixedUpdate, hacher.in_set(Phase::Sortie));
    }

    fn finish(&self, app: &mut App) {
        let graine = app.world().resource::<Catalogue>().hash_court();
        app.insert_resource(StateHash(graine));
    }
}

fn hacher(tick: Res<Tick>, mut hash: ResMut<StateHash>) {
    hash.0 = hash3(hash.0, tick.0, 0);
}
