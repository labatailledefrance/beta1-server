//! Serveur de La Bataille de France (beta 1).

use std::net::TcpListener;
use std::sync::Arc;

use clap::Parser;

use beta1_server::reseau::{self, Partage};
use beta1_server::simulation;

#[derive(Parser)]
#[command(version, about = "Serveur de simulation de La Bataille de France (beta 1)")]
struct Options {
    /// Adresse d'écoute du WebSocket.
    #[arg(long, default_value = "0.0.0.0:7425")]
    ecoute: String,
    /// Threads de la simulation.
    #[arg(long, default_value_t = 16)]
    threads: usize,
    /// Test de déterminisme : exécute N ticks sans attente, affiche l'empreinte et sort.
    #[arg(long, value_name = "TICKS")]
    determinisme: Option<u64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let o = Options::parse();
    if let Some(n) = o.determinisme {
        println!("{:016x}", simulation::empreinte_apres(n, o.threads));
        return Ok(());
    }
    let partage = Arc::new(Partage::default());
    reseau::demarrer(TcpListener::bind(&o.ecoute)?, partage.clone());
    let mut app = simulation::app(o.threads, true);
    simulation::brancher_reseau(&mut app, partage);
    app.run();
    Ok(())
}
