//! Temporary builds use ordinary character rules, with an explicit no-durable-export boundary.
use mmorpg_core::{
    CharacterId, CharacterPersistence, ClassChoice, EphemeralCharacterBuild, ItemId,
    MAX_PLAYER_LEVEL, PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale,
};
use std::sync::Arc;

fn content() -> Arc<mmorpg_core::ZoneContent> {
    Arc::new(
        Arc::unwrap_or_clone(greyhaven_vale::content())
            .with_character_persistence(CharacterPersistence::Ephemeral),
    )
}
fn choice(class: PlayerClass) -> ClassChoice {
    ClassChoice {
        class,
        sex: Sex::Male,
    }
}

#[test]
fn all_supported_classes_seed_legal_max_level_state_and_restore_exactly() {
    for class in PlayerClass::ALL {
        let build = EphemeralCharacterBuild::standard(choice(class)).unwrap();
        let content = content();
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(100), Arc::clone(&content)).unwrap();
        zone.admit_ephemeral(1, &build).unwrap();
        let state = zone.snapshot().unwrap();
        let player = &state.players[0];
        assert_eq!(player.combat.level, MAX_PLAYER_LEVEL);
        assert_eq!(player.combat.abilities.class, Some(choice(class)));
        assert_eq!(player.equipment, build.equipment());
        assert_eq!(
            player.combat.health,
            mmorpg_core::unit::player_max_health(MAX_PLAYER_LEVEL)
                + build.equipment().totals().bonus_health()
        );
        assert_eq!(
            player.combat.abilities.resource,
            class.resource().start(MAX_PLAYER_LEVEL)
        );
        assert!(player.combat.abilities.cooldowns.is_empty());
        assert_eq!(player.combat.abilities.global_cooldown, 0);
        assert_eq!(player.combat.experience, 0);
        assert_eq!(player.copper, 0);
        assert!(player.inventory.slots().iter().all(Option::is_none));
        assert!(
            zone.admit_ephemeral(1, &build).is_err(),
            "admission cannot edit an active life"
        );
        assert_eq!(zone.snapshot().unwrap(), state);
        let mut restored = ZoneSimulation::from_snapshot(state, content).unwrap();
        for _ in 0..5 {
            zone.advance_tick().unwrap();
            restored.advance_tick().unwrap();
            assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
        }
        assert!(zone.remove_player(1));
        zone.admit_ephemeral(1, &build).unwrap();
        let reset = zone.snapshot().unwrap();
        assert_eq!(reset.players[0].combat.level, MAX_PLAYER_LEVEL);
        assert_eq!(reset.players[0].equipment, build.equipment());
        assert_eq!(
            reset.players[0].combat.abilities.resource,
            class.resource().start(MAX_PLAYER_LEVEL)
        );
        assert!(reset.players[0].combat.abilities.cooldowns.is_empty());
    }
}

#[test]
fn temporary_lives_cannot_overwrite_or_import_durable_progress() {
    let class = choice(PlayerClass::Ranger);
    let mut durable =
        ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    durable.create_character(1, class).unwrap();
    let record = durable
        .capture_character(1, CharacterId::new("existing-ranger").unwrap())
        .unwrap();
    let original = record.clone();
    let build = EphemeralCharacterBuild::from_character(&record);
    let mut temporary = ZoneSimulation::with_content(ZoneId::new(100), content()).unwrap();
    temporary.admit_ephemeral(1, &build).unwrap();
    assert_eq!(build.class(), record.class);
    assert_eq!(record, original);
    let state = temporary.snapshot().unwrap();
    assert!(
        temporary
            .capture_character(1, record.character_id.clone())
            .is_err()
    );
    assert!(temporary.admit_character(2, &record).is_err());
    assert!(temporary.create_character(3, class).is_err());
    assert_eq!(temporary.snapshot().unwrap(), state);
    assert!(ZoneSimulation::from_snapshot(state, greyhaven_vale::content()).is_err());
    assert_eq!(
        durable
            .capture_character(1, record.character_id.clone())
            .unwrap(),
        original
    );
}

#[test]
fn admission_and_selection_fail_closed_before_mutating_ordinary_zones() {
    let class = choice(PlayerClass::Warden);
    let build = EphemeralCharacterBuild::standard(class).unwrap();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    let before = zone.snapshot().unwrap();
    assert!(zone.admit_ephemeral(1, &build).is_err());
    assert_eq!(zone.snapshot().unwrap(), before);
    assert!(
        EphemeralCharacterBuild::new(
            class,
            [Some(ItemId::new(999)), None, None, None, None, None]
        )
        .is_err()
    );
    assert!(
        EphemeralCharacterBuild::new(class, [Some(ItemId::new(5)), None, None, None, None, None])
            .is_err()
    );
}

#[test]
fn temporary_arena_policies_compose_and_both_bind_recovery() {
    use mmorpg_core::PlayerCombatPolicy;
    let base = Arc::unwrap_or_clone(greyhaven_vale::content());
    let first = base
        .clone()
        .with_player_combat(PlayerCombatPolicy::FreeForAll)
        .with_character_persistence(CharacterPersistence::Ephemeral);
    let second = base
        .with_character_persistence(CharacterPersistence::Ephemeral)
        .with_player_combat(PlayerCombatPolicy::FreeForAll);
    assert_eq!(first, second);
    let content = Arc::new(first);
    let mut zone = ZoneSimulation::with_content(ZoneId::new(100), Arc::clone(&content)).unwrap();
    let build = EphemeralCharacterBuild::standard(choice(PlayerClass::Warden)).unwrap();
    zone.admit_ephemeral(1, &build).unwrap();
    zone.admit_ephemeral(2, &build).unwrap();
    assert!(zone.players_are_hostile(1, 2));
    let state = zone.snapshot().unwrap();
    ZoneSimulation::from_snapshot(state.clone(), Arc::clone(&content)).unwrap();
    let durable = Arc::new(
        Arc::unwrap_or_clone(Arc::clone(&content))
            .with_character_persistence(CharacterPersistence::Durable),
    );
    assert!(ZoneSimulation::from_snapshot(state.clone(), durable).is_err());
    let safe = Arc::new(Arc::unwrap_or_clone(content).with_player_combat(PlayerCombatPolicy::Safe));
    assert!(ZoneSimulation::from_snapshot(state, safe).is_err());
}
