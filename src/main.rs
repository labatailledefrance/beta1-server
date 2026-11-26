//! Serveur de La Bataille de France (beta 1).

use std::net::TcpListener;
use std::sync::Arc;

use clap::Parser;

use beta1_server::network::{self, Shared};
use beta1_server::simulation;

#[derive(Parser)]
#[command(version, about = "Serveur de simulation de La Bataille de France (beta 1)")]
struct Args {
    /// Adresse d'écoute du WebSocket.
    #[arg(long, default_value = "0.0.0.0:7425")]
    listen: String,
    /// Threads de la simulation.
    #[arg(long, default_value_t = 16)]
    threads: usize,
    /// Test de déterminisme : exécute N ticks sans attente, affiche le hash d'état et sort.
    #[arg(long, value_name = "TICKS")]
    determinism: Option<u64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(n) = args.determinism {
        println!("{:016x}", simulation::state_hash_after(n, args.threads));
        return Ok(());
    }
    let shared = Arc::new(Shared::default());
    network::listen(TcpListener::bind(&args.listen)?, shared.clone());
    let mut app = simulation::app(args.threads, true);
    simulation::attach_network(&mut app, shared);
    app.run();
    Ok(())
}
