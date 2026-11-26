//! La Bataille de France, beta 1 : serveur de simulation.
//!
//! - [`simulation`] : l'`App` Bevy sans rendu, cadencée à 20 Hz ;
//! - [`reseau`] : WebSocket, poignée de main, file d'envoi bornée par client.

pub mod reseau;
pub mod simulation;
