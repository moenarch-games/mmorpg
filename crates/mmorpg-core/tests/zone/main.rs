//! mmorpg-core integration tests: zone services: interest, handoff, recovery, items, chat, parties and progression.
//!
//! One test binary per theme instead of one per file (#1087): each former
//! `tests/<name>.rs` is the module `<name>`, so its tests are `<name>::<test>`.

#[path = "../support/mod.rs"]
mod support;

mod chat;
mod content_identity;
mod corpse_loot;
mod crafting;
mod discovery_rewards;
mod emote;
mod ephemeral_builds;
mod equipment;
mod handoff;
mod interest;
mod inventory;
mod inventory_authority;
mod item_finds;
mod loot;
mod loot_settlement;
mod parties;
mod presentation_gameplay;
mod presentation_observation;
mod private_home;
mod progression;
mod progression_rules;
mod quests;
mod recovery;
mod routines;
mod vendor;
mod waystones;
mod workload_stats;
