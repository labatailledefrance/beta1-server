//! La Bataille de France, beta 1 : serveur de simulation.
//!
//! Un plugin Bevy par domaine (F-01) : il possède ses composants et ses
//! messages, lit ceux des autres et n'écrit que les siens.
//!
//! - [`app`] : l'`App` sans rendu à 20 Hz et les phases d'un tick ;
//! - [`catalogue`] : mods, objets TOML, textes Fluent, validation, compilation (E-02, F-03) ;
//! - [`tiles`] : publication de tiles, terrain, activation à tick fixé ;
//! - [`determinisme`] : hash d'état ;
//! - [`reseau`] : WebSocket, handshake, send queue bornée par client.

pub mod app;
pub mod catalogue;
pub mod determinisme;
pub mod reseau;
pub mod tiles;
