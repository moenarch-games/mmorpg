//! The browser's local single-player zone host ([ADR 0002]): the shared `mmorpg-core` zone
//! simulation hosted in the page like a listen server, with the review scenarios and the Action
//! Lab that drive it. `mmorpg-wasm` exports it to JavaScript.
//!
//! This is the gameplay half of the browser demo and owns no rules: gameplay lives in
//! `mmorpg-core`, wire formats in `mmorpg-protocol`, the Redbrand Depths run directory in
//! `mmorpg-control-plane::instance`. It never links presentation (`mmorpg-scenery`,
//! `mmorpg-presentation`) or the client rules (`mmorpg-client-model`), which
//! `mmorpg-game-server/tests/dependency_boundary.rs` checks over the resolved graph.
//!
//! [ADR 0002]: https://github.com/moenarch-games/mmorpg/blob/main/docs/adr/0002-browser-embeds-zone-simulation.md
#![forbid(unsafe_code)]

pub mod action_lab;
pub mod arena;
pub mod host;
pub mod lab_diagnostics;
pub mod lab_experiment;
pub mod lab_promotion;
pub mod scenario;
