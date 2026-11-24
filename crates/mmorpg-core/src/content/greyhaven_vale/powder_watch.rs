//! The Redbrand powder watch (#954): a positioning challenge on the camp's northern powder
//! barrels (#288), with no new rules. Barrels 923 and 924 stand 0.7 m apart beside the northern
//! tents, so one going up takes the other with it. Bandit 406 keeps watch 4.5 m from them, inside
//! the 5 m blast; bandit 404 stands 5.5 m off, just beyond it; bandit 410 stands 11 m north
//! behind the northern tent (collider 420), out of reach and in its shadow. Lighting the powder
//! at once catches only the one beside it and spends the barrels. Drawing 404 in beside 406
//! (a Frost Nova holds them there) and lighting it from outside the blast catches both, while
//! 410 is untouched. Spent barrels stand again after their respawn time, as every breakable
//! prop does. The three hold their posts so the setup reads the same every visit.

use crate::CreatureId;

/// The powder barrels, in chain order.
pub const BARRELS: [CreatureId; 2] = [CreatureId::new(923), CreatureId::new(924)];
/// The bandit beside the powder and the one just beyond its blast.
pub const PAIR: [CreatureId; 2] = [CreatureId::new(406), CreatureId::new(404)];
/// The bandit behind the western tent.
pub const COVERED: CreatureId = CreatureId::new(410);
/// The watch idles within 0.3 m of its posts, so the blast's reach of each stays the same.
pub const POST_WANDER_RADIUS_UNITS: i32 = 30;

/// Whether `spawn` is one of the powder watch's bandits.
#[must_use]
pub fn holds_post(spawn: u32) -> bool {
    PAIR.iter().chain([&COVERED]).any(|id| id.get() == spawn)
}
