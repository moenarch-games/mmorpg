//! The combat HUD: the projected class resource, the viewer's cast or channel
//! and each action-bar slot's state.
//!
//! Every value is read from the received viewer projection and the immutable
//! ability catalog; nothing here decides whether an ability can be used, the
//! zone answers every `UseAbility` itself.
use mmorpg_core::{
    ABILITY_CATALOG, Ability, AbilityId, AbilityRange, AbilityUser, CastView, Cooldown, EntityRef,
    GLOBAL_COOLDOWN_TICKS, PlayerClass, ResourceView, ViewerState, ZoneSnapshot, ability_by_id,
};

/// At most this many abilities sit on the action bar.
pub const ACTION_BAR_SLOTS: usize = 8;

/// Which wait a slot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CooldownKind {
    /// The ability's own cooldown.
    Ability,
    /// The global cooldown shared by every slot.
    Global,
}

/// A running wait on a slot, in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotCooldown {
    pub kind: CooldownKind,
    pub remaining: u16,
    pub total: u16,
}

impl SlotCooldown {
    /// The share of the wait still to run, 0–1.
    #[must_use]
    pub fn share(self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            (f32::from(self.remaining) / f32::from(self.total)).clamp(0.0, 1.0)
        }
    }
}

/// One action-bar slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotState {
    pub ability: u8,
    /// The viewer's level reaches the ability's unlock level.
    pub learned: bool,
    /// Level at which the class learns the ability.
    pub unlock_level: u8,
    /// The class resource is below the cost.
    pub resource_short: bool,
    /// The target is known and farther than the ability reaches.
    pub out_of_range: bool,
    /// The viewer is casting or channelling this slot's ability.
    pub casting: bool,
    pub cooldown: Option<SlotCooldown>,
}

/// The class abilities in catalog id order: action-bar slots 1–8.
#[must_use]
pub fn class_kit(class: PlayerClass) -> Vec<u8> {
    ABILITY_CATALOG
        .iter()
        .filter(|ability| ability.user == AbilityUser::Class(class))
        .map(|ability| ability.id.get())
        .take(ACTION_BAR_SLOTS)
        .collect()
}

/// XZ centre distance in core units from the viewer to its target, `None`
/// without a target or when targeting itself. A selected target the projection
/// omits has left the interest radius, beyond every targeted ability's reach,
/// so it counts as infinitely far.
#[must_use]
pub fn target_distance(snapshot: &ZoneSnapshot) -> Option<f64> {
    let me = EntityRef::Player(snapshot.viewer_id);
    let selected = snapshot.viewer.target.filter(|target| *target != me)?;
    let position = |entity: EntityRef| {
        snapshot
            .entities
            .iter()
            .find(|candidate| candidate.entity() == entity)
            .map(|candidate| candidate.position)
    };
    let viewer = position(me)?;
    Some(position(selected).map_or(f64::INFINITY, |target| {
        f64::from(target[0] - viewer[0]).hypot(f64::from(target[2] - viewer[2]))
    }))
}

/// The wait a slot shows: the longer of the ability's own cooldown and the
/// global cooldown, since that is the one the player waits on; a tie shows the
/// ability's own cooldown.
#[must_use]
pub fn slot_cooldown(
    ability: &Ability,
    cooldowns: &[Cooldown],
    global: u16,
) -> Option<SlotCooldown> {
    let running = cooldowns
        .iter()
        .find(|cooldown| cooldown.ability == ability.id)
        .map_or(0, |cooldown| cooldown.remaining);
    if running > 0 && running >= global {
        Some(SlotCooldown {
            kind: CooldownKind::Ability,
            remaining: running,
            total: ability.cooldown,
        })
    } else if global > 0 {
        Some(SlotCooldown {
            kind: CooldownKind::Global,
            remaining: global,
            total: GLOBAL_COOLDOWN_TICKS,
        })
    } else {
        None
    }
}

/// What the slot holding `ability` shows; `distance` is [`target_distance`].
#[must_use]
pub fn slot_state(
    ability: &Ability,
    viewer: &ViewerState,
    cooldowns: &[Cooldown],
    distance: Option<f64>,
) -> SlotState {
    // A supportive ability lands on the caster without a friendly selection, so
    // it is never shown out of range.
    let reach = match ability.range {
        AbilityRange::Friendly(_) => None,
        range => range.reach(),
    };
    SlotState {
        ability: ability.id.get(),
        learned: viewer.level >= ability.level,
        unlock_level: ability.level,
        resource_short: viewer
            .resource
            .is_some_and(|resource| resource.value < ability.cost),
        out_of_range: reach
            .zip(distance)
            .is_some_and(|(reach, distance)| distance > f64::from(reach)),
        casting: viewer.cast.is_some_and(|cast| cast.ability == ability.id),
        cooldown: slot_cooldown(ability, cooldowns, viewer.global_cooldown),
    }
}

/// What the cast bar shows for a cast or channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CastBar {
    pub ability: u8,
    /// Filled share, 0–1: a cast fills, a channel drains as it runs out.
    pub fill: f32,
    /// Ticks left.
    pub remaining: u16,
    pub channel: bool,
}

/// The cast bar for `cast`, the viewer's own or its target's.
#[must_use]
pub fn cast_bar(cast: CastView) -> CastBar {
    let progress = if cast.total == 0 {
        1.0
    } else {
        (f32::from(cast.elapsed) / f32::from(cast.total)).clamp(0.0, 1.0)
    };
    CastBar {
        ability: cast.ability.get(),
        fill: if cast.channel {
            1.0 - progress
        } else {
            progress
        },
        remaining: cast.total.saturating_sub(cast.elapsed),
        channel: cast.channel,
    }
}

/// What the combat HUD shows for one received projection.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatHud {
    pub resource: Option<ResourceView>,
    pub cast: Option<CastBar>,
    /// The projected class kit in catalog order; empty without a class.
    pub slots: Vec<SlotState>,
    pub dead: bool,
}

impl CombatHud {
    #[must_use]
    pub fn from_projection(snapshot: &ZoneSnapshot) -> Self {
        let viewer = &snapshot.viewer;
        let distance = target_distance(snapshot);
        let slots = viewer
            .class
            .map(|choice| class_kit(choice.class))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|ability| ability_by_id(AbilityId::new(ability)))
            .map(|ability| slot_state(ability, viewer, &snapshot.cooldowns, distance))
            .collect();
        Self {
            resource: viewer.resource,
            cast: viewer.cast.map(cast_bar),
            slots,
            dead: viewer.dead,
        }
    }

    /// The resource bar's fill, `value / max`.
    #[must_use]
    pub fn resource_fill(&self) -> Option<f32> {
        self.resource.map(|resource| {
            if resource.max == 0 {
                0.0
            } else {
                (f32::from(resource.value) / f32::from(resource.max)).clamp(0.0, 1.0)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{EntityFlags, EntityKind, EntitySnapshot, ResourceKind};

    const FIREBOLT: u8 = 9;
    const FROST_NOVA: u8 = 10;
    const BLIZZARD: u8 = 12;
    const STRIKE: u8 = 1;

    fn ability(id: u8) -> &'static Ability {
        ability_by_id(AbilityId::new(id)).unwrap()
    }

    fn viewer() -> ViewerState {
        ViewerState {
            level: 4,
            resource: Some(ResourceView {
                kind: ResourceKind::Mana,
                value: 100,
                max: 176,
            }),
            ..ViewerState::default()
        }
    }

    #[test]
    fn class_kits_hold_the_catalog_order() {
        assert_eq!(class_kit(PlayerClass::Warden), [1, 2, 3, 4, 17]);
        assert_eq!(class_kit(PlayerClass::Ranger), [5, 6, 7, 8, 19, 20]);
        assert_eq!(
            class_kit(PlayerClass::Arcanist),
            [9, 10, 11, 12, 16, 21, 22, 23]
        );
        assert_eq!(class_kit(PlayerClass::Necromancer), [26, 27, 28, 29, 45]);
        assert_eq!(class_kit(PlayerClass::Rogue), [30, 31, 32, 33, 42, 43, 44]);
        assert_eq!(class_kit(PlayerClass::Cleric), [34, 35, 36, 37]);
        assert_eq!(class_kit(PlayerClass::Druid), [38, 39, 40, 41, 46]);
    }

    #[test]
    fn a_slot_reads_level_resource_and_range() {
        let firebolt = ability(FIREBOLT);
        let state = slot_state(firebolt, &viewer(), &[], Some(1_000.0));
        assert!(state.learned && !state.resource_short && !state.out_of_range);
        let short = ViewerState {
            resource: Some(ResourceView {
                kind: ResourceKind::Mana,
                value: firebolt.cost - 1,
                max: 176,
            }),
            ..viewer()
        };
        assert!(slot_state(firebolt, &short, &[], None).resource_short);
        let reach = f64::from(firebolt.range.reach().unwrap());
        assert!(!slot_state(firebolt, &viewer(), &[], Some(reach)).out_of_range);
        assert!(slot_state(firebolt, &viewer(), &[], Some(reach + 1.0)).out_of_range);
        assert!(!slot_state(ability(STRIKE), &viewer(), &[], Some(250.0)).out_of_range);
        assert!(slot_state(ability(STRIKE), &viewer(), &[], Some(251.0)).out_of_range);
        // A self-centred ability has no reach; an unknown distance is never out of range.
        assert!(!slot_state(ability(FROST_NOVA), &viewer(), &[], Some(9_000.0)).out_of_range);
        assert!(!slot_state(firebolt, &viewer(), &[], None).out_of_range);
        assert!(slot_state(firebolt, &viewer(), &[], Some(f64::INFINITY)).out_of_range);
        // A supportive ability falls back to the caster.
        let renew = ability_by_id(mmorpg_core::ability::ids::RENEW).unwrap();
        assert!(!slot_state(renew, &viewer(), &[], Some(9_000.0)).out_of_range);
        let blizzard = slot_state(ability(BLIZZARD), &viewer(), &[], None);
        assert!(!blizzard.learned);
        assert_eq!(blizzard.unlock_level, ability(BLIZZARD).level);
    }

    #[test]
    fn the_longer_wait_is_shown_and_a_tie_shows_the_ability() {
        let nova = ability(FROST_NOVA);
        let running = [Cooldown {
            ability: nova.id,
            remaining: 300,
        }];
        assert_eq!(
            slot_cooldown(nova, &running, 40),
            Some(SlotCooldown {
                kind: CooldownKind::Ability,
                remaining: 300,
                total: nova.cooldown
            })
        );
        let gcd = Some(SlotCooldown {
            kind: CooldownKind::Global,
            remaining: 40,
            total: GLOBAL_COOLDOWN_TICKS,
        });
        assert_eq!(slot_cooldown(ability(FIREBOLT), &running, 40), gcd);
        let short = [Cooldown {
            ability: nova.id,
            remaining: 10,
        }];
        assert_eq!(slot_cooldown(nova, &short, 40), gcd);
        assert_eq!(
            slot_cooldown(nova, &short, 10).unwrap().kind,
            CooldownKind::Ability
        );
        assert_eq!(slot_cooldown(nova, &[], 0), None);
    }

    #[test]
    fn a_cast_fills_and_a_channel_drains() {
        let cast = |ability, elapsed, total, channel| CastView {
            ability: AbilityId::new(ability),
            elapsed,
            total,
            channel,
        };
        assert_eq!(
            cast_bar(cast(FIREBOLT, 15, 60, false)),
            CastBar {
                ability: FIREBOLT,
                fill: 0.25,
                remaining: 45,
                channel: false
            }
        );
        assert_eq!(cast_bar(cast(BLIZZARD, 45, 180, true)).fill, 0.75);
        assert_eq!(cast_bar(cast(BLIZZARD, 0, 180, true)).fill, 1.0);
        assert_eq!(cast_bar(cast(FIREBOLT, 70, 60, false)).remaining, 0);
    }

    #[test]
    fn the_target_distance_ignores_self_and_treats_an_unseen_target_as_infinitely_far() {
        let entity = |kind, id, x, z| EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [x, 90, z],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        };
        let mut snapshot = crate::test_support::snapshot(7, 1);
        snapshot.entities = vec![
            entity(EntityKind::Player, 7, 0, 0),
            entity(EntityKind::Creature, 3, 600, 800),
        ];
        snapshot.viewer.target = Some(EntityRef::new(EntityKind::Creature, 3));
        assert_eq!(target_distance(&snapshot), Some(1_000.0));
        snapshot.viewer.target = Some(EntityRef::new(EntityKind::Creature, 999));
        assert_eq!(target_distance(&snapshot), Some(f64::INFINITY));
        snapshot.viewer.target = Some(EntityRef::Player(7));
        assert_eq!(target_distance(&snapshot), None);
        snapshot.viewer.target = None;
        assert_eq!(target_distance(&snapshot), None);
    }
}
