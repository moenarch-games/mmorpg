//! Exhaustive XZ semantics. This development-only reference never uses index
//! buckets. It covers zones whose units are all players (no creatures or
//! NPCs), which the interest workloads and sequences use.
use mmorpg_core::unit::{health_percent, player_damage, player_max_health};
use mmorpg_core::{
    CanonicalPlayerSnapshot, CanonicalZoneSnapshot, EntityFlags, EntityKind, EntityRef,
    EntitySnapshot, INTEREST_RADIUS_UNITS, MAX_VISIBLE_ENTITIES, ViewerState, ZoneSnapshot,
};

/// Every player within the inclusive radius: the observer first, its target
/// next, then by `(squared XZ distance, kind, id)`, keeping at most the
/// relevance cap.
pub fn exhaustive_projection(
    canonical: &CanonicalZoneSnapshot,
    observer: &CanonicalPlayerSnapshot,
) -> ZoneSnapshot {
    assert!(
        canonical.creatures.is_empty(),
        "the oracle covers players only"
    );
    let distance_squared = |candidate: &CanonicalPlayerSnapshot| {
        let dx = i128::from(candidate.position[0]) - i128::from(observer.position[0]);
        let dz = i128::from(candidate.position[2]) - i128::from(observer.position[2]);
        dx * dx + dz * dz
    };
    let viewer = EntityRef::Player(observer.player_id);
    let mut relevant: Vec<_> = canonical
        .players
        .iter()
        .filter(|candidate| distance_squared(candidate) <= i128::from(INTEREST_RADIUS_UNITS).pow(2))
        .collect();
    relevant.sort_by_key(|candidate| {
        let entity = EntityRef::Player(candidate.player_id);
        let class = if entity == viewer {
            0
        } else if observer.combat.target == Some(entity) {
            1
        } else {
            2
        };
        (class, distance_squared(candidate), candidate.player_id)
    });
    let combat = &observer.combat;
    let target_of_target = match combat.target {
        Some(EntityRef::Player(target)) => canonical
            .players
            .iter()
            .find(|player| player.player_id == target)
            .and_then(|player| player.combat.target),
        _ => None,
    };
    let sheet = canonical.tick == observer.inventory_changed_at
        || canonical.tick == observer.quests_changed_at
        || canonical.tick.is_multiple_of(10);
    ZoneSnapshot {
        loot: None,
        content_revision: canonical.content_revision,
        acknowledged_sequence: observer.last_sequence,
        viewer_id: observer.player_id,
        inventory_revision: observer.inventory_revision,
        inventory: sheet.then(|| observer.inventory.clone()),
        equipment: sheet.then_some(observer.equipment),
        // The oracle's content has no quests.
        quests: sheet.then(mmorpg_core::QuestSheet::default),
        schema_version: canonical.schema_version,
        zone_id: canonical.zone_id,
        tick: canonical.tick,
        viewer: ViewerState {
            copper: observer.copper,
            experience: combat.experience,
            experience_to_next_level: mmorpg_core::experience_to_next_level(combat.level).unwrap(),
            health: combat.health,
            max_health: max_health(observer),
            level: combat.level,
            dead: combat.health == 0,
            in_combat: combat.combat_timer > 0,
            auto_attacking: combat.auto_attack,
            target: combat.target,
            // The oracle's players never choose a class.
            class: None,
            resource: None,
            cast: None,
            global_cooldown: 0,
            damage: {
                let bonus = observer.equipment.totals().damage_bonus(None);
                player_damage(combat.level).map(|end| end + bonus)
            },
        },
        cooldowns: Vec::new(),
        auras: Vec::new(),
        target_of_target,
        target_detail: mmorpg_core::TargetDetail::default(),
        events: combat.events.clone(),
        chat: Vec::new(),
        entities: relevant
            .into_iter()
            .take(MAX_VISIBLE_ENTITIES)
            .map(|candidate| EntitySnapshot {
                kind: EntityKind::Player,
                id: candidate.player_id,
                appearance: 0,
                position: candidate.position,
                velocity: candidate
                    .velocity
                    .map(|component| component.clamp(-128, 127) as i8),
                facing: candidate.facing,
                level: candidate.combat.level,
                health_percent: health_percent(candidate.combat.health, max_health(candidate)),
                flags: EntityFlags {
                    dead: candidate.combat.health == 0,
                    in_combat: candidate.combat.combat_timer > 0,
                    targets_viewer: candidate.player_id != observer.player_id
                        && candidate.combat.target == Some(viewer),
                    ..EntityFlags::default()
                },
            })
            .collect(),
    }
}

/// Level health plus the equipped stamina's bonus.
fn max_health(player: &CanonicalPlayerSnapshot) -> u32 {
    player_max_health(player.combat.level) + player.equipment.totals().bonus_health()
}
