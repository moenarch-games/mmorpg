//! Greyhaven Vale unit content: creature templates, spawns,
//! NPCs and graveyard follow the design contract and stay clear of geometry,
//! of each other and of the hub spawn plaza.
use std::collections::BTreeMap;

use mmorpg_core::greyhaven_vale::{self, SPAWN_GRID, area_at, units, wilds};
use mmorpg_core::physics::{BodyId, RigidBody, Vec3i, World};
use mmorpg_core::{
    AreaId, CreatureBehaviour, CreatureFamily, INTEREST_RADIUS_UNITS, MAX_PLAYERS_PER_ZONE,
    NpcRole, PropKind, ZoneCommand, ZoneId, ZoneSimulation,
};

#[test]
fn the_content_identity_is_pinned() {
    let content = greyhaven_vale::content();
    assert_eq!(content.rng_seed(), 0x3cbc_808b_be89_b29c);
    assert_eq!(
        content.definition(),
        &greyhaven_vale::greyhaven_vale_definition()
    );
    assert_eq!(content.areas(), greyhaven_vale::areas());
    // The revision hashes every content table, including the bound ability, item, loot,
    // vendor and quest catalogs, the inn door and the NPC routines, so it is deterministic
    // and changes by itself with the content (docs/adr/0006-merge-safe-format-identities.md).
    let rebuilt =
        greyhaven_vale::content_with_definition(greyhaven_vale::greyhaven_vale_definition())
            .unwrap();
    assert_eq!(rebuilt.revision(), content.revision());
    let mut renamed = content.npcs().to_vec();
    renamed[0].name.push('!');
    let changed = mmorpg_core::ZoneContent::new(
        content.definition().clone(),
        content.areas().clone(),
        content.creature_templates().to_vec(),
        content.creature_spawns().to_vec(),
        renamed,
        content.graveyard(),
    )
    .unwrap();
    assert_ne!(changed.revision(), content.revision());
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
                // Swing interval and respawn delay in ms.
                u32::from(template.swing_ticks) * 1_000 / u32::from(mmorpg_core::TICK_HZ),
                template.respawn_ticks * 1_000 / u32::from(mmorpg_core::TICK_HZ),
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
                2000,
                60000
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
                2200,
                60000
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
                1600,
                60000
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
                2200,
                60000
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
                2000,
                60000
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
                2200,
                60000
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
                2400,
                120000
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
                2200,
                60000
            ),
            // Breakable props (#288): a little health, 5 min respawn.
            (
                "Crate",
                Prop,
                CreatureBehaviour::Prop(PropKind::Crate),
                [1, 1],
                false,
                12,
                0,
                [1, 1],
                2000,
                300000
            ),
            (
                "Barrel",
                Prop,
                CreatureBehaviour::Prop(PropKind::Barrel),
                [1, 1],
                false,
                15,
                0,
                [1, 1],
                2000,
                300000
            ),
            (
                "Blasting Powder Barrel",
                Prop,
                CreatureBehaviour::Prop(PropKind::ExplosiveBarrel),
                [1, 1],
                false,
                10,
                0,
                [1, 1],
                2000,
                300000
            ),
            (
                "Fence",
                Prop,
                CreatureBehaviour::Prop(PropKind::Fence),
                [1, 1],
                false,
                18,
                0,
                [1, 1],
                2000,
                300000
            ),
            // A large creature (#297).
            (
                "Hill Ogre",
                Ogre,
                Aggressive,
                [5, 5],
                false,
                300,
                0,
                [9, 15],
                2800,
                60000
            ),
            // A movable prop (#58).
            (
                "Heavy Crate",
                Prop,
                CreatureBehaviour::Prop(PropKind::HeavyCrate),
                [1, 1],
                false,
                30,
                0,
                [1, 1],
                2000,
                300000
            ),
            // The Greyfang cavern's shield-bearer (#160).
            (
                "Redbrand Brute",
                Redbrand,
                Aggressive,
                [5, 5],
                false,
                215,
                0,
                [7, 12],
                2800,
                60000
            ),
            // The boulder trap (#299): unbreakable.
            (
                "Loose Boulder",
                Prop,
                CreatureBehaviour::Prop(PropKind::Boulder),
                [1, 1],
                false,
                1,
                0,
                [1, 1],
                2000,
                300000
            ),
            // The beasts of #889: the bear tough and slow, the spider fast and fragile.
            (
                "Brown Bear",
                Bear,
                Aggressive,
                [3, 4],
                false,
                130,
                20,
                [5, 9],
                3000,
                60000
            ),
            (
                "Cave Spider",
                Spider,
                Aggressive,
                [3, 4],
                false,
                40,
                10,
                [2, 4],
                1300,
                60000
            ),
            (
                "Marsh Lurker",
                Lurker,
                Aggressive,
                [2, 3],
                false,
                60,
                14,
                [3, 6],
                2400,
                60000
            ),
            // Old Scarback (#913), the hunter's cache's elite bear, returns like Garrick.
            (
                "Old Scarback",
                Bear,
                Aggressive,
                [5, 5],
                true,
                360,
                0,
                [8, 13],
                3000,
                120000
            ),
            // The Southwood lynx (#989): a hard, slow pounce.
            (
                "Forest Lynx",
                Lynx,
                Aggressive,
                [2, 3],
                false,
                50,
                12,
                [4, 7],
                2600,
                60000
            ),
            // The Eastwater Fen adder (#990): a frail body, a quick, warned strike.
            (
                "Marsh Adder",
                Adder,
                Aggressive,
                [2, 3],
                false,
                34,
                10,
                [4, 7],
                2600,
                60000
            ),
            // The Greyfang cave bat (#995): a frail flier with a warned dive.
            (
                "Cave Bat",
                Bat,
                Aggressive,
                [3, 4],
                false,
                36,
                10,
                [3, 6],
                2600,
                60000
            ),
            // The Fenwater river crayfish (#996): a shelled bank ambusher with a slow pinch.
            (
                "River Crayfish",
                Crayfish,
                Aggressive,
                [3, 4],
                false,
                70,
                14,
                [5, 9],
                3000,
                60000
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
        // Night raiders (#530) stand on the farm's outskirts only for a committed incident and
        // march rather than wander; `night_raid` covers them, and `hunt_outing` the hunt's bear
        // (#991), and `merchant_cart` the cart's robbers (#992).
        if content.is_raider(spawn.id)
            || content.is_hunt_beast(spawn.id)
            || content.is_cart_bandit(spawn.id)
        {
            continue;
        }
        let template = content.creature_template(spawn.template).unwrap();
        let area = area_at(spawn.position[0], spawn.position[1])
            .unwrap_or_else(|| panic!("spawn {} lies outside every area", spawn.id.get()));
        // Named rectangles are labels, while the lake and zone boundary own physical
        // clearance. The relocated shore cuts through the old woodland's label bounds.
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let x = spawn.position[0] + dx * spawn.wander_radius;
            let z = spawn.position[1] + dz * spawn.wander_radius;
            assert!(
                greyhaven_vale::inside_walls(x, z),
                "spawn {} wanders outside the zone",
                spawn.id.get()
            );
            if area.id() == greyhaven_vale::vale::WOLFRUN_WOODS {
                assert!(
                    !greyhaven_vale::lake::near_shore(x, z, 0),
                    "spawn {} wanders into Stillwater",
                    spawn.id.get()
                );
            }
        }
        *counts
            .entry((area.id(), template.name.as_str()))
            .or_default() += 1;
    }
    let expected = BTreeMap::from([
        ((greyhaven_vale::vale::WOLFRUN_WOODS, "Timber Wolf"), 14),
        ((greyhaven_vale::vale::WOLFRUN_WOODS, "Young Boar"), 6),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Grain Rat"), 6),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Field Marauder"), 10),
        ((greyhaven_vale::vale::STILLWATER_LAKE, "Mirefin Lurker"), 8),
        (
            (greyhaven_vale::vale::REDBRAND_HOLLOW, "Redbrand Bandit"),
            14,
        ),
        (
            (greyhaven_vale::vale::REDBRAND_HOLLOW, "Garrick Redbrand"),
            1,
        ),
        // Greyfang Ridge: the outriders, the Greyfang cavern's garrison (#160), the
        // Greyfang Approach patrol (#911) and the lookout's guards (#913).
        (
            (greyhaven_vale::vale::GREYFANG_RIDGE, "Redbrand Crossbowman"),
            7,
        ),
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Redbrand Bandit"), 4),
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Redbrand Brute"), 3),
        ((greyhaven_vale::vale::SOUTHMEADOW_DOWNS, "Young Boar"), 2),
        ((greyhaven_vale::vale::SOUTHMEADOW_DOWNS, "Hill Ogre"), 1),
        ((greyhaven_vale::vale::SOUTHMEADOW_DOWNS, "Grain Rat"), 1),
        ((greyhaven_vale::vale::HOLLOWPINE_REACH, "Timber Wolf"), 4),
        ((greyhaven_vale::vale::HOLLOWPINE_REACH, "Young Boar"), 2),
        ((greyhaven_vale::vale::EASTWATER_FEN, "Mirefin Lurker"), 3),
        // The Eastwater Fen wolf pack, a Ranger firing-line fight (#932).
        ((greyhaven_vale::vale::EASTWATER_FEN, "Timber Wolf"), 3),
        // The optional discoveries' guards (#913): the hunter's cache's elite bear and the fen
        // cache's ambushers.
        ((greyhaven_vale::vale::WOLFRUN_WOODS, "Old Scarback"), 1),
        ((greyhaven_vale::vale::EASTWATER_FEN, "Marsh Lurker"), 2),
        // The farmland hamlets' rats, boars and wolves (#859).
        (
            (greyhaven_vale::farmland::FARMLAND_AND_HAMLETS, "Grain Rat"),
            4,
        ),
        (
            (greyhaven_vale::farmland::FARMLAND_AND_HAMLETS, "Young Boar"),
            4,
        ),
        (
            (
                greyhaven_vale::farmland::FARMLAND_AND_HAMLETS,
                "Timber Wolf",
            ),
            4,
        ),
        // Breakable props (#288): around Greyhaven and the farm, powder barrels in the camp.
        ((greyhaven_vale::vale::OUTPOST, "Crate"), 3),
        ((greyhaven_vale::vale::OUTPOST, "Barrel"), 2),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Crate"), 2),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Barrel"), 2),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Fence"), 3),
        ((greyhaven_vale::vale::MILLBROOK_FARM, "Heavy Crate"), 2),
        // Redbrand Cave supplies inside Greyfang Ridge (#719).
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Crate"), 2),
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Barrel"), 1),
        // Cave Spiders (#889) in the cave's north mouth.
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Cave Spider"), 3),
        // Cave Bats (#995) on the outer cavern's stalactite.
        ((greyhaven_vale::vale::GREYFANG_RIDGE, "Cave Bat"), 2),
        (
            (
                greyhaven_vale::vale::REDBRAND_HOLLOW,
                "Blasting Powder Barrel",
            ),
            5,
        ),
        ((greyhaven_vale::vale::REDBRAND_HOLLOW, "Loose Boulder"), 1),
        // The Marsh-Lake Wetlands' creatures (#506).
        (
            (greyhaven_vale::marsh::MARSH_LAKE_WETLANDS, "Mirefin Lurker"),
            15,
        ),
        (
            (greyhaven_vale::marsh::MARSH_LAKE_WETLANDS, "Young Boar"),
            8,
        ),
        (
            (greyhaven_vale::marsh::MARSH_LAKE_WETLANDS, "Timber Wolf"),
            8,
        ),
        // Marsh Lurkers (#889) beside the trails.
        (
            (greyhaven_vale::marsh::MARSH_LAKE_WETLANDS, "Marsh Lurker"),
            3,
        ),
        // The Bandit Highlands garrison (#502).
        (
            (
                greyhaven_vale::highlands::BANDIT_HIGHLANDS,
                "Redbrand Bandit",
            ),
            14,
        ),
        (
            (
                greyhaven_vale::highlands::BANDIT_HIGHLANDS,
                "Redbrand Crossbowman",
            ),
            8,
        ),
        (
            (
                greyhaven_vale::highlands::BANDIT_HIGHLANDS,
                "Redbrand Brute",
            ),
            3,
        ),
        // The new sub-regions (#816).
        ((wilds::WESTMERE_WOODS, "Timber Wolf"), 8),
        ((wilds::WESTMERE_WOODS, "Young Boar"), 4),
        ((wilds::WESTMERE_WOODS, "Mirefin Lurker"), 3),
        ((wilds::SOUTHWOOD, "Timber Wolf"), 4),
        ((wilds::SOUTHWOOD, "Young Boar"), 6),
        // Brown Bears (#889) in the deep woods.
        ((wilds::WESTMERE_WOODS, "Brown Bear"), 2),
        ((wilds::SOUTHWOOD, "Brown Bear"), 1),
        // The Forest Lynxes (#989) on the wood's edge.
        ((wilds::SOUTHWOOD, "Forest Lynx"), 3),
        // The Marsh Adders (#990) in Adder Hollow.
        ((greyhaven_vale::vale::EASTWATER_FEN, "Marsh Adder"), 3),
        ((wilds::CROWN_FOOTHILLS, "Timber Wolf"), 6),
        ((wilds::CROWN_FOOTHILLS, "Hill Ogre"), 1),
        // The Rimewood scouts (#933), an Arcanist's group-control fight,
        // and the Rimewood signal post (#952), a target-priority fight.
        ((wilds::CROWN_FOOTHILLS, "Redbrand Bandit"), 5),
        ((wilds::CROWN_FOOTHILLS, "Redbrand Crossbowman"), 1),
        ((wilds::GREYFANG_APPROACH, "Redbrand Bandit"), 4),
        ((wilds::GREYFANG_APPROACH, "Redbrand Crossbowman"), 3),
        // The Redbrands hold the Delving Workings (#818).
        (
            (
                greyhaven_vale::settlements::DELVING_WORKINGS,
                "Redbrand Bandit",
            ),
            3,
        ),
        (
            (
                greyhaven_vale::settlements::DELVING_WORKINGS,
                "Redbrand Crossbowman",
            ),
            2,
        ),
        ((wilds::DELVING_HEIGHTS, "Hill Ogre"), 2),
        ((wilds::FENWATER_CROSSING, "Field Marauder"), 4),
        // River Crayfish (#996) on the Fenwater's bank.
        ((wilds::FENWATER_CROSSING, "River Crayfish"), 3),
        ((wilds::KINGS_ROAD_DOWNS, "Field Marauder"), 4),
        ((wilds::KINGS_ROAD_DOWNS, "Young Boar"), 6),
        ((wilds::KINGS_ROAD_DOWNS, "Grain Rat"), 3),
    ]);
    assert_eq!(counts, expected);
}

/// No creature wanders within the interest radius (plus a margin) of any
/// spawn slot: the hub plaza shows only players and NPCs, and no creature
/// can aggro a player standing there.
#[test]
fn creatures_stay_out_of_sight_of_the_spawn_plaza() {
    const MARGIN: i64 = 400;
    let content = greyhaven_vale::content();
    for spawn in content.creature_spawns() {
        // Breakable props (#288) stand around the hub; they never fight.
        if content
            .creature_template(spawn.template)
            .is_some_and(|template| template.prop().is_some())
            // Night raiders (#530) stand only during an incident, on the farm's outskirts, and
            // the cart's robbers (#992) only during an ambush, in the Millbrook Climb's cutting.
            || content.is_raider(spawn.id)
            || content.is_cart_bandit(spawn.id)
        {
            continue;
        }
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
    let outpost = Some(greyhaven_vale::vale::OUTPOST);
    assert_eq!(
        posts,
        [
            ("Marshal Elden Greywatch", NpcRole::QuestGiver, outpost),
            ("Tanner Hilda Brook", NpcRole::QuestGiver, outpost),
            ("Innkeeper Bram Tolliver", NpcRole::Vendor, outpost),
            (
                "Farmer Osric Mill",
                NpcRole::QuestGiver,
                Some(greyhaven_vale::vale::MILLBROOK_FARM)
            ),
            ("Brother Aldous", NpcRole::SpiritHealer, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Greyhaven Guard", NpcRole::Guard, outpost),
            ("Weaver Lysa Reed", NpcRole::Resident, outpost),
            ("Carpenter Owen Alder", NpcRole::Resident, outpost),
            ("Mara Wells", NpcRole::Resident, outpost),
            ("Davin Moss", NpcRole::Resident, outpost),
            ("Baker Anwen", NpcRole::Resident, outpost),
            ("Miller Tomas", NpcRole::Resident, outpost),
            ("Herbalist Nessa", NpcRole::Resident, outpost),
            ("Peddler Rowan", NpcRole::Resident, outpost),
            ("Fisher Ada", NpcRole::Resident, outpost),
            ("Porter Ben", NpcRole::Resident, outpost),
            ("Washer Elin", NpcRole::Resident, outpost),
            ("Apprentice Finn", NpcRole::Resident, outpost),
        ]
        .into_iter()
        .chain(region_residents())
        // The smith (#917), the newest NPC.
        .chain([("Smith Wulfric Hale", NpcRole::Vendor, outpost)])
        .collect::<Vec<_>>()
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
    assert_eq!(start.creatures.len(), content.creature_spawns().len());
    let mut ground = World::new(Vec3i::ZERO);
    ground.add_body(RigidBody::floor(BodyId(1), 0)).unwrap();
    ground.set_terrain(BodyId(2), content.definition().terrain().unwrap().clone());
    for (creature, spawn) in start.creatures.iter().zip(content.creature_spawns()) {
        let template = content.creature_template(spawn.template).unwrap();
        assert_eq!(creature.creature_id, spawn.id);
        // The 18 night raiders (#530) wait, without a body, for an incident, and the hunt's bear
        // (#991) for a confrontation day, and the cart's robbers (#992) for an ambush.
        if content.is_raider(spawn.id)
            || content.is_hunt_beast(spawn.id)
            || content.is_cart_bandit(spawn.id)
        {
            assert!(matches!(
                creature.life,
                mmorpg_core::CreatureLife::Despawned { .. }
            ));
            continue;
        }
        assert!((template.min_level..=template.max_level).contains(&creature.level));
        assert_eq!(creature.health, template.max_health(creature.level));
        let [x, y, z] = mmorpg_core::physics::to_units(creature.position);
        assert_eq!([x, z], spawn.position);
        // Check the actual triangulated support under the capsule, including the hillside
        // beneath its rounded base. Bilinear centre samples no longer bound the authored cliffs.
        let origin = [x as f32, y as f32 + 1_000., z as f32];
        let [hx, hy, hz] = template.half_extents;
        let hit = ground
            .cast_shape(origin, Vec3i::new(hx, hy, hz), [0., -2_000., 0.], |id| {
                id == BodyId(2)
                    || (id == BodyId(1)
                        && content
                            .definition()
                            .terrain()
                            .unwrap()
                            .low_within(x, z, hx.max(hz) + 1)
                            >= 0)
            })
            .expect("terrain supports each spawn");
        let supported_y = origin[1] - 2_000. * hit.fraction;
        // A flier (#995) roosts its rise over that support.
        let supported_y = if template.family.flies() {
            supported_y - hy as f32 + mmorpg_core::flight::ROOST_RISE_UNITS as f32
        } else {
            supported_y
        };
        assert!(
            (y as f32 - supported_y).abs() <= 15.,
            "{:?} at {y}, capsule support at {supported_y}",
            spawn.id
        );
    }
    // A player on the plaza sees the hub NPCs and props but no creature that fights.
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
    for _ in 0..mmorpg_core::golden::t30(600) {
        first.advance_tick().unwrap();
        second.advance_tick().unwrap();
    }
    assert_eq!(first.snapshot().unwrap(), second.snapshot().unwrap());
    let view = first.snapshot_for_player(1).unwrap();
    assert!(view.entities.iter().all(|entity| {
        entity.kind != mmorpg_core::EntityKind::Creature
            || content
                .creature_template(mmorpg_core::CreatureTemplateId::new(entity.appearance))
                .is_some_and(|template| template.prop().is_some())
    }));
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
        // Raiders, the hunt's bear (#991), the cart's robbers (#992) and den packs (#912) keep
        // their own ranges.
        if content.is_raider(spawn.id)
            || content.is_hunt_beast(spawn.id)
            || content.is_cart_bandit(spawn.id)
            || content
                .dens()
                .iter()
                .any(|den| den.pack.contains(&spawn.id))
        {
            continue;
        }
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

/// The region's people (#819): the settlements' residents and watchmen (#818) and the
/// farmland's hamlets' (#424), each in the area it works in.
fn region_residents() -> Vec<(&'static str, NpcRole, Option<mmorpg_core::AreaId>)> {
    use greyhaven_vale::{farmland::FARMLAND_AND_HAMLETS, settlements, wilds};
    let resident = |name, area| (name, NpcRole::Resident, Some(area));
    let guard = |name, area| (name, NpcRole::Guard, Some(area));
    // The region's quest givers (#820) and the hamlets' (#859).
    let giver = |name, area| (name, NpcRole::QuestGiver, Some(area));
    vec![
        giver("Alewife Gwen Barrow", settlements::BARROWDOWN),
        resident("Shepherd Corin", settlements::BARROWDOWN),
        resident("Old Maud", wilds::KINGS_ROAD_DOWNS),
        resident("Farmer Tobin Hale", settlements::BARROWDOWN),
        resident("Pip", settlements::BARROWDOWN),
        giver("Innkeeper Hob Ferris", settlements::FENFORD),
        resident("Miller Wynne", settlements::FENFORD),
        resident("Ferryman Osk", wilds::FENWATER_CROSSING),
        resident("Netter Ilsa", settlements::FENFORD),
        giver("Fisher Brann", settlements::WESTMERE_LODGE),
        giver("Huntsman Edric Vale", settlements::HUNTERS_CAMP),
        resident("Trapper Sela", settlements::HUNTERS_CAMP),
        giver("Watchman Garth", settlements::GREYFANG_WATCH),
        guard("Watchman Pell", settlements::GREYFANG_WATCH),
        giver("Goodwife Ena", FARMLAND_AND_HAMLETS),
        resident("Ploughman Garr", FARMLAND_AND_HAMLETS),
        resident("Old Bertil", FARMLAND_AND_HAMLETS),
        giver("Shepherd Lise", FARMLAND_AND_HAMLETS),
        giver("Miller Hanne", FARMLAND_AND_HAMLETS),
        resident("Farmhand Jory", FARMLAND_AND_HAMLETS),
        ("Trader Maren", NpcRole::Vendor, Some(settlements::FENFORD)),
    ]
}
