//! Class choice, ability use, casts and channels, creature abilities and
//! aura pulses.
//!
//! - **Use** (tick step 1): validation runs in a fixed order (class,
//!   learned, alive, stunned, casting, global cooldown, cooldown, resource,
//!   target, range) and every refusal is an `Error` event. Instants pay and
//!   resolve at once, casts pay when they complete, channels pay at the start.
//! - **Casts** (step 6): global cooldowns, cooldowns and casts advance for
//!   players, then creatures; channels pulse; completed casts resolve.
//! - **Auras** (step 6): damage and heal over time pulse, and every aura
//!   counts down, in `(EntityRef, aura slot)` order.
//! - **Creature abilities** (step 2): an engaged creature uses its
//!   content-bound ability once its timer reaches zero; the timer restarts at
//!   the cooldown plus a zone-RNG jitter of 0–30 ticks.
//!
//! Damage adds equal threat; healing adds half the effective amount to every
//! creature engaged with the healed player.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ability::{
    Ability, AbilityEffect, AbilityId, AuraKind, CREATURE_ABILITY_JITTER_TICKS, CastTime,
    GLOBAL_COOLDOWN_TICKS, ability_by_id, learned, scaled,
};
use crate::ai::distance_squared;
use crate::aura::{apply_aura, has_kind, pulse_amount};
use crate::snapshot::CreatureAi;
use crate::unit::{COMBAT_LINGER_TICKS, percent_of};
use crate::{
    Aura, CastState, ClassChoice, Cooldown, CreatureId, EntityRef, ErrorCode, PlayerClass,
    PlayerId, Sex, ZoneError, ZoneEvent, ZoneSimulation,
};

/// A Shield Bash on a casting creature locks the interrupted ability this long.
pub(crate) const SHIELD_BASH_LOCKOUT_TICKS: u16 = 120;

/// A refusal: the error code and the unit it concerned.
pub(crate) type Refusal = Option<(ErrorCode, Option<EntityRef>)>;

impl ZoneSimulation {
    /// A one-time class choice; the resource starts at the class's start value.
    /// Content that never bound the ability catalog keeps it out of its
    /// fingerprint, so its zones refuse classes (and with them abilities).
    pub(crate) fn choose_class(&mut self, player_id: PlayerId, class: u8, sex: u8) -> Refusal {
        let catalog_bound = self.content.ability_revision() != 0;
        let player = self.players.get_mut(&player_id)?;
        if !catalog_bound {
            return Some((ErrorCode::InvalidClass, None));
        }
        match (
            player.class,
            PlayerClass::from_code(class),
            Sex::from_code(sex),
        ) {
            (None, Some(class), Some(sex)) => {
                player.class = Some(ClassChoice { class, sex });
                player.resource.value = class.resource().start(player.level);
                None
            }
            _ => Some((ErrorCode::InvalidClass, None)),
        }
    }

    /// Validates and starts or resolves a class ability.
    pub(crate) fn use_ability(
        &mut self,
        player_id: PlayerId,
        ability: u8,
        target: Option<EntityRef>,
        now: u64,
    ) -> Result<Refusal, ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(None);
        };
        let Some(choice) = player.class else {
            return Ok(Some((ErrorCode::NoClass, None)));
        };
        let Some(ability) = learned(choice.class, player.level, AbilityId::new(ability)) else {
            return Ok(Some((ErrorCode::NotLearned, None)));
        };
        if !player.is_alive() {
            return Ok(Some((ErrorCode::YouAreDead, None)));
        }
        if has_kind(&player.auras, AuraKind::Stun) {
            return Ok(Some((ErrorCode::Stunned, None)));
        }
        if player.cast.is_some() {
            return Ok(Some((ErrorCode::AlreadyCasting, None)));
        }
        if player.global_cooldown > 0
            || player
                .cooldowns
                .iter()
                .any(|cooldown| cooldown.ability == ability.id)
        {
            return Ok(Some((ErrorCode::NotReady, None)));
        }
        if player.resource.value < ability.cost {
            return Ok(Some((ErrorCode::NotEnoughResource, None)));
        }
        let target = if ability.needs_target() {
            let target = target.or(player.target);
            let Some(entity @ EntityRef::Creature(creature_id)) = target else {
                return Ok(Some((ErrorCode::InvalidTarget, target)));
            };
            let alive = self
                .creatures
                .get(&creature_id)
                .is_some_and(|creature| creature.is_alive());
            if !alive || !self.visible_to(player_id, entity)? {
                return Ok(Some((ErrorCode::InvalidTarget, target)));
            }
            let reach = ability.range.reach().unwrap_or(0);
            if self.unit_distance_squared(EntityRef::Player(player_id), entity)?
                > i64::from(reach).pow(2)
            {
                return Ok(Some((ErrorCode::OutOfRange, target)));
            }
            Some(entity)
        } else {
            None
        };
        let source = EntityRef::Player(player_id);
        self.update_player(player_id, |player| {
            player.global_cooldown = GLOBAL_COOLDOWN_TICKS;
        });
        match ability.cast {
            CastTime::Instant => {
                self.pay_and_cool_down(player_id, ability);
                self.resolve_player_ability(player_id, ability, target, now)?;
            }
            CastTime::Cast(ticks) | CastTime::Channel(ticks) => {
                let point = if ability.cast.is_channel() {
                    self.pay_and_cool_down(player_id, ability);
                    let entity =
                        target.ok_or_else(|| ZoneError::new("a channel needs a target"))?;
                    let position = self.unit_position(entity)?;
                    Some([position[0], position[2]])
                } else {
                    None
                };
                self.update_player(player_id, |player| {
                    player.cast = Some(CastState {
                        ability: ability.id,
                        elapsed: 0,
                        target,
                        point,
                    });
                });
                self.notify_units(
                    [Some(source), target],
                    ZoneEvent::CastStarted {
                        source,
                        target,
                        ability: ability.id,
                        ticks,
                    },
                );
            }
        }
        Ok(None)
    }

    /// Stops the player's own cast or channel; nothing happens without one.
    pub(crate) fn cancel_cast(&mut self, player_id: PlayerId) {
        self.interrupt_cast(EntityRef::Player(player_id), None, None);
    }

    /// Ends `unit`'s cast or channel early and reports it. A creature's
    /// interrupted ability waits at least `lockout` ticks.
    pub(crate) fn interrupt_cast(
        &mut self,
        unit: EntityRef,
        by: Option<EntityRef>,
        lockout: Option<u16>,
    ) -> bool {
        let cast = match unit {
            EntityRef::Player(player_id) => self
                .players
                .get_mut(&player_id)
                .and_then(|player| player.cast.take()),
            EntityRef::Creature(creature_id) => {
                self.creatures.get_mut(&creature_id).and_then(|creature| {
                    let cast = creature.cast.take();
                    if cast.is_some()
                        && let Some(lockout) = lockout
                    {
                        creature.ability_timer = creature.ability_timer.max(lockout);
                    }
                    cast
                })
            }
            EntityRef::Npc(_) => None,
        };
        let Some(cast) = cast else {
            return false;
        };
        self.notify_units(
            [by, Some(unit)],
            ZoneEvent::Interrupted {
                source: by,
                target: unit,
                ability: cast.ability,
            },
        );
        true
    }

    fn pay_and_cool_down(&mut self, player_id: PlayerId, ability: &Ability) {
        self.update_player(player_id, |player| {
            if let Some(choice) = player.class {
                player.resource.spend(choice.class.resource(), ability.cost);
            }
            if ability.cooldown > 0 {
                let cooldown = Cooldown {
                    ability: ability.id,
                    remaining: ability.cooldown,
                };
                match player
                    .cooldowns
                    .binary_search_by_key(&ability.id, |cooldown| cooldown.ability)
                {
                    Ok(index) => player.cooldowns[index] = cooldown,
                    Err(index) => player.cooldowns.insert(index, cooldown),
                }
            }
        });
    }

    /// The effect of a player's instant or completed cast.
    fn resolve_player_ability(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        target: Option<EntityRef>,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        let (level, max_health) = (player.level, player.max_health());
        let (melee, bonus) = (player.melee_damage(), u32::from(player.damage_bonus()));
        let source = EntityRef::Player(player_id);
        self.notify_units(
            [Some(source), target],
            ZoneEvent::AbilityUsed {
                source,
                target,
                ability: ability.id,
            },
        );
        let creature = match target {
            Some(EntityRef::Creature(creature_id)) => Some(creature_id),
            _ => None,
        };
        let aura = |amount: u16| {
            ability.aura().map(|spec| Aura {
                ability: ability.id,
                caster: source,
                remaining: spec.duration,
                amount,
            })
        };
        match ability.effect {
            AbilityEffect::WeaponStrike {
                numerator,
                denominator,
                bonus,
            } => {
                if let Some(creature_id) = creature {
                    let weapon = self.weapon_roll(melee);
                    let amount = weapon * u32::from(numerator) / u32::from(denominator.max(1))
                        + u32::from(bonus);
                    self.player_damages_creature(player_id, creature_id, amount, false, now)?;
                }
            }
            AbilityEffect::Bash { lockout_ticks, .. } => {
                if let Some(creature_id) = creature {
                    self.interrupt_cast(
                        EntityRef::Creature(creature_id),
                        Some(source),
                        Some(lockout_ticks),
                    );
                    if let Some(stun) = aura(0) {
                        self.hostile_aura(player_id, creature_id, stun)?;
                    }
                }
            }
            AbilityEffect::HealOverTime { percent, .. } => {
                let total = percent_of(max_health, u32::from(percent));
                if let Some(aura) = aura(u16::try_from(total).unwrap_or(u16::MAX)) {
                    self.put_aura(source, aura);
                }
            }
            AbilityEffect::Cleave { extra, radius } => {
                if let Some(creature_id) = creature {
                    let centre = self.unit_position(EntityRef::Creature(creature_id))?;
                    let mut victims = vec![creature_id];
                    victims.extend(
                        self.living_creatures_near([centre[0], centre[2]], radius)?
                            .into_iter()
                            .filter(|&other| other != creature_id)
                            .take(usize::from(extra)),
                    );
                    for victim in victims {
                        let amount = self.weapon_roll(melee);
                        self.player_damages_creature(player_id, victim, amount, false, now)?;
                    }
                }
            }
            AbilityEffect::DamageOverTime {
                base, per_level, ..
            } => {
                if let (Some(creature_id), Some(aura)) =
                    (creature, aura(scaled(base, per_level, level)))
                {
                    self.hostile_aura(player_id, creature_id, aura)?;
                }
            }
            AbilityEffect::Snare { percent, .. } => {
                if let (Some(creature_id), Some(aura)) = (creature, aura(percent)) {
                    self.hostile_aura(player_id, creature_id, aura)?;
                }
            }
            AbilityEffect::Haste { percent, .. } => {
                if let Some(aura) = aura(percent) {
                    self.put_aura(source, aura);
                }
            }
            AbilityEffect::Bolt { damage, per_level } => {
                if let Some(creature_id) = creature {
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + u32::from(scaled(0, per_level, level))
                        + bonus;
                    self.player_damages_creature(player_id, creature_id, amount, false, now)?;
                }
            }
            AbilityEffect::Nova { damage, radius, .. } => {
                let centre = self.player_position(player_id)?;
                for victim in self.living_creatures_near([centre.x, centre.z], radius)? {
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + bonus;
                    self.player_damages_creature(player_id, victim, amount, false, now)?;
                    if let Some(root) = aura(0) {
                        self.hostile_aura(player_id, victim, root)?;
                    }
                }
            }
            AbilityEffect::Absorb {
                base, per_level, ..
            } => {
                if let Some(aura) = aura(scaled(base, per_level, level)) {
                    self.put_aura(source, aura);
                }
            }
            // Blizzard pulses while it channels; bandages are creature-only.
            AbilityEffect::Blizzard { .. } | AbilityEffect::Bandage { .. } => {}
        }
        Ok(())
    }

    /// Weapon damage: one uniform draw in the player's melee range.
    fn weapon_roll(&mut self, [low, high]: [u16; 2]) -> u32 {
        self.rng.inclusive(u32::from(low), u32::from(high))
    }

    /// Living creatures whose bodies lie within `radius` of `centre`, in
    /// `EntityRef` order.
    fn living_creatures_near(
        &self,
        centre: [i32; 2],
        radius: i32,
    ) -> Result<Vec<CreatureId>, ZoneError> {
        let mut found = Vec::new();
        for entity in self.interest.near(centre[0], centre[1], radius) {
            let EntityRef::Creature(creature_id) = entity else {
                continue;
            };
            if !self
                .creatures
                .get(&creature_id)
                .is_some_and(|creature| creature.is_alive())
            {
                continue;
            }
            let position = self.unit_position(entity)?;
            if distance_squared(centre, [position[0], position[2]]) <= i64::from(radius).pow(2) {
                found.push(creature_id);
            }
        }
        Ok(found)
    }

    /// Puts a player's aura on a creature: evading creatures ignore it,
    /// others engage the caster.
    fn hostile_aura(
        &mut self,
        player_id: PlayerId,
        creature_id: CreatureId,
        aura: Aura,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        let target = EntityRef::Creature(creature_id);
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(());
        };
        if !creature.is_alive() {
            return Ok(());
        }
        if matches!(creature.ai, CreatureAi::Evading { .. }) {
            self.notify(player_id, ZoneEvent::Evade { source, target });
            return Ok(());
        }
        if self
            .ability_kind(aura.ability)
            .is_some_and(|kind| kind == AuraKind::Stun)
        {
            self.interrupt_cast(target, Some(source), None);
        }
        self.put_aura(target, aura);
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.combat_timer = COMBAT_LINGER_TICKS;
        }
        self.update_player(player_id, |player| {
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        self.engage(creature_id, source, true)
    }

    fn ability_kind(&self, ability: AbilityId) -> Option<AuraKind> {
        ability_by_id(ability)?.aura().map(|spec| spec.kind)
    }

    /// Adds or refreshes an aura on a unit and reports it; an aura it
    /// evicted from a full unit is reported removed.
    pub(crate) fn put_aura(&mut self, unit: EntityRef, aura: Aura) {
        let evicted = match unit {
            EntityRef::Player(player_id) => self
                .players
                .get_mut(&player_id)
                .and_then(|player| apply_aura(&mut player.auras, aura)),
            EntityRef::Creature(creature_id) => self
                .creatures
                .get_mut(&creature_id)
                .and_then(|creature| apply_aura(&mut creature.auras, aura)),
            EntityRef::Npc(_) => return,
        };
        if let Some(evicted) = evicted {
            self.notify_units(
                [Some(evicted.caster), Some(unit)],
                ZoneEvent::AuraRemoved {
                    source: evicted.caster,
                    target: unit,
                    ability: evicted.ability,
                },
            );
        }
        self.notify_units(
            [Some(aura.caster), Some(unit)],
            ZoneEvent::AuraApplied {
                source: aura.caster,
                target: unit,
                ability: aura.ability,
                ticks: aura.remaining,
            },
        );
    }

    /// Removes every aura `caster` put on any unit, without events: its
    /// caster died or left.
    pub(crate) fn forget_caster(&mut self, caster: EntityRef) {
        for player in self.players.values_mut() {
            player.auras.retain(|aura| aura.caster != caster);
        }
        for creature in self.creatures.values_mut() {
            creature.auras.retain(|aura| aura.caster != caster);
        }
    }

    /// Reports `event` to the players among `units`; when there are none,
    /// to the players engaged with the creatures among them.
    pub(crate) fn notify_units(&mut self, units: [Option<EntityRef>; 2], event: ZoneEvent) {
        let mut recipients: BTreeSet<PlayerId> = units
            .iter()
            .flatten()
            .filter_map(|unit| match unit {
                EntityRef::Player(player_id) => Some(*player_id),
                _ => None,
            })
            .collect();
        if recipients.is_empty() {
            for unit in units.iter().flatten() {
                if let EntityRef::Creature(creature_id) = unit
                    && let Some(creature) = self.creatures.get(creature_id)
                {
                    recipients.extend(creature.threat.iter().filter_map(
                        |entry| match entry.entity {
                            EntityRef::Player(player_id) => Some(player_id),
                            _ => None,
                        },
                    ));
                }
            }
        }
        for player_id in recipients {
            self.notify(player_id, event);
        }
    }

    /// The body (or corpse) centre of a unit.
    pub(crate) fn unit_position(&self, unit: EntityRef) -> Result<[i32; 3], ZoneError> {
        match unit {
            EntityRef::Player(player_id) => {
                let position = self.player_position(player_id)?;
                Ok([position.x, position.y, position.z])
            }
            EntityRef::Creature(creature_id) => {
                let creature = self
                    .creatures
                    .get(&creature_id)
                    .ok_or_else(|| ZoneError::new("unknown creature"))?;
                self.creature_position(creature_id, creature)?
                    .ok_or_else(|| ZoneError::new("creature is despawned"))
            }
            EntityRef::Npc(npc_id) => self
                .content
                .npc(npc_id)
                .map(|npc| [npc.position[0], 0, npc.position[1]])
                .ok_or_else(|| ZoneError::new("unknown npc")),
        }
    }

    fn unit_distance_squared(&self, from: EntityRef, to: EntityRef) -> Result<i64, ZoneError> {
        let from = self.unit_position(from)?;
        let to = self.unit_position(to)?;
        Ok(distance_squared([from[0], from[2]], [to[0], to[2]]))
    }

    /// Step 6, first part: global cooldowns, cooldowns and casts of players,
    /// then creature ability timers and casts, in ID order.
    pub(crate) fn advance_casts(&mut self, now: u64) -> Result<(), ZoneError> {
        let players: Vec<_> = self.players.keys().copied().collect();
        for player_id in players {
            let Some(player) = self.players.get_mut(&player_id) else {
                continue;
            };
            player.global_cooldown = player.global_cooldown.saturating_sub(1);
            for cooldown in &mut player.cooldowns {
                cooldown.remaining = cooldown.remaining.saturating_sub(1);
            }
            player.cooldowns.retain(|cooldown| cooldown.remaining > 0);
            let Some(mut cast) = player.cast else {
                continue;
            };
            let ability = ability_by_id(cast.ability)
                .ok_or_else(|| ZoneError::new("cast names an unknown ability"))?;
            cast.elapsed += 1;
            let done = cast.elapsed >= ability.cast.ticks();
            player.cast = (!done).then_some(cast);
            let level = player.level;
            match (ability.effect, cast.point) {
                (
                    AbilityEffect::Blizzard {
                        damage,
                        per_level,
                        radius,
                        period,
                    },
                    Some(point),
                ) => {
                    if period > 0 && cast.elapsed % period == 0 {
                        for victim in self.living_creatures_near(point, radius)? {
                            let amount = self
                                .rng
                                .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                                + u32::from(scaled(0, per_level, level));
                            self.player_damages_creature(player_id, victim, amount, false, now)?;
                        }
                    }
                }
                _ if done => self.complete_player_cast(player_id, ability, cast, now)?,
                _ => {}
            }
        }
        let creatures: Vec<_> = self.creatures.keys().copied().collect();
        for creature_id in creatures {
            self.advance_creature_cast(creature_id)?;
        }
        Ok(())
    }

    fn complete_player_cast(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        cast: CastState,
        now: u64,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        let target_alive = match cast.target {
            Some(EntityRef::Creature(creature_id)) => self
                .creatures
                .get(&creature_id)
                .is_some_and(|creature| creature.is_alive()),
            Some(_) => false,
            None => true,
        };
        if !target_alive {
            self.notify_units(
                [Some(source), None],
                ZoneEvent::Interrupted {
                    source: None,
                    target: source,
                    ability: ability.id,
                },
            );
            return Ok(());
        }
        let affordable = self
            .players
            .get(&player_id)
            .is_some_and(|player| player.resource.value >= ability.cost);
        if !affordable {
            self.notify(
                player_id,
                ZoneEvent::Error {
                    code: ErrorCode::NotEnoughResource,
                    target: cast.target,
                },
            );
            return Ok(());
        }
        self.pay_and_cool_down(player_id, ability);
        self.resolve_player_ability(player_id, ability, cast.target, now)
    }

    fn advance_creature_cast(&mut self, creature_id: CreatureId) -> Result<(), ZoneError> {
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        if !creature.is_alive() {
            return Ok(());
        }
        let Some(mut cast) = creature.cast else {
            creature.ability_timer = creature.ability_timer.saturating_sub(1);
            return Ok(());
        };
        let ability = ability_by_id(cast.ability)
            .ok_or_else(|| ZoneError::new("cast names an unknown ability"))?;
        cast.elapsed += 1;
        if cast.elapsed < ability.cast.ticks() {
            creature.cast = Some(cast);
            return Ok(());
        }
        creature.cast = None;
        let source = EntityRef::Creature(creature_id);
        let level = creature.level;
        match ability.effect {
            AbilityEffect::Bolt { damage, per_level } => {
                let Some(target @ EntityRef::Player(player_id)) = cast.target else {
                    return Ok(());
                };
                if !self
                    .players
                    .get(&player_id)
                    .is_some_and(|player| player.is_alive())
                {
                    return Ok(());
                }
                self.restart_creature_ability(creature_id, ability);
                self.notify_units(
                    [Some(source), Some(target)],
                    ZoneEvent::AbilityUsed {
                        source,
                        target: Some(target),
                        ability: ability.id,
                    },
                );
                let amount = self
                    .rng
                    .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                    + u32::from(scaled(0, per_level, level));
                self.creature_damages_player(creature_id, player_id, amount, false)?;
            }
            AbilityEffect::Bandage { percent, .. } => {
                self.restart_creature_ability(creature_id, ability);
                self.notify_units(
                    [Some(source), None],
                    ZoneEvent::AbilityUsed {
                        source,
                        target: None,
                        ability: ability.id,
                    },
                );
                let content = Arc::clone(&self.content);
                let (_, template) = Self::creature_content(&content, creature_id)?;
                let Some(creature) = self.creatures.get_mut(&creature_id) else {
                    return Ok(());
                };
                let max_health = template.max_health(creature.level);
                let healed = percent_of(max_health, u32::from(percent))
                    .min(max_health.saturating_sub(creature.health));
                creature.health += healed;
                self.notify_units(
                    [Some(source), None],
                    ZoneEvent::Healed {
                        source,
                        target: source,
                        ability: ability.id,
                        amount: u16::try_from(healed).unwrap_or(u16::MAX),
                    },
                );
            }
            _ => {}
        }
        Ok(())
    }

    /// The creature's ability waits its cooldown plus a 0–30 tick jitter.
    fn restart_creature_ability(&mut self, creature_id: CreatureId, ability: &Ability) {
        let jitter = self
            .rng
            .inclusive(0, u32::from(CREATURE_ABILITY_JITTER_TICKS));
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.ability_timer = ability
                .cooldown
                .saturating_add(u16::try_from(jitter).unwrap_or(0));
        }
    }

    /// A creature engaging from idle waits a 0–30 tick jitter before its
    /// first ability use.
    pub(crate) fn arm_creature_ability(
        &mut self,
        creature_id: CreatureId,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, _) = Self::creature_content(&content, creature_id)?;
        if content.creature_ability(spawn.template).is_none() {
            return Ok(());
        }
        let jitter = self
            .rng
            .inclusive(0, u32::from(CREATURE_ABILITY_JITTER_TICKS));
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.ability_timer = u16::try_from(jitter).unwrap_or(0);
        }
        Ok(())
    }

    /// Step 2 for an engaged creature with a ready ability: starts its cast
    /// when the threat leader is in range (Muck Bolt) or its own health is
    /// low enough (Crude Bandage). Returns whether it is casting.
    pub(crate) fn try_creature_ability(
        &mut self,
        creature_id: CreatureId,
        leader: EntityRef,
        here: [i32; 2],
        leader_at: [i32; 2],
    ) -> Result<bool, ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, template) = Self::creature_content(&content, creature_id)?;
        let Some(ability) = content
            .creature_ability(spawn.template)
            .and_then(ability_by_id)
        else {
            return Ok(false);
        };
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(false);
        };
        if creature.cast.is_some() {
            return Ok(true);
        }
        if creature.ability_timer > 0 || has_kind(&creature.auras, AuraKind::Stun) {
            return Ok(false);
        }
        let target = match ability.effect {
            AbilityEffect::Bandage { below_percent, .. } => {
                let max_health = u64::from(template.max_health(creature.level));
                if u64::from(creature.health) * 100 >= max_health * u64::from(below_percent) {
                    return Ok(false);
                }
                None
            }
            _ => {
                let reach = ability.range.reach().unwrap_or(0);
                if distance_squared(here, leader_at) > i64::from(reach).pow(2) {
                    return Ok(false);
                }
                Some(leader)
            }
        };
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.cast = Some(CastState {
                ability: ability.id,
                elapsed: 0,
                target,
                point: None,
            });
        }
        let source = EntityRef::Creature(creature_id);
        self.notify_units(
            [Some(source), target],
            ZoneEvent::CastStarted {
                source,
                target,
                ability: ability.id,
                ticks: ability.cast.ticks(),
            },
        );
        Ok(true)
    }

    /// Step 6, second part: every aura counts down in `(EntityRef, slot)`
    /// order; damage and heal over time pulse every period, and expired auras
    /// are removed.
    pub(crate) fn advance_auras(&mut self, now: u64) -> Result<(), ZoneError> {
        let units: Vec<EntityRef> = self
            .players
            .iter()
            .filter(|(_, player)| !player.auras.is_empty())
            .map(|(&id, _)| EntityRef::Player(id))
            .chain(
                self.creatures
                    .iter()
                    .filter(|(_, creature)| !creature.auras.is_empty())
                    .map(|(&id, _)| EntityRef::Creature(id)),
            )
            .collect();
        for unit in units {
            // Each aura present at the start of the pass counts down once, in
            // slot order, even when a pulse removes others (a broken root, an
            // emptied shield, death): the pass follows identities, not indices.
            let keys: Vec<_> = self
                .unit_auras(unit)
                .map(|auras| {
                    auras
                        .iter()
                        .map(|aura| (aura.ability, aura.caster))
                        .collect()
                })
                .unwrap_or_default();
            for key in keys {
                let Some(auras) = self.unit_auras(unit) else {
                    break;
                };
                let Some(slot) = auras
                    .iter()
                    .position(|aura| (aura.ability, aura.caster) == key)
                else {
                    continue;
                };
                let current = &mut auras[slot];
                current.remaining = current.remaining.saturating_sub(1);
                let aura = *current;
                let spec = aura
                    .spec()
                    .ok_or_else(|| ZoneError::new("aura names an ability without an aura"))?;
                if aura.remaining == 0 {
                    auras.remove(slot);
                }
                let elapsed = spec.duration - aura.remaining;
                if spec.period > 0 && elapsed % spec.period == 0 {
                    let amount = pulse_amount(
                        aura.amount,
                        elapsed / spec.period,
                        spec.duration / spec.period,
                    );
                    self.pulse(unit, aura, spec.kind, amount, now)?;
                }
                if aura.remaining == 0 {
                    self.notify_units(
                        [Some(aura.caster), Some(unit)],
                        ZoneEvent::AuraRemoved {
                            source: aura.caster,
                            target: unit,
                            ability: aura.ability,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn unit_auras(&mut self, unit: EntityRef) -> Option<&mut Vec<Aura>> {
        match unit {
            EntityRef::Player(player_id) => self
                .players
                .get_mut(&player_id)
                .map(|player| &mut player.auras),
            EntityRef::Creature(creature_id) => self
                .creatures
                .get_mut(&creature_id)
                .map(|creature| &mut creature.auras),
            EntityRef::Npc(_) => None,
        }
    }

    fn pulse(
        &mut self,
        unit: EntityRef,
        aura: Aura,
        kind: AuraKind,
        amount: u16,
        now: u64,
    ) -> Result<(), ZoneError> {
        match (kind, unit, aura.caster) {
            (
                AuraKind::DamageOverTime,
                EntityRef::Creature(creature_id),
                EntityRef::Player(player_id),
            ) => {
                self.player_damages_creature(player_id, creature_id, u32::from(amount), false, now)
            }
            (AuraKind::HealOverTime, EntityRef::Player(player_id), _) => {
                self.heal_player(player_id, aura.caster, aura.ability, u32::from(amount));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Heals a living player; half the effective amount becomes the healer's
    /// threat on every creature engaged with the healed player.
    pub(crate) fn heal_player(
        &mut self,
        player_id: PlayerId,
        healer: EntityRef,
        ability: AbilityId,
        amount: u32,
    ) {
        let Some(player) = self.players.get_mut(&player_id) else {
            return;
        };
        if !player.is_alive() {
            return;
        }
        let healed = amount.min(player.max_health().saturating_sub(player.health));
        if healed == 0 {
            return;
        }
        player.health += healed;
        let healed_unit = EntityRef::Player(player_id);
        for creature in self.creatures.values_mut() {
            if creature.is_alive()
                && creature.ai == CreatureAi::Engaged
                && creature
                    .threat
                    .iter()
                    .any(|entry| entry.entity == healed_unit)
            {
                creature.add_threat(healer, healed / 2);
            }
        }
        self.notify_units(
            [Some(healer), Some(healed_unit)],
            ZoneEvent::Healed {
                source: healer,
                target: healed_unit,
                ability,
                amount: u16::try_from(healed).unwrap_or(u16::MAX),
            },
        );
    }

    /// Damage breaks a root once its unbreakable start has passed.
    pub(crate) fn break_roots(&mut self, unit: EntityRef) {
        let breakable = |aura: &Aura| {
            matches!(
                ability_by_id(aura.ability).map(|ability| ability.effect),
                Some(AbilityEffect::Nova {
                    unbreakable_ticks,
                    ..
                }) if aura.elapsed() >= unbreakable_ticks
            )
        };
        let auras = match unit {
            EntityRef::Player(player_id) => self
                .players
                .get_mut(&player_id)
                .map(|player| &mut player.auras),
            EntityRef::Creature(creature_id) => self
                .creatures
                .get_mut(&creature_id)
                .map(|creature| &mut creature.auras),
            EntityRef::Npc(_) => None,
        };
        let Some(auras) = auras else {
            return;
        };
        let broken: Vec<Aura> = auras.iter().copied().filter(breakable).collect();
        auras.retain(|aura| !breakable(aura));
        for aura in broken {
            self.notify_units(
                [Some(aura.caster), Some(unit)],
                ZoneEvent::AuraRemoved {
                    source: aura.caster,
                    target: unit,
                    ability: aura.ability,
                },
            );
        }
    }

    /// Absorb shields on a unit soak `amount` in slot order; an emptied
    /// shield is removed. Returns the damage left over.
    pub(crate) fn absorb(&mut self, unit: EntityRef, source: EntityRef, amount: u32) -> u32 {
        let auras = match unit {
            EntityRef::Player(player_id) => self
                .players
                .get_mut(&player_id)
                .map(|player| &mut player.auras),
            EntityRef::Creature(creature_id) => self
                .creatures
                .get_mut(&creature_id)
                .map(|creature| &mut creature.auras),
            EntityRef::Npc(_) => None,
        };
        let Some(auras) = auras else {
            return amount;
        };
        let mut left = amount;
        let mut absorbed = 0_u32;
        let mut emptied = Vec::new();
        for aura in auras.iter_mut() {
            if left == 0 {
                break;
            }
            if aura.kind() != Some(AuraKind::Absorb) {
                continue;
            }
            let soaked = left.min(u32::from(aura.amount));
            aura.amount -= u16::try_from(soaked).unwrap_or(aura.amount);
            left -= soaked;
            absorbed += soaked;
            if aura.amount == 0 {
                emptied.push(*aura);
            }
        }
        auras.retain(|aura| aura.kind() != Some(AuraKind::Absorb) || aura.amount > 0);
        if absorbed > 0 {
            self.notify_units(
                [Some(source), Some(unit)],
                ZoneEvent::Absorbed {
                    source,
                    target: unit,
                    amount: u16::try_from(absorbed).unwrap_or(u16::MAX),
                },
            );
        }
        for aura in emptied {
            self.notify_units(
                [Some(aura.caster), Some(unit)],
                ZoneEvent::AuraRemoved {
                    source: aura.caster,
                    target: unit,
                    ability: aura.ability,
                },
            );
        }
        left
    }
}
