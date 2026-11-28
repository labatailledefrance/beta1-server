//! L'`App` de simulation et le flux d'un tick : le seul endroit où le lire (F-01).
//!
//! Bevy sans rendu (`MinimalPlugins`), exécuteur multi-thread, 20 Hz. Un tick
//! parcourt toujours les mêmes [`Phase`], dans le même ordre, comme un paquet
//! traverse une chaîne netfilter. Chaque plugin range ses systèmes dans une
//! phase ; à l'intérieur d'une phase, deux systèmes qui touchent la même
//! donnée doivent déclarer leur ordre : la détection d'ambiguïté de Bevy est
//! en **erreur**, le serveur refuse de démarrer sinon (A-12).
//!
//! ```text
//! FixedFirst   le tick avance
//! FixedUpdate  Entrees → Tiles → Perception → Decision → Mouvement → Combat
//!              → Logistique → Nettoyage → Sortie
//! ```

use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings, ScheduleLabel};
use bevy::prelude::*;

use beta1_common::geo::TileId;
use beta1_common::protocol::TICKS_PER_SECOND;

use crate::catalogue::{Catalogue, CataloguePlugin};
use crate::determinisme::DeterminismePlugin;
use crate::tiles::{Publication, TilesPlugin};

/// Phases d'un tick, dans l'ordre.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Ordres des joueurs, météo et autres entrées, appliqués à ce tick.
    Entrees,
    /// Activation et désactivation des tiles, terrain.
    Tiles,
    /// Ce que chacun voit, entend, sent.
    Perception,
    /// IA des zombies et des unités : buts, réflexes (D-02).
    Decision,
    /// Déplacements.
    Mouvement,
    /// Tirs, morsures, explosions.
    Combat,
    /// Consommation, production, transport.
    Logistique,
    /// Retrait des morts, fin des tâches.
    Nettoyage,
    /// Hash d'état, envoi aux clients.
    Sortie,
}

/// Tick courant ; seul `app.rs` l'écrit, en `FixedFirst`.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick(pub u64);

/// Durée d'un tick.
pub fn tick_duration() -> Duration {
    Duration::from_secs(1) / TICKS_PER_SECOND
}

/// Rend l'ambiguïté fatale dans un schedule du jeu.
fn ambiguite_en_erreur(app: &mut App, label: impl ScheduleLabel) {
    app.edit_schedule(label, |s| {
        s.set_build_settings(ScheduleBuildSettings { ambiguity_detection: LogLevel::Error, ..default() });
    });
}

/// Ce qui définit un monde au démarrage : catalogue, tiles, scénario.
pub struct Monde {
    /// Catalogue chargé et validé.
    pub catalogue: Catalogue,
    /// Publication de tiles ; `None` : tout est mer (tests, CI).
    pub publication: Option<Arc<Publication>>,
    /// Tiles actives au démarrage.
    pub tiles_initiales: Vec<TileId>,
}

impl Monde {
    /// Monde du scénario `scenario` du catalogue.
    pub fn new(catalogue: Catalogue, publication: Option<Arc<Publication>>, scenario: &str) -> Result<Monde, String> {
        let s = catalogue.scenario(scenario).ok_or_else(|| {
            let ids: Vec<&str> = catalogue.scenarios.iter().map(|s| s.id.as_str()).collect();
            format!("scénario « {scenario} » absent du catalogue (scénarios : {})", ids.join(", "))
        })?;
        let tiles_initiales = s.tiles_actives.clone();
        Ok(Monde { catalogue, publication, tiles_initiales })
    }
}

/// Monde sans données ni tile active : celui des tests d'un plugin seul.
impl From<Catalogue> for Monde {
    fn from(catalogue: Catalogue) -> Monde {
        Monde { catalogue, publication: None, tiles_initiales: Vec::new() }
    }
}

/// `App` de simulation, catalogue chargé. `paced` : boucle à 20 Hz réels
/// (sinon les ticks s'enchaînent sans attente, pour les tests et le
/// déterminisme).
pub fn app(monde: impl Into<Monde>, threads: usize, paced: bool) -> App {
    let monde = monde.into();
    let mut app = App::new();
    let wait = if paced { tick_duration() } else { Duration::ZERO };
    app.add_plugins(
        MinimalPlugins
            .set(TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(threads) })
            .set(ScheduleRunnerPlugin::run_loop(wait)),
    );
    app.insert_resource(Time::<Fixed>::from_duration(tick_duration()));
    ambiguite_en_erreur(&mut app, FixedFirst);
    ambiguite_en_erreur(&mut app, FixedPreUpdate);
    ambiguite_en_erreur(&mut app, FixedUpdate);
    ambiguite_en_erreur(&mut app, FixedPostUpdate);
    ambiguite_en_erreur(&mut app, FixedLast);
    use Phase::*;
    app.configure_sets(
        FixedUpdate,
        (Entrees, Tiles, Perception, Decision, Mouvement, Combat, Logistique, Nettoyage, Sortie).chain(),
    );
    app.init_resource::<Tick>().add_systems(FixedFirst, avancer);
    app.add_plugins((
        CataloguePlugin(monde.catalogue),
        TilesPlugin { publication: monde.publication, scenario: monde.tiles_initiales },
        DeterminismePlugin,
    ));
    app
}

fn avancer(mut tick: ResMut<Tick>) {
    tick.0 += 1;
}

/// Construit tous les schedules sans rien exécuter : rend les ambiguïtés et
/// autres erreurs de construction (`--check`, tests de chaque plugin).
pub fn verifier(app: &mut App) -> Result<(), String> {
    preparer(app);
    let world = app.world_mut();
    let labels: Vec<_> = world.resource::<Schedules>().iter().map(|(_, s)| s.label()).collect();
    let mut erreurs = Vec::new();
    for label in labels {
        // Un schedule à la fois : `initialize` a besoin de la ressource `Schedules`.
        world.schedule_scope(label, |world, schedule| {
            if let Err(e) = schedule.initialize(world) {
                erreurs.push(format!("{label:?} : {}", e.to_string(schedule.graph(), world)));
            }
        });
    }
    if erreurs.is_empty() { Ok(()) } else { Err(erreurs.join("\n")) }
}

/// Exécute `ticks` ticks en temps virtuel, sans attente (test de déterminisme).
pub fn avancer_de(app: &mut App, ticks: u64) {
    preparer(app);
    // Exactement un pas de FixedUpdate par update.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(tick_duration()));
    let fin = app.world().resource::<Tick>().0 + ticks;
    while app.world().resource::<Tick>().0 < fin {
        app.update();
    }
}

/// Termine la construction des plugins (`finish`, `cleanup`), comme le fait `App::run`.
fn preparer(app: &mut App) {
    if app.plugins_state() == bevy::app::PluginsState::Ready {
        app.finish();
        app.cleanup();
    }
}
