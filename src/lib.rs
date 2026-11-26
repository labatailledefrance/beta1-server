//! La Bataille de France, beta 1 : serveur de simulation.
//!
//! - [`simulation`] : l'`App` Bevy sans rendu, cadencée à 20 Hz ;
//! - [`network`] : WebSocket, handshake, send queue bornée par client.

pub mod network;
pub mod simulation;
