//! Simulation : `App` Bevy sans rendu (MinimalPlugins), exécuteur multi-thread.
//!
//! Squelette : un compteur de ticks et une empreinte d'état. Les systèmes de
//! jeu de P-24 à P-26 viendront s'y brancher ; la règle reste la même : l'état
//! ne dépend ni de l'horloge ni du nombre de threads (A-12).

use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use beta1_common::alea::hache;
use beta1_common::protocole::{DuServeur, TICKS_PAR_SECONDE};

use crate::reseau::Partage;

/// Tick courant.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Tick(pub u64);

/// Empreinte de l'état, mise à jour à chaque tick.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Empreinte(pub u64);

/// Durée d'un tick.
pub fn pas() -> Duration {
    Duration::from_secs(1) / TICKS_PAR_SECONDE
}

/// `App` de simulation. `cadence` : boucle à 20 Hz réels (sinon les ticks
/// s'enchaînent sans attente, pour les tests et le déterminisme).
pub fn app(threads: usize, cadence: bool) -> App {
    let mut app = App::new();
    let attente = if cadence { pas() } else { Duration::ZERO };
    app.add_plugins(
        MinimalPlugins
            .set(TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(threads) })
            .set(ScheduleRunnerPlugin::run_loop(attente)),
    );
    app.init_resource::<Tick>().init_resource::<Empreinte>().add_systems(FixedUpdate, avancer);
    app.insert_resource(Time::<Fixed>::from_duration(pas()));
    app
}

fn avancer(mut tick: ResMut<Tick>, mut e: ResMut<Empreinte>) {
    tick.0 += 1;
    e.0 = hache(e.0, tick.0, 0);
}

/// Branche le réseau : chaque tick publie le tick courant et diffuse l'état.
pub fn brancher_reseau(app: &mut App, partage: Arc<Partage>) {
    app.insert_resource(PartageRes(partage)).add_systems(FixedPostUpdate, diffuser);
}

#[derive(Resource)]
struct PartageRes(Arc<Partage>);

fn diffuser(tick: Res<Tick>, p: Res<PartageRes>) {
    p.0.publier_tick(tick.0);
    // État à 10 Hz.
    if tick.0.is_multiple_of(2) {
        p.0.diffuser(DuServeur::Etat { tick: tick.0 }.encoder());
    }
}

/// Exécute `ticks` ticks sans attente et rend l'empreinte (test de déterminisme).
pub fn empreinte_apres(ticks: u64, threads: usize) -> u64 {
    let mut app = app(threads, false);
    // Temps virtuel : exactement un pas de FixedUpdate par update.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(pas()));
    while app.world().resource::<Tick>().0 < ticks {
        app.update();
    }
    app.world().resource::<Empreinte>().0
}
