//! Acceptance for #72: the native combat HUD model.
//!
//! The HUD shows the projected class resource, the viewer's cast or channel
//! progress and each action-bar slot's cooldown or global cooldown. Every value
//! comes from the received viewer projection and the immutable ability catalog;
//! the HUD never computes an authoritative combat result. Slot and cooldown
//! rules mirror the browser's derived ability state
//! (`web/src/world/units/ability-state.ts` `slotState`, `cast-bar.ts`
//! `castBarView`): the longer of a slot's own cooldown and the global cooldown
//! is shown (the ability's own cooldown on a tie), a cast fills while a
//! channel drains.
//!
//! Every fixture crosses the real wire codec (`mmorpg_protocol::encode_snapshot`
//! then `decode_snapshot`), so the HUD reads only what a client can receive.

use mmorpg_client::hud::{CombatHud, CooldownKind, SlotCooldown};
use mmorpg_core::{
    AbilityId, CastView, ClassChoice, Cooldown, EntityFlags, EntityKind, EntityRef, EntitySnapshot,
    GLOBAL_COOLDOWN_TICKS, PlayerClass, ResourceKind, ResourceView, SNAPSHOT_SCHEMA_VERSION, Sex,
    TargetDetail, ViewerState, ZoneCommand, ZoneId, ZoneSimulation, ZoneSnapshot, ability_by_id,
    greyhaven_vale,
};
use std::sync::Arc;

const EPSILON: f32 = 1e-6;

/// What a client receives for `snapshot`: one trip through the wire codec.
fn received(snapshot: &ZoneSnapshot) -> ZoneSnapshot {
    let payload = mmorpg_protocol::encode_snapshot(snapshot).expect("fixture is encodable");
    let decoded = mmorpg_protocol::decode_snapshot(&payload).expect("fixture decodes");
    assert_eq!(
        &decoded, snapshot,
        "the fixture survives the wire unchanged"
    );
    decoded
}

/// A level-6 viewer (every class ability learned) of `class` with nothing
/// running: full health, the class's starting resource, no cast, no cooldowns.
fn viewer_of(class: PlayerClass) -> ZoneSnapshot {
    let kind = resource_kind(class);
    ZoneSnapshot {
        content_revision: greyhaven_vale::REVISION,
        acknowledged_sequence: 1,
        viewer_id: 1,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(1),
        tick: 100,
        viewer: ViewerState {
            experience: 0,
            experience_to_next_level: 100,
            health: 90,
            max_health: 90,
            level: 6,
            class: Some(ClassChoice {
                class,
                sex: Sex::Male,
            }),
            resource: Some(ResourceView {
                kind,
                value: kind.start(6),
                max: kind.max(6),
            }),
            ..ViewerState::default()
        },
        cooldowns: Vec::new(),
        auras: Vec::new(),
        target_of_target: None,
        target_detail: TargetDetail::default(),
        inventory_revision: 1,
        inventory: None,
        equipment: None,
        quests: None,
        loot: None,
        events: Vec::new(),
        chat: Vec::new(),
        entities: vec![EntitySnapshot {
            kind: EntityKind::Player,
            id: 1,
            appearance: 0,
            position: [0, 90, 0],
            velocity: [0, 0, 0],
            facing: 0,
            level: 6,
            health_percent: 100,
            flags: EntityFlags::default(),
        }],
    }
}

const fn resource_kind(class: PlayerClass) -> ResourceKind {
    match class {
        PlayerClass::Warden => ResourceKind::Rage,
        PlayerClass::Ranger => ResourceKind::Focus,
        PlayerClass::Arcanist => ResourceKind::Mana,
    }
}

fn catalog_cooldown(ability: u8) -> u16 {
    ability_by_id(AbilityId::new(ability))
        .expect("a catalog ability")
        .cooldown
}

fn slot_ids(hud: &CombatHud) -> Vec<u8> {
    hud.slots.iter().map(|slot| slot.ability).collect()
}

fn assert_close(actual: Option<f32>, expected: f32, what: &str) {
    let actual = actual.unwrap_or_else(|| panic!("{what} must be shown"));
    assert!(
        (actual - expected).abs() < EPSILON,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn slots_hold_the_projected_class_kit_in_catalog_order() {
    for (class, expected) in [
        (PlayerClass::Warden, [1, 2, 3, 4]),
        (PlayerClass::Ranger, [5, 6, 7, 8]),
        (PlayerClass::Arcanist, [9, 10, 11, 12]),
    ] {
        let hud = CombatHud::from_projection(&received(&viewer_of(class)));
        assert_eq!(slot_ids(&hud), expected, "{class:?}");
        assert!(
            hud.slots
                .iter()
                .all(|slot| slot.cooldown.is_none() && !slot.casting),
            "{class:?}: nothing runs without projected cooldowns or a cast"
        );
        assert_eq!(hud.cast, None);
        assert_eq!(hud.cast_fill(), None);
        assert!(!hud.dead);
    }
}

#[test]
fn the_resource_shows_the_projected_value_of_its_maximum() {
    let mut warden = viewer_of(PlayerClass::Warden);
    warden.viewer.resource = Some(ResourceView {
        kind: ResourceKind::Rage,
        value: 25,
        max: 100,
    });
    let hud = CombatHud::from_projection(&received(&warden));
    assert_eq!(
        hud.resource,
        Some(ResourceView {
            kind: ResourceKind::Rage,
            value: 25,
            max: 100
        })
    );
    assert_close(hud.resource_fill(), 0.25, "a quarter of the rage bar");

    // Mana at level 6 is 220; the bar is the projected value, nothing derived.
    let mut arcanist = viewer_of(PlayerClass::Arcanist);
    arcanist.viewer.resource = Some(ResourceView {
        kind: ResourceKind::Mana,
        value: 55,
        max: 220,
    });
    let hud = CombatHud::from_projection(&received(&arcanist));
    assert_close(hud.resource_fill(), 0.25, "a quarter of the mana bar");

    // Rage starts empty, focus full.
    let empty = CombatHud::from_projection(&received(&viewer_of(PlayerClass::Warden)));
    assert_close(empty.resource_fill(), 0.0, "an empty rage bar");
    let full = CombatHud::from_projection(&received(&viewer_of(PlayerClass::Ranger)));
    assert_close(full.resource_fill(), 1.0, "a full focus bar");
}

#[test]
fn a_classless_viewer_shows_no_resource_no_cast_and_no_slots() {
    let mut classless = viewer_of(PlayerClass::Warden);
    classless.viewer.class = None;
    classless.viewer.resource = None;
    classless.viewer.level = 1;
    classless.entities[0].level = 1;
    let hud = CombatHud::from_projection(&received(&classless));
    assert_eq!(hud.resource, None);
    assert_eq!(hud.resource_fill(), None);
    assert_eq!(hud.cast, None);
    assert_eq!(hud.cast_fill(), None);
    assert!(hud.slots.is_empty(), "no class, no action bar");
}

#[test]
fn each_slot_shows_its_running_cooldown_against_the_catalog_cooldown() {
    let mut warden = viewer_of(PlayerClass::Warden);
    warden.viewer.resource = Some(ResourceView {
        kind: ResourceKind::Rage,
        value: 40,
        max: 100,
    });
    // Shield Bash (2) and Rallying Cry (3) are cooling down; no global cooldown.
    warden.cooldowns = vec![
        Cooldown {
            ability: AbilityId::new(2),
            remaining: 300,
        },
        Cooldown {
            ability: AbilityId::new(3),
            remaining: 12,
        },
    ];
    let hud = CombatHud::from_projection(&received(&warden));
    assert_eq!(slot_ids(&hud), [1, 2, 3, 4]);
    let cooldowns: Vec<_> = hud.slots.iter().map(|slot| slot.cooldown).collect();
    assert_eq!(
        cooldowns,
        [
            None,
            Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 300,
                total: catalog_cooldown(2),
            }),
            Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 12,
                total: catalog_cooldown(3),
            }),
            None,
        ]
    );
    assert!(catalog_cooldown(2) >= 300 && catalog_cooldown(3) >= 12);
}

#[test]
fn the_global_cooldown_covers_every_slot_unless_a_longer_cooldown_runs() {
    let mut warden = viewer_of(PlayerClass::Warden);
    warden.viewer.resource = Some(ResourceView {
        kind: ResourceKind::Rage,
        value: 40,
        max: 100,
    });
    warden.viewer.global_cooldown = 30;
    warden.cooldowns = vec![
        // Longer than the global cooldown: the slot shows its own cooldown.
        Cooldown {
            ability: AbilityId::new(2),
            remaining: 200,
        },
        // Exactly the global cooldown: the ability's own cooldown wins the tie.
        Cooldown {
            ability: AbilityId::new(3),
            remaining: 30,
        },
        // Shorter than the global cooldown: the slot shows the global cooldown.
        Cooldown {
            ability: AbilityId::new(4),
            remaining: 10,
        },
    ];
    let hud = CombatHud::from_projection(&received(&warden));
    let global = Some(SlotCooldown {
        kind: CooldownKind::Global,
        remaining: 30,
        total: GLOBAL_COOLDOWN_TICKS,
    });
    let cooldowns: Vec<_> = hud.slots.iter().map(|slot| slot.cooldown).collect();
    assert_eq!(
        cooldowns,
        [
            // Heroic Strike has no cooldown of its own but waits on the global one.
            global,
            Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 200,
                total: catalog_cooldown(2),
            }),
            Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 30,
                total: catalog_cooldown(3),
            }),
            global,
        ]
    );
}

#[test]
fn a_channel_drains_and_marks_its_slot() {
    let mut arcanist = viewer_of(PlayerClass::Arcanist);
    let blizzard = ability_by_id(AbilityId::new(12)).unwrap();
    arcanist.viewer.resource = Some(ResourceView {
        kind: ResourceKind::Mana,
        value: 220 - blizzard.cost,
        max: 220,
    });
    arcanist.viewer.cast = Some(CastView {
        ability: AbilityId::new(12),
        elapsed: 45,
        total: 180,
        channel: true,
    });
    arcanist.viewer.in_combat = true;
    let hud = CombatHud::from_projection(&received(&arcanist));
    assert_eq!(
        hud.cast,
        Some(CastView {
            ability: AbilityId::new(12),
            elapsed: 45,
            total: 180,
            channel: true,
        })
    );
    assert_close(hud.cast_fill(), 0.75, "a quarter-run channel");
    let casting: Vec<_> = hud.slots.iter().map(|slot| slot.casting).collect();
    assert_eq!(casting, [false, false, false, true], "only Blizzard's slot");
}

#[test]
fn a_dead_viewer_shows_death_with_the_projected_resource_and_no_cast() {
    let mut warden = viewer_of(PlayerClass::Warden);
    warden.viewer.dead = true;
    warden.viewer.health = 0;
    warden.entities[0].health_percent = 0;
    warden.entities[0].flags = EntityFlags {
        dead: true,
        ..EntityFlags::default()
    };
    let hud = CombatHud::from_projection(&received(&warden));
    assert!(hud.dead);
    assert_eq!(hud.cast, None);
    assert_eq!(hud.cast_fill(), None);
    assert_close(
        hud.resource_fill(),
        0.0,
        "death drains rage in the projection",
    );
    assert_eq!(slot_ids(&hud), [1, 2, 3, 4], "the bar stays");
}

/// End to end through core: a level-1 Arcanist starts Firebolt (a 60-tick
/// cast) at a nearby creature on the hosted Greyhaven Vale content. Fifteen
/// ticks in, the received projection shows the cast a quarter done, the global
/// cooldown on every slot and full mana (a cast pays on completion).
#[test]
fn a_real_cast_projection_drives_the_cast_bar_and_global_cooldown() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 2, sex: 0 })
        .unwrap();
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [0, 90, -2_000];
    state.creatures[0].position = [180, 45, -1_700];
    let creature = EntityRef::Creature(state.creatures[0].creature_id);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
    zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(creature)))
        .unwrap();
    zone.advance_tick().unwrap();
    zone.apply_command(
        1,
        3,
        ZoneCommand::UseAbility {
            ability: 9,
            target: None,
        },
    )
    .unwrap();
    for _ in 0..15 {
        zone.advance_tick().unwrap();
    }
    let projection = received(&zone.snapshot_for_player(1).unwrap());
    assert_eq!(
        projection
            .viewer
            .cast
            .map(|cast| (cast.ability, cast.elapsed)),
        Some((AbilityId::new(9), 15)),
        "fixture: Firebolt is fifteen ticks into its cast"
    );

    let hud = CombatHud::from_projection(&projection);
    assert_eq!(hud.cast, projection.viewer.cast);
    assert_close(hud.cast_fill(), 0.25, "Firebolt 15 of 60 ticks");
    assert_eq!(hud.resource, projection.viewer.resource);
    assert_close(hud.resource_fill(), 1.0, "a cast pays on completion");
    assert_eq!(slot_ids(&hud), [9, 10, 11, 12]);
    let casting: Vec<_> = hud.slots.iter().map(|slot| slot.casting).collect();
    assert_eq!(casting, [true, false, false, false]);
    let global = projection.viewer.global_cooldown;
    assert!(global > 0, "fixture: the global cooldown is running");
    for slot in &hud.slots {
        assert_eq!(
            slot.cooldown,
            Some(SlotCooldown {
                kind: CooldownKind::Global,
                remaining: global,
                total: GLOBAL_COOLDOWN_TICKS,
            }),
            "slot {}",
            slot.ability
        );
    }

    // The HUD follows the next received projection, never a local clock.
    zone.advance_tick().unwrap();
    let next = received(&zone.snapshot_for_player(1).unwrap());
    let later = CombatHud::from_projection(&next);
    assert_eq!(later.cast.map(|cast| cast.elapsed), Some(16));
    assert_eq!(
        later.slots[0].cooldown.map(|cooldown| cooldown.remaining),
        Some(global - 1)
    );
    assert_eq!(
        CombatHud::from_projection(&projection),
        hud,
        "the same projection always yields the same HUD"
    );
}
