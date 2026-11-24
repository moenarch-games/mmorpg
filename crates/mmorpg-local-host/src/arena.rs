//! The Battle Royale practice arena's gameplay content (#550, #552, #680): the shared
//! symmetric fortress's own bodies (`battle_royale_core::arena`, the one definition of its
//! collision) with a courtyard spawn lattice and stationary training targets, simulated by the
//! shared `mmorpg-core` rules in [`crate::host::LocalZoneHost::practice_arena`]. The browser
//! draws it in `mmorpg-wasm`'s arena presentation; nothing here is presentation.

use std::sync::{Arc, OnceLock};

use battle_royale_core::arena;
use mmorpg_core::{
    CharacterPersistence, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn,
    CreatureTemplate, CreatureTemplateId, HitRewind, MAX_PLAYER_LEVEL, PlayerCombatPolicy,
    PropKind, SpawnGrid, TimedCombat, ZoneAreas, ZoneContent, ZoneDefinition,
};

/// The practice arena's training target (#550): a breakable prop at the maximum player level,
/// so it never moves or fights and keeps no threat, but takes real damage and breaks.
const TRAINING_TARGET: CreatureTemplateId = CreatureTemplateId::new(1);
/// Health of a training target: about twenty max-level weapon hits, a few ability rotations.
const TRAINING_TARGET_HEALTH: u32 = 300;
/// Practice spawn slot 0: just inside the south gate, facing +Z across the courtyard to the keep.
pub const PRACTICE_SPAWN: [i32; 2] = [0, -7_600];
/// Feet XZ of the training targets relative to [`PRACTICE_SPAWN`]: three 8–9 m ahead for melee,
/// two 21 m ahead for ranged abilities, all between spawn slots.
pub const TRAINING_TARGETS: [[i32; 2]; 5] = [
    [-300, 800],
    [0, 900],
    [300, 800],
    [-600, 2_100],
    [600, 2_100],
];
/// Practice slots: 16 columns along +X and 32 rows toward the keep, 2 m apart, inside the south
/// courtyard and clear of its ramps, lifts and walls.
const PRACTICE_SPAWN_GRID: SpawnGrid = SpawnGrid {
    origin: PRACTICE_SPAWN,
    columns: 16,
    spacing: 200,
};

/// The shared fortress content a hosted match runs.
pub fn fortress() -> Arc<ZoneContent> {
    arena::content().expect("the pinned battle-royale fortress content is valid")
}

/// The practice arena: the fortress's own bodies, a courtyard spawn lattice and training targets.
pub fn content() -> Arc<ZoneContent> {
    static CONTENT: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    Arc::clone(CONTENT.get_or_init(|| {
        let fortress = fortress();
        let definition = ZoneDefinition::with_spawn_grid(
            fortress.definition().gravity(),
            PRACTICE_SPAWN_GRID,
            fortress.definition().colliders().to_vec(),
        )
        .expect("practice spawn lattice is clear of the fortress");
        let target = CreatureTemplate {
            id: TRAINING_TARGET,
            name: "Training Target".to_owned(),
            family: CreatureFamily::Prop,
            behaviour: CreatureBehaviour::Prop(PropKind::Barrel),
            min_level: MAX_PLAYER_LEVEL,
            max_level: MAX_PLAYER_LEVEL,
            elite: false,
            health: TRAINING_TARGET_HEALTH,
            health_per_level: 0,
            damage: [1, 1],
            damage_per_level: 0,
            swing_ticks: mmorpg_core::ticks_from_ms(2_000),
            respawn_ticks: mmorpg_core::unit::CORPSE_TICKS,
            half_extents: [30, 45, 30],
        };
        let spawns = (1..)
            .zip(TRAINING_TARGETS)
            .map(|(id, [x, z])| CreatureSpawn {
                id: CreatureId::new(id),
                template: TRAINING_TARGET,
                position: [PRACTICE_SPAWN[0] + x, PRACTICE_SPAWN[1] + z],
                facing: 32_768,
                wander_radius: 0,
                levels: None,
            })
            .collect();
        let content = ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![target],
            spawns,
            Vec::new(),
            PRACTICE_SPAWN,
        )
        .expect("practice arena content is valid");
        Arc::new(
            content
                .with_character_persistence(CharacterPersistence::Ephemeral)
                .with_creature_abilities(Vec::new())
                .and_then(|content| content.with_platforms(fortress.platforms().to_vec()))
                .and_then(|content| content.with_ramps(fortress.ramps().to_vec()))
                // Practice swings are timed and can clash, arena first (#679).
                .and_then(|content| content.with_timed_combat(TimedCombat::ARENA))
                .expect("the fortress's lifts and ramps bind to the practice arena"),
        )
    }))
}

/// The practice arena of a netcode trial (#680): the practice arena where players may fight
/// (a sparring partner joins), with the arena trial's capped hit rewind when `rewind` (the
/// match's option, the same for everyone in it).
pub fn trial_content(rewind: bool) -> Arc<ZoneContent> {
    static PLAIN: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    static REWOUND: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    let cell = if rewind { &REWOUND } else { &PLAIN };
    Arc::clone(cell.get_or_init(|| {
        let content = (*content())
            .clone()
            .with_player_combat(PlayerCombatPolicy::FreeForAll);
        Arc::new(if rewind {
            content
                .with_hit_rewind(HitRewind::ARENA_TRIAL)
                .expect("the practice arena has timed swings")
        } else {
            content
        })
    }))
}
