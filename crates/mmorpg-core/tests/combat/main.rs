//! mmorpg-core integration tests: classes, abilities, the action graph, weapons and class fights.
//!
//! One test binary per theme instead of one per file (#1087): each former
//! `tests/<name>.rs` is the module `<name>`, so its tests are `<name>::<test>`.

#[path = "../support/mod.rs"]
mod support;

mod abilities;
mod abilities_land;
mod action_authored;
mod action_classes;
mod action_graph;
mod action_latency;
mod action_outcomes;
mod action_swimming;
mod action_traversal;
mod action_windows;
mod arcanist_group_control;
mod cleave;
mod combat;
mod combo;
mod companions;
mod coop_root_pierce;
mod corpse_payoffs;
mod creature_attacks;
mod directed_strike;
mod divine_reckoning;
mod druid_multi_threat;
mod druid_wolf_form;
mod firing_line;
mod flank;
mod flank_circle;
mod forced_movement;
mod guard;
mod guard_buffer;
mod hit_rewind;
mod magic_missiles;
mod minions;
mod movement_melee;
mod movement_spells;
mod movement_strikes;
mod powder_watch;
mod projectiles;
mod pvp;
mod shapes;
mod shield_guard;
mod signal_call;
mod thrown_weapons;
mod timed_swing;
mod tinker_setup;
mod trap_shove;
mod trick_shots;
mod weapon_families;
mod weapon_family_group;
mod weapon_mastery;
