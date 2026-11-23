//! mmorpg-core integration tests: physics, movement, collision, terrain, water and props.
//!
//! One test binary per theme instead of one per file (#1087): each former
//! `tests/<name>.rs` is the module `<name>`, so its tests are `<name>::<test>`.

#[path = "../support/mod.rs"]
mod support;

mod authored_collision;
mod boulders;
mod breakables;
mod cave;
mod climbing;
mod crossings;
mod doors;
mod environmental_damage;
mod hills;
mod large_creatures;
mod line_of_sight;
mod movable_props;
mod movement;
mod physical_world;
mod physics_backend;
mod physique;
mod platforms;
mod ramps;
mod roll;
mod structure_pathing;
mod structures;
mod summoned_terrain;
mod traps;
mod vale_basin;
mod village_brook;
mod water;
mod water_current;
