//! Serveur de La Bataille de France (beta 1).

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;

use beta1_server::app;
use beta1_server::catalogue;
use beta1_server::determinisme::StateHash;
use beta1_server::reseau::{self, ReseauPlugin, Shared};
use beta1_server::tiles::Publication;

#[derive(Parser)]
#[command(version, about = "Serveur de simulation de La Bataille de France (beta 1)")]
struct Args {
    /// Adresse d'écoute du WebSocket.
    #[arg(long, default_value = "0.0.0.0:7425")]
    listen: String,
    /// Threads de la simulation.
    #[arg(long, default_value_t = 16)]
    threads: usize,
    /// Dossier des mods (un sous-dossier par mod).
    #[arg(long, env = "MODS", default_value = "mods")]
    mods: PathBuf,
    /// Mods à charger, dans l'ordre déclaré (les dépendances passent avant).
    #[arg(long = "mod", value_name = "ID", value_delimiter = ',', default_value = "base")]
    liste: Vec<String>,
    /// Publication de tiles (dossier qui contient COMPLET et MANIFESTE) ; sans elle, tout est mer.
    #[arg(long, env = "TILES")]
    tiles: Option<PathBuf>,
    /// Scénario du catalogue à jouer.
    #[arg(long, default_value = "officiel")]
    scenario: String,
    /// Valide le catalogue et l'ordre des systèmes (ambiguïtés), puis sort.
    #[arg(long)]
    check: bool,
    /// Écrit les JSON Schemas des fichiers de mods dans ce dossier, puis sort.
    #[arg(long, value_name = "DOSSIER")]
    schema: Option<PathBuf>,
    /// Test de déterminisme : exécute N ticks sans attente, affiche le hash d'état et sort.
    #[arg(long, value_name = "TICKS")]
    determinism: Option<u64>,
}

fn main() -> ExitCode {
    match lancer(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e.to_string().trim_end());
            ExitCode::FAILURE
        }
    }
}

fn lancer(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dossier) = args.schema {
        std::fs::create_dir_all(&dossier)?;
        for (nom, contenu) in catalogue::schemas() {
            std::fs::write(dossier.join(nom), contenu)?;
        }
        return Ok(());
    }
    let catalogue = catalogue::charger(&args.mods, &args.liste)?;
    let hash = blake3::Hash::from(catalogue.hash());
    let publication = match &args.tiles {
        Some(d) => {
            let debut = Instant::now();
            let p = Publication::ouvrir(d, args.threads)?;
            eprintln!(
                "tiles {} : {} départements, {} paquets, {} tiles, index en {:.1} s",
                p.id,
                p.departements.len(),
                p.nb_paquets(),
                p.len(),
                debut.elapsed().as_secs_f32()
            );
            Some(Arc::new(p))
        }
        None => {
            eprintln!("aucune publication de tiles (--tiles ou TILES) : tout est mer");
            None
        }
    };
    let monde = app::Monde::new(catalogue, publication, &args.scenario)?;
    let initiales = monde.scenario.as_ref().map_or(0, |s| s.tiles_actives.len());
    if args.check {
        let mut app = app::app(monde, args.threads, false);
        app::verifier(&mut app)?;
        println!(
            "catalogue {hash} : valide ; scénario « {} » ({initiales} tiles) ; ordre des systèmes : sans ambiguïté",
            args.scenario
        );
        return Ok(());
    }
    if let Some(n) = args.determinism {
        let mut app = app::app(monde, args.threads, false);
        app::avancer_de(&mut app, n);
        println!("{:016x}", app.world().resource::<StateHash>().0);
        return Ok(());
    }
    eprintln!("catalogue {hash} ; scénario « {} » : {initiales} tiles actives au démarrage", args.scenario);
    let shared = Arc::new(Shared::new(monde.camera()));
    reseau::listen(TcpListener::bind(&args.listen)?, shared.clone());
    let mut app = app::app(monde, args.threads, true);
    app.add_plugins(ReseauPlugin(shared));
    app::verifier(&mut app)?;
    app.run();
    Ok(())
}
