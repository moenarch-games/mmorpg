//! Greyhaven Vale unit content (revision 6): creature templates, spawns,
//! NPCs and graveyard follow the design contract and stay clear of geometry,
//! of each other and of the hub spawn plaza.
use std::collections::BTreeMap;

use mmorpg_core::greyhaven_vale::{self, SPAWN_GRID, area_at, units};
use mmorpg_core::{
    AreaId, CreatureBehaviour, CreatureFamily, INTEREST_RADIUS_UNITS, MAX_PLAYERS_PER_ZONE,
    NpcRole, ZoneCommand, ZoneId, ZoneSimulation,
};

#[test]
fn the_content_identity_is_pinned() {
    let content = greyhaven_vale::content();
    assert_eq!(content.revision(), 14);
    assert_eq!(content.rng_seed(), 0x3cbc_808b_be89_b29c);
    assert_eq!(
        content.definition(),
        &greyhaven_vale::greyhaven_vale_definition()
    );
    assert_eq!(content.areas(), greyhaven_vale::areas());
    // The recorded fingerprint pins every content table, including the bound ability, item,
    // loot, vendor and quest catalogs, the inn door and the NPC routines. Changing creatures,
    // NPCs, colliders or areas requires a new revision; `MMORPG_UPDATE_FIXTURES=1` re-records it.
    let actual = format!("{:#018x}\n", content.fingerprint());
    let recorded = mmorpg_core::golden::golden(
        mmorpg_core::golden::workspace_root()
            .join("fixtures/content/greyhaven-vale-fingerprint.txt"),
        &actual,
    );
    assert_eq!(actual, recorded, "recorded fingerprint");
}

#[test]
fn templates_follow_the_design_contract() {
    let content = greyhaven_vale::content();
    let rows: Vec<_> = content
        .creature_templates()
        .iter()
        .map(|template| {
            (
                template.name.as_str(),
                template.family,
                template.behaviour,
                [template.min_level, template.max_level],
                template.elite,
                template.max_health(template.min_level),
                template.health_per_level,
                template.damage_at(template.min_level),
                template.swing_ticks,
                template.respawn_ticks,
            )
        })
        .collect();
    use CreatureBehaviour::{Aggressive, Neutral};
    use CreatureFamily::*;
    assert_eq!(
        rows,
        [
            (
                "Timber Wolf",
                Wolf,
                Aggressive,
                [1, 2],
                false,
                42,
                14,
                [2, 4],
                60,
                1_800
            ),
            (
                "Young Boar",
                Boar,
                Neutral,
                [1, 2],
                false,
                48,
                14,
                [2, 4],
                66,
                1_800
            ),
            (
                "Grain Rat",
                Vermin,
                Neutral,
                [1, 1],
                false,
                30,
                0,
                [1, 3],
                48,
                1_800
            ),
            (
                "Field Marauder",
                Marauder,
                Aggressive,
                [2, 3],
                false,
                60,
                16,
                [3, 6],
                66,
                1_800
            ),
            (
                "Mirefin Lurker",
                Mirefin,
                Aggressive,
                [3, 3],
                false,
                70,
                0,
                [3, 6],
                60,
                1_800
            ),
            (
                "Redbrand Bandit",
                Redbrand,
                Aggressive,
                [3, 5],
                false,
                75,
                16,
                [4, 7],
                66,
                1_800
            ),
            (
                "Garrick Redbrand",
                Redbrand,
                Aggressive,
                [5, 5],
                true,
                420,
                0,
                [8, 14],
                72,
                9_000
            ),
            (
                "Redbrand Crossbowman",
                Redbrand,
                Aggressive,
                [3, 5],
                false,
                70,
                14,
                [3, 6],
                66,
                1_800
            ),
        ]
    );
    let wolf = content
        .creature_template(units::templates::TIMBER_WOLF)
        .unwrap();
    assert_eq!(wolf.damage_at(2), [3, 5], "wolves hit 1 harder per level");
}

#[test]
fn spawns_populate_their_subzones() {
    let content = greyhaven_vale::content();
    let mut counts: BTreeMap<(AreaId, &str), usize> = BTreeMap::new();
    for spawn in content.creature_spawns() {
        let template = content.creature_template(spawn.template).unwrap();
        let area = area_at(spawn.position[0], spawn.position[1])
            .unwrap_or_else(|| panic!("spawn {} lies outside every area", spawn.id.get()));
        // The whole wander area stays in the same subzone.
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let x = spawn.position[0] + dx * spawn.wander_radius;
            let z = spawn.position[1] + dz * spawn.wander_radius;
            assert_eq!(
                area_at(x, z).map(|area| area.id()),
                Some(area.id()),
                "spawn {} wanders out of {}",
                spawn.id.get(),
                area.name()
            );
        }
        *counts
            .entry((area.id(), template.name.as_str()))
            .or_default() += 1;
    }
    let expected = BTreeMap::from([
        ((greyhaven_vale::WOLFRUN_WOODS, "Timber Wolf"), 14),
        ((greyhaven_vale::WOLFRUN_WOODS, "Young Boar"), 6),
        ((greyhaven_vale::MILLBROOK_FARM, "Grain Rat"), 6),
        ((greyhaven_vale::MILLBROOK_FARM, "Field Marauder"), 10),
        ((greyhaven_vale::STILLWATER_LAKE, "Mirefin Lurker"), 8),
        ((greyhaven_vale::REDBRAND_HOLLOW, "Redbrand Bandit"), 14),
        ((greyhaven_vale::REDBRAND_HOLLOW, "Garrick Redbrand"), 1),
        ((greyhaven_vale::GREYFANG_RIDGE, "Redbrand Crossbowman"), 3),
        ((greyhaven_vale::SOUTHMEADOW_DOWNS, "Young Boar"), 2),
        ((greyhaven_vale::SOUTHMEADOW_DOWNS, "Grain Rat"), 1),
        ((greyhaven_vale::HOLLOWPINE_REACH, "Timber Wolf"), 4),
        ((greyhaven_vale::HOLLOWPINE_REACH, "Young Boar"), 2),
        ((greyhaven_vale::EASTWATER_FEN, "Mirefin Lurker"), 3),
    ]);
    assert_eq!(counts, expected);
    assert_eq!(content.creature_spawns().len(), 74);
}

/// No creature wanders within the interest radius (plus a margin) of any
/// spawn slot: the hub plaza shows only players and NPCs, and no creature
/// can aggro a player standing there.
#[test]
fn creatures_stay_out_of_sight_of_the_spawn_plaza() {
    const MARGIN: i64 = 400;
    let content = greyhaven_vale::content();
    for spawn in content.creature_spawns() {
        let keep_away = i64::from(INTEREST_RADIUS_UNITS) + i64::from(spawn.wander_radius) + MARGIN;
        for slot in 0..MAX_PLAYERS_PER_ZONE {
            let feet = SPAWN_GRID.feet(u16::try_from(slot).unwrap()).unwrap();
            let dx = i64::from(spawn.position[0] - feet[0]);
            let dz = i64::from(spawn.position[1] - feet[2]);
            assert!(
                dx * dx + dz * dz > keep_away * keep_away,
                "spawn {} is within {keep_away} units of slot {slot}",
                spawn.id.get()
            );
        }
    }
}

#[test]
fn npcs_stand_at_their_posts() {
    let content = greyhaven_vale::content();
    let posts: Vec<_> = content
        .npcs()
        .iter()
        .map(|npc| {
            (
                npc.name.as_str(),
                npc.role,
                area_at(npc.position[0], npc.position[1]).map(|area| area.id()),
            )
        })
        .collect();
    let outpost = Some(greyhaven_vale::OUTPOST);
    assert_eq!(
        posts,
        [
            ("Marshal Elden Greywatch", NpcRole::QuestGiver, outpost),
            ("Tanner Hilda Brook", NpcRole::QuestGiver, outpost),
            ("Innkeeper Bram Tolliver", NpcRole::Vendor, outpost),
            (
                "Farmer Osric Mill",
                NpcRole::QuestGiver,
                Some(greyhaven_vale::MILLBROOK_FARM)
            ),
            ("Brother Aldous", NpcRole::SpiritHealer, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
        ]
    );
    let graveyard = content.graveyard();
    assert_eq!(graveyard, units::GRAVEYARD);
    assert_eq!(
        area_at(graveyard[0], graveyard[1]).map(|area| area.id()),
        outpost,
        "the graveyard is in the hub"
    );
}

#[test]
fn the_hosted_zone_spawns_every_creature_and_rests_deterministically() {
    let content = greyhaven_vale::content();
    let mut first = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    let mut second = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    let start = first.snapshot().unwrap();
    assert_eq!(start.creatures.len(), 74);
    for (creature, spawn) in start.creatures.iter().zip(content.creature_spawns()) {
        let template = content.creature_template(spawn.template).unwrap();
        assert_eq!(creature.creature_id, spawn.id);
        assert!((template.min_level..=template.max_level).contains(&creature.level));
        assert_eq!(creature.health, template.max_health(creature.level));
        // Spawned on the ground, out on the hills too (#159): the physics heightfield triangles stay
        // within 0.15 m of the analytic hills.
        let [x, y, z] = mmorpg_core::physics::to_units(creature.position);
        assert_eq!([x, z], spawn.position);
        let ground = greyhaven_vale::hill_height(x, z);
        assert!(
            (y - ground - template.half_extents[1]).abs() <= 15,
            "{:?} at {y} on {ground}",
            spawn.id
        );
    }
    // A player on the plaza sees the hub NPCs but no creature.
    first.add_player(1).unwrap();
    second.add_player(1).unwrap();
    first
        .apply_command(
            1,
            1,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: 0,
            },
        )
        .unwrap();
    second
        .apply_command(
            1,
            1,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: 0,
            },
        )
        .unwrap();
    for _ in 0..600 {
        first.advance_tick().unwrap();
        second.advance_tick().unwrap();
    }
    assert_eq!(first.snapshot().unwrap(), second.snapshot().unwrap());
    let view = first.snapshot_for_player(1).unwrap();
    assert!(
        view.entities
            .iter()
            .all(|entity| entity.kind != mmorpg_core::EntityKind::Creature)
    );
    assert!(
        view.entities
            .iter()
            .any(|entity| entity.kind == mmorpg_core::EntityKind::Npc)
    );
    assert_eq!(view.viewer.health, 50);
    // Idle creatures wander within their radius.
    let moved = first
        .snapshot()
        .unwrap()
        .creatures
        .iter()
        .zip(content.creature_spawns())
        .filter(|(creature, spawn)| {
            let [x, _, z] = mmorpg_core::physics::to_units(creature.position);
            [x, z] != spawn.position
        })
        .count();
    assert!(moved > 20, "only {moved} creatures wandered");
    for (creature, spawn) in first
        .snapshot()
        .unwrap()
        .creatures
        .iter()
        .zip(content.creature_spawns())
    {
        let dx =
            i64::from(mmorpg_core::physics::to_units(creature.position)[0] - spawn.position[0]);
        let dz =
            i64::from(mmorpg_core::physics::to_units(creature.position)[2] - spawn.position[1]);
        let limit = i64::from(spawn.wander_radius) + 50;
        assert!(
            dx * dx + dz * dz <= limit * limit,
            "creature {} strayed",
            spawn.id.get()
        );
    }
}
