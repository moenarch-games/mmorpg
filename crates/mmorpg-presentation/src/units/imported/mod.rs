//! Which imported model presents a unit, its clip names and the per-unit clip clock.
//!
//! Players use asset-tooling's Ranger humanoid (Atlas 6.3.7); wolves, vermin and boars use the
//! bundled Quaternius CC0 animals (Atlas 6.3.6); the named Greyhaven residents wear the Ranger in
//! their appearance presets (#418) and the humanoid enemies (marauders, Redbrands) game-authored
//! presets (#39, `presets`) where the client draws them; other humanoid creatures and NPCs keep their
//! procedural models. Each client imports and skins the models with its own
//! provider; this module only decides what to pose. Presentation only, never read by gameplay.

mod catalogue;
mod clips;
mod layers;
mod playback;

pub use catalogue::*;
pub use clips::*;
pub use layers::*;
pub use playback::*;
