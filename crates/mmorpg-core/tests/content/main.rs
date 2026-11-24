//! mmorpg-core integration tests: Greyhaven's regions, creatures, encounters and the long journeys.
//!
//! One test binary per theme instead of one per file (#1087): each former
//! `tests/<name>.rs` is the module `<name>`, so its tests are `<name>::<test>`.

#[path = "../support/mod.rs"]
mod support;

mod cave_bat;
mod creature_streams;
mod discovery_caches;
mod evidence_quests;
mod farmland;
mod forest_lynx;
mod garrick;
mod greyhaven_vale;
mod greyhaven_vale_units;
mod highland_bandits;
mod highlands;
mod hunt_outing;
mod incident_receipts;
mod loot_content;
mod marsh;
mod marsh_adder;
mod marsh_creatures;
mod merchant_cart;
mod night_raid;
mod quest_chain;
mod raid_aftermath;
mod raid_forewarning;
mod redbrand_cavern;
mod redbrand_depths;
mod redbrand_depths_thorn;
mod redbrand_patrol;
mod redbrand_story;
mod region_quests;
mod reward_capacity;
mod river_crayfish;
mod southwood_stag;
mod starter_acceptance;
mod sunken_shrine;
mod wildlife_dens;
