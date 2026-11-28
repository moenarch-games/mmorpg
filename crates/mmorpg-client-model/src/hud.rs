//! The combat HUD: the projected class resource, the viewer's cast or channel
//! and each action-bar slot's state.
//!
//! Every value is read from the received viewer projection and the immutable
//! ability catalog; nothing here decides whether an ability can be used, the
//! zone answers every `UseAbility` itself.
use mmorpg_core::{
    ABILITY_CATALOG, Ability, AbilityId, AbilityRange, AbilityUser, AuraKind, AuraView, CastView,
    Cooldown, EntityRef, PlayerClass, ResourceView, TICK_HZ, ViewerState, ZoneSnapshot,
    ability_by_id, trig, unit,
};

/// At most this many abilities sit on the action bar: two rows, the second reached with Shift
/// (Atlas `5.2.i294`).
pub const ACTION_BAR_SLOTS: usize = 2 * ACTION_BAR_ROW_SLOTS;
/// One row of the action bar: keys 1–9 and 0.
pub const ACTION_BAR_ROW_SLOTS: usize = 10;

/// The key that uses slot `index` (0-based): `1`–`9`, `0`, then `Shift+1`–`Shift+0` on the
/// second row.
#[must_use]
pub fn slot_key_label(index: usize) -> String {
    let digit = |offset: usize| {
        if offset == ACTION_BAR_ROW_SLOTS - 1 {
            '0'
        } else {
            char::from_digit(u32::try_from(offset + 1).unwrap_or(0), 10).unwrap_or('?')
        }
    };
    if index < ACTION_BAR_ROW_SLOTS {
        digit(index).to_string()
    } else {
        format!("Shift+{}", digit(index - ACTION_BAR_ROW_SLOTS))
    }
}

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
    pub ability: u32,
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

/// The tooltip range line for an ability, from its authoritative catalog range (#1013): `Self`,
/// `Melee range`, `30 m range`, or for a supportive ability that lands on a selected friendly
/// player (otherwise on the caster) `30 m range, friendly or self`.
#[must_use]
pub fn range_label(range: AbilityRange) -> String {
    let metres = |units: i32| {
        let metres = f64::from(units) / f64::from(mmorpg_core::UNITS_PER_METRE);
        format!("{metres} m range")
    };
    match range {
        AbilityRange::SelfCentred => "Self".to_owned(),
        AbilityRange::Melee => "Melee range".to_owned(),
        AbilityRange::Units(units) => metres(units),
        AbilityRange::Friendly(units) => format!("{}, friendly or self", metres(units)),
    }
}

/// The class abilities in catalog id order: action-bar slots 1–20, the first ten on the first
/// row and the rest on the Shift row.
#[must_use]
pub fn class_kit(class: PlayerClass) -> Vec<u32> {
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

/// Whether the zone would aim `ability` at the viewer's selection (Atlas `5.2.i884`): the
/// selection lies inside the ability's aim cone around the viewer's facing, the swing arc for a
/// melee ability or 15° either side for a ranged one. A selection outside it is ignored for the
/// unit in front, so its distance says nothing about the slot; one the projection omits counts
/// as inside, beyond every reach.
#[must_use]
pub fn aims_at_selection(snapshot: &ZoneSnapshot, ability: &Ability) -> bool {
    let me = EntityRef::Player(snapshot.viewer_id);
    let find = |entity: EntityRef| {
        snapshot
            .entities
            .iter()
            .find(|candidate| candidate.entity() == entity)
    };
    let (Some(viewer), Some(selected)) = (find(me), snapshot.viewer.target) else {
        return false;
    };
    let Some(target) = find(selected) else {
        return true;
    };
    let half_width = match ability.range {
        AbilityRange::Melee => unit::SWING_ARC_HALF_WIDTH,
        _ => unit::AIM_CONE_HALF_WIDTH,
    };
    trig::yaw_from_vector(
        target.position[0] - viewer.position[0],
        target.position[2] - viewer.position[2],
    )
    .is_none_or(|yaw| yaw.wrapping_sub(viewer.facing).cast_signed().unsigned_abs() <= half_width)
}

/// The wait a slot shows: the longer of the ability's own cooldown and the
/// global cooldown, since that is the one the player waits on; a tie shows the
/// ability's own cooldown. `global` is the global cooldown's ticks left and the length it began
/// with, which haste may have shortened (#884): the sweep runs over the wait actually begun.
#[must_use]
pub fn slot_cooldown(
    ability: &Ability,
    cooldowns: &[Cooldown],
    [global, global_total]: [u16; 2],
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
            total: global_total.max(global),
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
        cooldown: slot_cooldown(
            ability,
            cooldowns,
            [viewer.global_cooldown, viewer.global_cooldown_total],
        ),
    }
}

/// What the cast bar shows for a cast or channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CastBar {
    pub ability: u32,
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
        // Each slot counts the selection only where the zone would aim at it (#884).
        let slots = viewer
            .class
            .map(|choice| class_kit(choice.class))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|ability| ability_by_id(AbilityId::new(ability)))
            .map(|ability| {
                let distance = distance.filter(|_| aims_at_selection(snapshot, ability));
                slot_state(ability, viewer, &snapshot.cooldowns, distance)
            })
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

impl SlotCooldown {
    /// The whole seconds a slot's own cooldown shows while at least 2 s remain, rounded up;
    /// `None` below that and for the global cooldown, which only dims the slot (#1280).
    #[must_use]
    pub fn shown_seconds(self) -> Option<u16> {
        (self.kind == CooldownKind::Ability && self.remaining >= 2 * TICK_HZ)
            .then(|| self.remaining.div_ceil(TICK_HZ))
    }
}

/// Time left as a short label: whole seconds rounded up (at least `1s`), then whole minutes
/// rounded up from one minute.
#[must_use]
pub fn remaining_label(ticks: u16) -> String {
    let seconds = ticks.div_ceil(TICK_HZ).max(1);
    if seconds >= 60 {
        format!("{}m", seconds.div_ceil(60))
    } else {
        format!("{seconds}s")
    }
}

/// Whether an aura helps its bearer or hurts it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuraPolarity {
    Buff,
    Debuff,
}

/// Heals, shields, haste, a fresh corpse and wolf form help their bearer; damage over time,
/// roots, snares and stuns hurt it.
#[must_use]
pub const fn aura_polarity(kind: AuraKind) -> AuraPolarity {
    match kind {
        AuraKind::HealOverTime
        | AuraKind::Absorb
        | AuraKind::Haste
        | AuraKind::Corpse
        | AuraKind::Shapeshift => AuraPolarity::Buff,
        AuraKind::DamageOverTime | AuraKind::Root | AuraKind::Snare | AuraKind::Stun => {
            AuraPolarity::Debuff
        }
    }
}

/// What an aura does, from its kind and amount.
#[must_use]
pub fn aura_effect_text(aura: AuraView) -> String {
    let amount = aura.amount;
    match aura.kind {
        AuraKind::DamageOverTime => format!("{amount} damage over time"),
        AuraKind::HealOverTime => format!("{amount} healing over time"),
        AuraKind::Absorb => format!("Absorbs {amount} damage"),
        AuraKind::Root => "Rooted".to_owned(),
        AuraKind::Snare => format!("Slowed by {amount}%"),
        AuraKind::Stun => "Stunned".to_owned(),
        AuraKind::Haste => format!("Attacks {amount}% faster"),
        AuraKind::Corpse => "A fresh corpse to raise or burst".to_owned(),
        AuraKind::Shapeshift => "In wolf form".to_owned(),
    }
}

/// One aura icon of a unit frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuraRow {
    pub ability: u32,
    pub polarity: AuraPolarity,
    /// The icon's tooltip: `Frost Nova: Rooted (2s)`.
    pub label: String,
    /// The corner timer: [`remaining_label`].
    pub timer: String,
}

/// A unit frame's auras: buffs shown above, debuffs below, each longest remaining first, then by
/// ability ID so equal times keep their order between projections.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuraRows {
    pub buffs: Vec<AuraRow>,
    pub debuffs: Vec<AuraRow>,
}

/// The rows a unit frame shows for `auras`.
#[must_use]
pub fn aura_rows(auras: &[AuraView]) -> AuraRows {
    let mut ordered = auras.to_vec();
    ordered.sort_by(|left, right| {
        right
            .remaining
            .cmp(&left.remaining)
            .then(left.ability.get().cmp(&right.ability.get()))
    });
    let mut rows = AuraRows::default();
    for aura in ordered {
        let name = ability_by_id(aura.ability).map_or_else(
            || format!("ability {}", aura.ability.get()),
            |ability| ability.name.to_owned(),
        );
        let timer = remaining_label(aura.remaining);
        let polarity = aura_polarity(aura.kind);
        let row = AuraRow {
            ability: aura.ability.get(),
            polarity,
            label: format!("{name}: {} ({timer})", aura_effect_text(aura)),
            timer,
        };
        match polarity {
            AuraPolarity::Buff => rows.buffs.push(row),
            AuraPolarity::Debuff => rows.debuffs.push(row),
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::GLOBAL_COOLDOWN_TICKS;
    use mmorpg_core::{EntityFlags, EntityKind, EntitySnapshot, ResourceKind};

    const FIREBOLT: u32 = 9;
    const FROST_NOVA: u32 = 10;
    const BLIZZARD: u32 = 12;
    const STRIKE: u32 = 1;

    fn ability(id: u32) -> &'static Ability {
        ability_by_id(AbilityId::new(id)).unwrap()
    }

    #[test]
    fn range_labels_follow_the_catalog_ranges() {
        // Renew, Divine Shield and Rejuvenation land on a selected friend within 30 m (#1013).
        for id in [35, 37, 40] {
            assert_eq!(
                range_label(ability(id).range),
                "30 m range, friendly or self",
                "ability {id}"
            );
        }
        assert_eq!(range_label(ability(FIREBOLT).range), "30 m range");
        assert_eq!(range_label(ability(STRIKE).range), "Melee range");
        assert_eq!(range_label(ability(FROST_NOVA).range), "Self");
        for ability in ABILITY_CATALOG {
            let label = range_label(ability.range);
            match ability.range.reach() {
                Some(_) if ability.range == AbilityRange::Melee => assert_eq!(label, "Melee range"),
                Some(units) => assert!(
                    label.starts_with(&format!("{} m range", f64::from(units) / 100.)),
                    "{}: {label}",
                    ability.name
                ),
                None => assert_eq!(label, "Self", "{}", ability.name),
            }
        }
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
        assert_eq!(class_kit(PlayerClass::Warden), [1, 2, 3, 4, 17, 51]);
        assert_eq!(class_kit(PlayerClass::Ranger), [5, 6, 7, 8, 19, 20, 52]);
        assert_eq!(
            class_kit(PlayerClass::Arcanist),
            [9, 10, 11, 12, 16, 21, 22, 23, 49, 50, 53]
        );
        assert_eq!(slot_key_label(0), "1");
        assert_eq!(slot_key_label(9), "0");
        assert_eq!(slot_key_label(10), "Shift+1");
        assert_eq!(slot_key_label(19), "Shift+0");
        assert_eq!(
            class_kit(PlayerClass::Necromancer),
            [26, 27, 28, 29, 45, 54, 55]
        );
        assert_eq!(class_kit(PlayerClass::Rogue), [30, 31, 32, 33, 42, 43, 44]);
        assert_eq!(class_kit(PlayerClass::Cleric), [34, 35, 36, 37, 56]);
        assert_eq!(class_kit(PlayerClass::Druid), [38, 39, 40, 41, 46, 58, 59]);
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
            slot_cooldown(nova, &running, [40, GLOBAL_COOLDOWN_TICKS]),
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
        assert_eq!(
            slot_cooldown(ability(FIREBOLT), &running, [40, GLOBAL_COOLDOWN_TICKS]),
            gcd
        );
        let short = [Cooldown {
            ability: nova.id,
            remaining: 10,
        }];
        assert_eq!(
            slot_cooldown(nova, &short, [40, GLOBAL_COOLDOWN_TICKS]),
            gcd
        );
        assert_eq!(
            slot_cooldown(nova, &short, [10, GLOBAL_COOLDOWN_TICKS])
                .unwrap()
                .kind,
            CooldownKind::Ability
        );
        assert_eq!(slot_cooldown(nova, &[], [0, GLOBAL_COOLDOWN_TICKS]), None);
    }

    #[test]
    fn a_hasted_global_cooldown_sweeps_over_its_own_length() {
        // Rapid Fire shortens a shot's global cooldown (#884); the sweep spans that, not 1.5 s.
        let hasted = GLOBAL_COOLDOWN_TICKS * 100 / 160;
        let sweep = slot_cooldown(ability(FIREBOLT), &[], [hasted / 2, hasted]).unwrap();
        assert_eq!((sweep.remaining, sweep.total), (hasted / 2, hasted));
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

    #[test]
    fn a_slot_counts_the_selection_only_inside_the_aim_cone() {
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
        let firebolt = ability(FIREBOLT);
        let mut snapshot = crate::test_support::snapshot(7, 1);
        snapshot.viewer.target = Some(EntityRef::new(EntityKind::Creature, 3));
        // Facing +Z: a wolf straight ahead is aimed at, one 45° off is not (the zone aims at
        // the unit in front instead), though the swing arc still holds it.
        snapshot.entities = vec![
            entity(EntityKind::Player, 7, 0, 0),
            entity(EntityKind::Creature, 3, 0, 5_000),
        ];
        assert!(aims_at_selection(&snapshot, firebolt));
        snapshot.entities[1].position = [3_000, 90, 3_000];
        assert!(!aims_at_selection(&snapshot, firebolt));
        assert!(aims_at_selection(&snapshot, ability(STRIKE)));
        snapshot.entities[1].position = [-3_000, 90, -3_000];
        assert!(!aims_at_selection(&snapshot, ability(STRIKE)));
        // An unseen selection stays beyond every reach.
        snapshot.viewer.target = Some(EntityRef::new(EntityKind::Creature, 999));
        assert!(aims_at_selection(&snapshot, firebolt));
        snapshot.viewer.target = None;
        assert!(!aims_at_selection(&snapshot, firebolt));
    }

    /// Ticks for `thirtieths` of a second, as the browser's former tests counted them.
    fn t(thirtieths: u16) -> u16 {
        u16::try_from(u32::from(thirtieths) * u32::from(TICK_HZ) / 30).unwrap()
    }

    #[test]
    fn cooldown_seconds_show_from_two_seconds_and_never_for_the_global_wait() {
        let cooldown = |kind, remaining| SlotCooldown {
            kind,
            remaining,
            total: t(900),
        };
        assert_eq!(cooldown(CooldownKind::Ability, t(59)).shown_seconds(), None);
        assert_eq!(
            cooldown(CooldownKind::Ability, t(60)).shown_seconds(),
            Some(2)
        );
        assert_eq!(
            cooldown(CooldownKind::Ability, t(61)).shown_seconds(),
            Some(3)
        );
        assert_eq!(
            cooldown(CooldownKind::Ability, t(360)).shown_seconds(),
            Some(12)
        );
        assert_eq!(cooldown(CooldownKind::Global, t(90)).shown_seconds(), None);
    }

    #[test]
    fn remaining_time_reads_as_seconds_then_minutes() {
        assert_eq!(remaining_label(t(1)), "1s");
        assert_eq!(remaining_label(t(31)), "2s");
        assert_eq!(remaining_label(t(300)), "10s");
        assert_eq!(remaining_label(t(1_800)), "1m");
        assert_eq!(remaining_label(t(2_000)), "2m");
    }

    #[test]
    fn auras_split_by_polarity_longest_first_with_named_effects() {
        let aura = |ability, kind, remaining, amount| AuraView {
            ability: AbilityId::new(ability),
            kind,
            remaining,
            amount,
        };
        let rows = aura_rows(&[
            aura(10, AuraKind::Root, 100, 0),
            aura(11, AuraKind::Absorb, 200, 40),
            aura(3, AuraKind::HealOverTime, 200, 30),
            aura(6, AuraKind::DamageOverTime, 400, 30),
            aura(8, AuraKind::Haste, 50, 40),
        ]);
        let ids = |rows: &[AuraRow]| rows.iter().map(|row| row.ability).collect::<Vec<_>>();
        assert_eq!(ids(&rows.buffs), [3, 11, 8]);
        assert_eq!(ids(&rows.debuffs), [6, 10]);
        let label = |aura| {
            aura_rows(&[aura])
                .buffs
                .into_iter()
                .chain(aura_rows(&[aura]).debuffs)
                .next()
                .unwrap()
        };
        assert_eq!(
            label(aura(11, AuraKind::Absorb, t(600), 60)).label,
            "Arcane Barrier: Absorbs 60 damage (20s)"
        );
        assert_eq!(
            label(aura(10, AuraKind::Root, t(31), 0)).label,
            "Frost Nova: Rooted (2s)"
        );
        assert_eq!(
            label(aura(2, AuraKind::Snare, t(30), 50)).label,
            "Shield Bash: Slowed by 50% (1s)"
        );
        assert_eq!(label(aura(11, AuraKind::Absorb, t(1_800), 0)).timer, "1m");
    }
}
