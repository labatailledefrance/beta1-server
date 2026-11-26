//! Simulation : `App` Bevy sans rendu (MinimalPlugins), exécuteur multi-thread.
//!
//! Squelette : un compteur de ticks et un hash d'état. Les systèmes de jeu de
//! P-24 à P-26 viendront s'y brancher ; la règle reste la même : l'état ne
//! dépend ni de l'horloge ni du nombre de threads (A-12).

use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use beta1_common::protocol::{ServerMessage, TICKS_PER_SECOND};
use beta1_common::rng::hash3;

use crate::network::Shared;

/// Tick courant.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Tick(pub u64);

/// Hash de l'état, mis à jour à chaque tick.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct StateHash(pub u64);

/// Durée d'un tick.
pub fn tick_duration() -> Duration {
    Duration::from_secs(1) / TICKS_PER_SECOND
}

/// `App` de simulation. `paced` : boucle à 20 Hz réels (sinon les ticks
/// s'enchaînent sans attente, pour les tests et le déterminisme).
pub fn app(threads: usize, paced: bool) -> App {
    let mut app = App::new();
    let wait = if paced { tick_duration() } else { Duration::ZERO };
    app.add_plugins(
        MinimalPlugins
            .set(TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(threads) })
            .set(ScheduleRunnerPlugin::run_loop(wait)),
    );
    app.init_resource::<Tick>().init_resource::<StateHash>().add_systems(FixedUpdate, step);
    app.insert_resource(Time::<Fixed>::from_duration(tick_duration()));
    app
}

fn step(mut tick: ResMut<Tick>, mut hash: ResMut<StateHash>) {
    tick.0 += 1;
    hash.0 = hash3(hash.0, tick.0, 0);
}

/// Branche le réseau : chaque tick publie le tick courant et broadcast l'état.
pub fn attach_network(app: &mut App, shared: Arc<Shared>) {
    app.insert_resource(SharedRes(shared)).add_systems(FixedPostUpdate, broadcast_state);
}

#[derive(Resource)]
struct SharedRes(Arc<Shared>);

fn broadcast_state(tick: Res<Tick>, shared: Res<SharedRes>) {
    shared.0.publish_tick(tick.0);
    // État à 10 Hz.
    if tick.0.is_multiple_of(2) {
        shared.0.broadcast(ServerMessage::State { tick: tick.0 }.encode());
    }
}

/// Exécute `ticks` ticks sans attente et rend le hash d'état (test de déterminisme).
pub fn state_hash_after(ticks: u64, threads: usize) -> u64 {
    let mut app = app(threads, false);
    // Temps virtuel : exactement un pas de FixedUpdate par update.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(tick_duration()));
    while app.world().resource::<Tick>().0 < ticks {
        app.update();
    }
    app.world().resource::<StateHash>().0
}
