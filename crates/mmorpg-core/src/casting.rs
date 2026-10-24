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
    Ability, AbilityEffect, AbilityId, AbilityRange, AuraKind, CREATURE_ABILITY_JITTER_TICKS,
    CastTime, GLOBAL_COOLDOWN_TICKS, ability_by_id, learned, scaled,
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

/// One explosion: Fire Blast (#287) or a breaking explosive barrel (#288).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Detonation {
    /// The player credited with the damage to creatures and props.
    pub(crate) attacker: PlayerId,
    /// The exploding barrel, which also hurts players; `None` for a spell.
    pub(crate) source: Option<CreatureId>,
    pub(crate) centre: [i32; 3],
    pub(crate) radius: i32,
    pub(crate) damage: [u16; 2],
    pub(crate) bonus: u32,
    pub(crate) knockback: i32,
    /// The push direction for a unit standing on the centre.
    pub(crate) fallback: [i64; 2],
}

/// A large creature's stomp (#297).
#[derive(Clone, Copy, Debug)]
struct Stomp {
    damage: [u16; 2],
    bonus: u32,
    radius: i32,
    knockback: i32,
}

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
        // A Shield Bash in an open combo window follows up on the struck
        // creature and may ignore the slash's global cooldown (#157); any
        // other ability use ends a pending combo.
        let combo = self
            .players
            .get(&player_id)
            .and_then(|player| player.combo)
            .filter(|combo| combo.is_open() && ability == crate::ability::ids::SHIELD_BASH.get());
        self.update_player(player_id, |player| player.combo = None);
        let target = match combo {
            Some(combo) => Some(combo.target),
            None => target,
        };
        let ability = match self.ready_ability(player_id, ability, false, combo.is_some()) {
            None => return Ok(None),
            Some(Err(code)) => return Ok(Some((code, None))),
            Some(Ok(ability)) => ability,
        };
        let Some(player) = self.players.get(&player_id) else {
            return Ok(None);
        };
        let target = if ability.needs_target() {
            let target = target.or(player.target);
            // Aimed Shot without a target flies freely along the facing (#286); aimed specials
            // always do (#291).
            if (target.is_none() && ability.aims_freely()) || ability.shot() != crate::Shot::Plain {
                None
            } else {
                let Some(entity) = target else {
                    return Ok(Some((ErrorCode::InvalidTarget, target)));
                };
                let alive = match entity {
                    EntityRef::Creature(id) => {
                        self.creatures.get(&id).is_some_and(|unit| unit.is_alive())
                    }
                    EntityRef::Player(id) => {
                        self.players_are_hostile(player_id, id)
                            && self.players.get(&id).is_some_and(|unit| unit.is_alive())
                    }
                    EntityRef::Npc(_) => false,
                };
                if !alive || !self.visible_to(player_id, entity)? {
                    return Ok(Some((ErrorCode::InvalidTarget, target)));
                }
                let reach = ability.range.reach().unwrap_or(0);
                if self.unit_distance_squared(EntityRef::Player(player_id), entity)?
                    > i64::from(reach).pow(2)
                {
                    return Ok(Some((ErrorCode::OutOfRange, target)));
                }
                if (matches!(ability.range, AbilityRange::Units(_))
                    || matches!(entity, EntityRef::Player(_)))
                    && !self.line_of_sight(EntityRef::Player(player_id), entity)?
                {
                    return Ok(Some((ErrorCode::LineOfSight, target)));
                }
                // A charge needs a path as wide as the charger's body (#57).
                if matches!(ability.effect, AbilityEffect::Charge { .. })
                    && !self.charge_path_clear(EntityRef::Player(player_id), entity)?
                {
                    return Ok(Some((ErrorCode::LineOfSight, target)));
                }
                Some(entity)
            }
        } else if ability.is_friendly() {
            // A heal or shield lands on the selected friendly player, otherwise on the caster
            // (#303). Hostile and non-player selections fall back to the caster too.
            match target.or(player.target) {
                Some(EntityRef::Player(id))
                    if id != player_id && !self.players_are_hostile(player_id, id) =>
                {
                    let entity = EntityRef::Player(id);
                    let alive = self.players.get(&id).is_some_and(|unit| unit.is_alive());
                    if !alive
                        || self.combat_blocked(EntityRef::Player(player_id), entity)
                        || !self.visible_to(player_id, entity)?
                    {
                        return Ok(Some((ErrorCode::InvalidTarget, Some(entity))));
                    }
                    let reach = ability.range.reach().unwrap_or(0);
                    if self.unit_distance_squared(EntityRef::Player(player_id), entity)?
                        > i64::from(reach).pow(2)
                    {
                        return Ok(Some((ErrorCode::OutOfRange, Some(entity))));
                    }
                    if !self.line_of_sight(EntityRef::Player(player_id), entity)? {
                        return Ok(Some((ErrorCode::LineOfSight, Some(entity))));
                    }
                    Some(entity)
                }
                _ => None,
            }
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
                    // A beam's point is where it ends now; a ground channel's is its target's feet.
                    if let AbilityEffect::Beam { length, .. } = ability.effect {
                        Some(self.trace_beam(player_id, length)?.end)
                    } else {
                        let entity =
                            target.ok_or_else(|| ZoneError::new("a channel needs a target"))?;
                        let position = self.unit_position(entity)?;
                        Some([position[0], position[2]])
                    }
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

    /// The shared ability checks in their fixed order (class, learned, the
    /// directed mode when `directed`, alive, stunned, casting, global
    /// cooldown and cooldown, resource); `None` for an absent player.
    pub(crate) fn ready_ability(
        &self,
        player_id: PlayerId,
        ability: u8,
        directed: bool,
        waive_global_cooldown: bool,
    ) -> Option<Result<&'static Ability, ErrorCode>> {
        let player = self.players.get(&player_id)?;
        let Some(choice) = player.class else {
            return Some(Err(ErrorCode::NoClass));
        };
        let Some(ability) = learned(choice.class, player.level, AbilityId::new(ability)) else {
            return Some(Err(ErrorCode::NotLearned));
        };
        if directed && !crate::strike::has_directed_mode(ability.id) {
            return Some(Err(ErrorCode::NotLearned));
        }
        if !player.is_alive() {
            return Some(Err(ErrorCode::YouAreDead));
        }
        if has_kind(&player.auras, AuraKind::Stun) {
            return Some(Err(ErrorCode::Stunned));
        }
        if player.cast.is_some() {
            return Some(Err(ErrorCode::AlreadyCasting));
        }
        if (player.global_cooldown > 0 && !waive_global_cooldown)
            || player
                .cooldowns
                .iter()
                .any(|cooldown| cooldown.ability == ability.id)
        {
            return Some(Err(ErrorCode::NotReady));
        }
        if player.resource.value < ability.cost {
            return Some(Err(ErrorCode::NotEnoughResource));
        }
        Some(Ok(ability))
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

    pub(crate) fn pay_and_cool_down(&mut self, player_id: PlayerId, ability: &Ability) {
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
        let Some(facing) = self.players.get(&player_id).map(|player| player.facing) else {
            return Ok(());
        };
        let source = EntityRef::Player(player_id);
        self.notify_units(
            [Some(source), target],
            ZoneEvent::AbilityUsed {
                source,
                target,
                ability: ability.id,
            },
        );
        // A shot carries the effect: it lands when the projectile does (#286).
        if let Some(kind) = ability.projectile {
            return match target {
                Some(target) => self.shoot_at(source, target, kind, ability.id),
                None => self.shoot_free(source, facing, kind, ability.id),
            };
        }
        self.apply_player_effect(player_id, ability, target, now)
    }

    /// What a player's ability does to `target` (or around the caster) when it lands.
    pub(crate) fn apply_player_effect(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        target: Option<EntityRef>,
        now: u64,
    ) -> Result<(), ZoneError> {
        self.apply_player_effect_scaled(player_id, ability, target, 100, now)
    }

    /// [`Self::apply_player_effect`] with a weapon strike dealing `percent` of its damage (a
    /// piercing shot's later hits, #291).
    pub(crate) fn apply_player_effect_scaled(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        target: Option<EntityRef>,
        percent: u32,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        let (level, max_health) = (player.level, player.max_health());
        let (x, z) = crate::trig::direction(player.facing);
        // A unit standing exactly on a knockback's centre is pushed along the caster's facing.
        let fallback = [i64::from(x), i64::from(z)];
        let (melee, bonus) = (player.melee_damage(), u32::from(player.damage_bonus()));
        let source = EntityRef::Player(player_id);
        let aura = |amount: u16| {
            ability.aura().map(|spec| Aura {
                ability: ability.id,
                caster: source,
                remaining: spec.duration,
                amount,
            })
        };
        match ability.effect {
            AbilityEffect::WeaponStrike { .. } => {
                if let Some(victim) = target {
                    let amount =
                        self.weapon_strike_amount(player_id, ability.effect) * percent / 100;
                    let amount = self.stance_damage(player_id, target, amount)?;
                    self.player_melee_hits(player_id, victim, amount, false, now)?;
                }
            }
            AbilityEffect::Bash {
                lockout_ticks,
                knockback,
                ..
            } => {
                if let Some(victim) = target {
                    self.interrupt_cast(victim, Some(source), Some(lockout_ticks));
                    if let Some(stun) = aura(0) {
                        self.hostile_aura(player_id, victim, stun)?;
                    }
                    let from = self.player_position(player_id)?;
                    self.knock_away(victim, [from.x, from.z], fallback, knockback)?;
                }
            }
            AbilityEffect::HealOverTime { percent, .. } => {
                // A friendly heal (#303) lands on its target, scaled by the target's health.
                let (recipient, max_health) = match target {
                    Some(EntityRef::Player(id)) if ability.is_friendly() => (
                        EntityRef::Player(id),
                        self.players.get(&id).map_or(0, |unit| unit.max_health()),
                    ),
                    _ => (source, max_health),
                };
                let total = percent_of(max_health, u32::from(percent));
                if let Some(aura) = aura(u16::try_from(total).unwrap_or(u16::MAX)) {
                    self.put_aura(recipient, aura);
                }
            }
            AbilityEffect::Cleave { extra, radius } => {
                if let Some(victim) = target {
                    let centre = self.unit_position(victim)?;
                    let mut victims = vec![victim];
                    victims.extend(
                        self.living_hostiles_near(player_id, [centre[0], centre[2]], radius)?
                            .into_iter()
                            .filter(|&other| other != victim)
                            .take(usize::from(extra)),
                    );
                    for victim in victims {
                        let amount = self.weapon_roll(melee);
                        let amount = self.stance_damage(player_id, target, amount)?;
                        self.player_melee_hits(player_id, victim, amount, false, now)?;
                    }
                }
            }
            AbilityEffect::DamageOverTime {
                base, per_level, ..
            } => {
                if let (Some(victim), Some(aura)) = (target, aura(scaled(base, per_level, level))) {
                    self.hostile_aura(player_id, victim, aura)?;
                }
            }
            AbilityEffect::Snare { percent, .. } => {
                if let (Some(victim), Some(aura)) = (target, aura(percent)) {
                    self.hostile_aura(player_id, victim, aura)?;
                }
            }
            AbilityEffect::Root { .. } => {
                if let (Some(victim), Some(root)) = (target, aura(0)) {
                    self.hostile_aura(player_id, victim, root)?;
                }
            }
            AbilityEffect::Haste { percent, .. } => {
                if let Some(aura) = aura(percent) {
                    self.put_aura(source, aura);
                }
            }
            AbilityEffect::Bolt { damage, per_level } => {
                if let Some(victim) = target {
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + u32::from(scaled(0, per_level, level))
                        + bonus;
                    self.player_damages_unit(player_id, victim, amount, false, now)?;
                }
            }
            AbilityEffect::Nova { damage, radius, .. } => {
                let centre = self.player_position(player_id)?;
                for victim in self.living_hostiles_near(player_id, [centre.x, centre.z], radius)? {
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + bonus;
                    self.player_damages_unit(player_id, victim, amount, false, now)?;
                    if let Some(root) = aura(0) {
                        self.hostile_aura(player_id, victim, root)?;
                    }
                }
            }
            AbilityEffect::Absorb {
                base, per_level, ..
            } => {
                if let Some(aura) = aura(scaled(base, per_level, level)) {
                    let recipient = match target {
                        Some(friend @ EntityRef::Player(_)) if ability.is_friendly() => friend,
                        _ => source,
                    };
                    self.put_aura(recipient, aura);
                }
            }
            AbilityEffect::Blast {
                damage,
                per_level,
                radius,
                knockback,
            } => {
                if let Some(target) = target {
                    let centre = self.unit_position(target)?;
                    self.detonate(
                        Detonation {
                            attacker: player_id,
                            source: None,
                            centre,
                            radius,
                            damage,
                            bonus: u32::from(scaled(0, per_level, level)) + bonus,
                            knockback,
                            fallback,
                        },
                        now,
                    )?;
                }
            }
            AbilityEffect::Charge { knockback } => {
                if let Some(target) = target {
                    self.charge(source, target, knockback);
                }
            }
            AbilityEffect::Cone {
                damage,
                per_level,
                range,
                half_angle,
                percent,
                ..
            } => {
                for victim in self.cone_victims(player_id, range, half_angle)? {
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + u32::from(scaled(0, per_level, level))
                        + bonus;
                    self.player_damages_unit(player_id, victim, amount, false, now)?;
                    if let Some(snare) = aura(percent) {
                        self.hostile_aura(player_id, victim, snare)?;
                    }
                }
            }
            AbilityEffect::Chain {
                damage,
                per_level,
                jumps,
                radius,
            } => {
                if let Some(first) = target {
                    for victim in self.chain_victims(player_id, first, jumps, radius)? {
                        let amount = self
                            .rng
                            .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                            + u32::from(scaled(0, per_level, level))
                            + bonus;
                        self.player_damages_unit(player_id, victim, amount, false, now)?;
                    }
                }
            }
            // Blizzard, Lightning Beam and Drain Life pulse while they channel; bandages and
            // stomps are creature-only.
            AbilityEffect::Blizzard { .. }
            | AbilityEffect::Beam { .. }
            | AbilityEffect::Drain { .. }
            | AbilityEffect::Bandage { .. }
            | AbilityEffect::Stomp { .. }
            | AbilityEffect::Smash { .. } => {}
        }
        Ok(())
    }

    /// A weapon strike's damage: weapon damage × numerator / denominator +
    /// bonus; zero for any other effect.
    pub(crate) fn weapon_strike_amount(
        &mut self,
        player_id: PlayerId,
        effect: AbilityEffect,
    ) -> u32 {
        let AbilityEffect::WeaponStrike {
            numerator,
            denominator,
            bonus,
        } = effect
        else {
            return 0;
        };
        let Some(melee) = self
            .players
            .get(&player_id)
            .map(super::zone::PlayerState::melee_damage)
        else {
            return 0;
        };
        let weapon = self.weapon_roll(melee);
        weapon * u32::from(numerator) / u32::from(denominator.max(1)) + u32::from(bonus)
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

    /// Eligible opponents and creatures, in stable entity order. Safe-world creature order is unchanged.
    fn living_hostiles_near(
        &self,
        attacker: PlayerId,
        centre: [i32; 2],
        radius: i32,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let mut found = Vec::new();
        for (&victim, player) in &self.players {
            if self.players_are_hostile(attacker, victim) && player.is_alive() {
                let unit = EntityRef::Player(victim);
                let position = self.unit_position(unit)?;
                if distance_squared(centre, [position[0], position[2]]) <= i64::from(radius).pow(2)
                    && self.line_of_sight(EntityRef::Player(attacker), unit)?
                {
                    found.push(unit);
                }
            }
        }
        found.extend(
            self.living_creatures_near(centre, radius)?
                .into_iter()
                .map(EntityRef::Creature),
        );
        Ok(found)
    }

    /// One explosion (#287, #288): every victim the centre can see within `radius` takes
    /// `damage` (inclusive) + `bonus` and is knocked `knockback` units away, both falling off
    /// linearly to half at the edge; walls between centre and victim shield it. Creatures and
    /// props take the damage from `attacker`. With an exploding `source` (an explosive barrel)
    /// living players within reach are hurt by that source too.
    pub(crate) fn detonate(&mut self, blast: Detonation, now: u64) -> Result<(), ZoneError> {
        let Detonation {
            attacker,
            source,
            centre,
            radius,
            damage,
            bonus,
            knockback,
            fallback,
        } = blast;
        for (victim, distance) in self.blast_victims(
            centre,
            radius,
            source.is_some()
                || self.content.player_combat_policy() == crate::PlayerCombatPolicy::FreeForAll,
        )? {
            if source.is_none()
                && matches!(victim, EntityRef::Player(id) if !self.players_are_hostile(attacker, id))
            {
                continue;
            }
            // Linear falloff to half at the edge, in per-mille.
            let share = 1_000 - 500 * distance / i64::from(radius.max(1));
            let amount = self
                .rng
                .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                + bonus;
            let amount = u32::try_from(i64::from(amount) * share / 1_000)
                .unwrap_or(0)
                .max(1);
            match (victim, source) {
                (EntityRef::Creature(creature_id), _) => {
                    self.player_damages_creature(attacker, creature_id, amount, false, now)?;
                }
                (EntityRef::Player(player_id), Some(barrel)) => {
                    self.creature_damages_player(barrel, player_id, amount, false)?;
                }
                (EntityRef::Player(victim), None) => {
                    self.player_damages_unit(
                        attacker,
                        EntityRef::Player(victim),
                        amount,
                        false,
                        now,
                    )?;
                }
                _ => continue,
            }
            let push = i32::try_from(i64::from(knockback) * share / 1_000).unwrap_or(0);
            self.knock_away(victim, [centre[0], centre[2]], fallback, push)?;
        }
        // Destructible pieces (#305) take the blast's top roll, after the units it reached.
        self.blast_pieces(centre, radius, u32::from(damage[1]) + bonus, now)
    }

    /// A large creature's stomp (#297): falloff damage and knockback, as a blast centred on the
    /// creature, to every living player within reach that it can see.
    fn stomp(&mut self, creature_id: CreatureId, stomp: Stomp) -> Result<(), ZoneError> {
        let source = EntityRef::Creature(creature_id);
        let centre = self.unit_position(source)?;
        let facing = self
            .creatures
            .get(&creature_id)
            .map_or(0, |creature| creature.facing);
        let (x, z) = crate::trig::direction(facing);
        let radius = stomp.radius.max(1);
        for (victim, distance) in self.blast_victims(centre, radius, true)? {
            let EntityRef::Player(player_id) = victim else {
                continue;
            };
            // Linear falloff to half at the edge, in per-mille.
            let share = 1_000 - 500 * distance / i64::from(radius);
            let amount = self
                .rng
                .inclusive(u32::from(stomp.damage[0]), u32::from(stomp.damage[1]))
                + stomp.bonus;
            let amount = u32::try_from(i64::from(amount) * share / 1_000)
                .unwrap_or(0)
                .max(1);
            // A boss's special hits harder under a larger attempt's profile (#276).
            let amount = match self.creatures.get(&creature_id) {
                Some(creature) => {
                    let (spawn, _) = Self::creature_content(&self.content, creature_id)?;
                    creature.scale_damage(&self.content, spawn.template, amount)
                }
                None => amount,
            };
            self.creature_damages_player(creature_id, player_id, amount, false)?;
            let push = i32::try_from(i64::from(stomp.knockback) * share / 1_000).unwrap_or(0);
            self.knock_away(
                victim,
                [centre[0], centre[2]],
                [i64::from(x), i64::from(z)],
                push,
            )?;
        }
        Ok(())
    }

    /// Living units within `radius` of the blast `centre` that it can see (#287): living
    /// players first when `players`, then living creatures, with their XZ distance. A static
    /// collider between centre and body shields a unit.
    fn blast_victims(
        &self,
        centre: [i32; 3],
        radius: i32,
        players: bool,
    ) -> Result<Vec<(EntityRef, i64)>, ZoneError> {
        let mut units = Vec::new();
        if players {
            let mut near: Vec<_> = self
                .interest
                .near(centre[0], centre[2], radius)
                .into_iter()
                .filter(|unit| match unit {
                    EntityRef::Player(id) => self.players.get(id).is_some_and(|p| p.is_alive()),
                    _ => false,
                })
                .collect();
            near.sort_unstable();
            units.extend(near);
        }
        units.extend(
            self.living_creatures_near([centre[0], centre[2]], radius)?
                .into_iter()
                .map(EntityRef::Creature),
        );
        let origin = crate::physics::Vec3i::new(centre[0], centre[1], centre[2]);
        let mut victims = Vec::new();
        for unit in units {
            let at = self.unit_position(unit)?;
            let distance = distance_squared([centre[0], centre[2]], [at[0], at[2]]);
            if distance > i64::from(radius).pow(2) {
                continue;
            }
            let shielded = self.world.segment_blocked(
                origin,
                crate::physics::Vec3i::new(at[0] - centre[0], at[1] - centre[1], at[2] - centre[2]),
                |body| body.0 < crate::entity::PLAYER_BODY_BASE,
            );
            if !shielded {
                victims.push((unit, distance.isqrt()));
            }
        }
        Ok(victims)
    }

    /// Puts a player's aura on a creature: evading creatures ignore it,
    /// others engage the caster.
    fn hostile_aura(
        &mut self,
        player_id: PlayerId,
        target: EntityRef,
        aura: Aura,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        if !self.attackable_by_player(player_id, target) {
            return Ok(());
        }
        if let EntityRef::Creature(creature_id) = target
            && self
                .creatures
                .get(&creature_id)
                .is_some_and(|creature| matches!(creature.ai, CreatureAi::Evading { .. }))
        {
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
        if let EntityRef::Creature(creature_id) = target
            && let Some(creature) = self.creatures.get_mut(&creature_id)
        {
            creature.combat_timer = COMBAT_LINGER_TICKS;
        }
        if let EntityRef::Player(victim) = target {
            self.update_player(victim, |player| player.combat_timer = COMBAT_LINGER_TICKS);
        }
        self.update_player(player_id, |player| {
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        if let EntityRef::Creature(creature_id) = target {
            self.engage(creature_id, source, true)?;
        }
        Ok(())
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
        if matches!(
            event,
            ZoneEvent::CastStarted { .. }
                | ZoneEvent::AbilityUsed { .. }
                | ZoneEvent::StrikeSwept { .. }
                | ZoneEvent::Interrupted { .. }
        ) {
            let refs: Vec<_> = units.iter().flatten().copied().collect();
            self.notify_observers(&refs, &recipients, event);
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
                .npc_feet(npc_id)
                .map(|feet| [feet[0], 0, feet[1]])
                .ok_or_else(|| ZoneError::new("unknown npc")),
        }
    }

    /// Whether no static content collider lies on the straight segment between the two units'
    /// body centres. Physics owns the deterministic ray query; units never block each other.
    pub(crate) fn line_of_sight(&self, from: EntityRef, to: EntityRef) -> Result<bool, ZoneError> {
        let origin = self.unit_position(from)?;
        let end = self.unit_position(to)?;
        Ok(!self.world.segment_blocked(
            crate::physics::Vec3i::new(origin[0], origin[1], origin[2]),
            crate::physics::Vec3i::new(end[0] - origin[0], end[1] - origin[1], end[2] - origin[2]),
            |body| body.0 < crate::entity::PLAYER_BODY_BASE,
        ))
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
                        for victim in self.living_hostiles_near(player_id, point, radius)? {
                            let amount = self
                                .rng
                                .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                                + u32::from(scaled(0, per_level, level));
                            self.player_damages_unit(player_id, victim, amount, false, now)?;
                        }
                    }
                }
                (
                    AbilityEffect::Beam {
                        damage,
                        per_level,
                        length,
                        period,
                    },
                    Some(_),
                ) => {
                    // The beam follows the caster's facing every tick; its point is where it ends.
                    let trace = self.trace_beam(player_id, length)?;
                    if !done {
                        self.update_player(player_id, |player| {
                            if let Some(cast) = &mut player.cast {
                                cast.point = Some(trace.end);
                            }
                        });
                    }
                    if period > 0 && cast.elapsed % period == 0 {
                        self.beam_pulse(player_id, &trace, damage, per_level, level, now)?;
                    }
                }
                (
                    AbilityEffect::Drain {
                        damage,
                        per_level,
                        period,
                    },
                    Some(_),
                ) => {
                    if period > 0 && cast.elapsed % period == 0 {
                        let roll = [damage[0], damage[1]].map(|d| scaled(d, per_level, level));
                        self.drain_pulse(player_id, ability, cast.target, roll, now)?;
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

    /// One Drain Life pulse (#301): damage to the channel's target, and the caster heals by the
    /// health it lost. A target that died, left sight or range breaks the channel instead.
    fn drain_pulse(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        target: Option<EntityRef>,
        damage: [u16; 2],
        now: u64,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        let reach = i64::from(ability.range.reach().unwrap_or(0));
        let holds = match target {
            Some(target) if self.attackable_by_player(player_id, target) => {
                self.unit_distance_squared(source, target)? <= reach.pow(2)
                    && self.line_of_sight(source, target)?
            }
            _ => false,
        };
        let Some(target) = target.filter(|_| holds) else {
            self.update_player(player_id, |player| player.cast = None);
            self.notify_units(
                [Some(source), None],
                ZoneEvent::Interrupted {
                    source: None,
                    target: source,
                    ability: ability.id,
                },
            );
            return Ok(());
        };
        let before = self.unit_health(target);
        let amount = self
            .rng
            .inclusive(u32::from(damage[0]), u32::from(damage[1]));
        self.player_damages_unit(player_id, target, amount, false, now)?;
        let drained = before.saturating_sub(self.unit_health(target));
        if drained > 0 {
            self.heal_player(player_id, source, ability.id, drained);
        }
        Ok(())
    }

    fn unit_health(&self, unit: EntityRef) -> u32 {
        match unit {
            EntityRef::Player(id) => self.players.get(&id).map_or(0, |player| player.health),
            EntityRef::Creature(id) => self.creatures.get(&id).map_or(0, |unit| unit.health),
            EntityRef::Npc(_) => 0,
        }
    }

    fn complete_player_cast(
        &mut self,
        player_id: PlayerId,
        ability: &Ability,
        cast: CastState,
        now: u64,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        let target_alive = cast
            .target
            .is_none_or(|target| self.attackable_by_player(player_id, target));
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
        if let Some(target) = cast.target
            && matches!(ability.range, AbilityRange::Units(_))
            && !self.line_of_sight(source, target)?
        {
            self.notify(
                player_id,
                ZoneEvent::Error {
                    code: ErrorCode::LineOfSight,
                    target: Some(target),
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
                if !self.line_of_sight(source, target)? {
                    // The target stepped behind cover: the bolt fizzles.
                    self.notify_units(
                        [Some(source), Some(target)],
                        ZoneEvent::Interrupted {
                            source: None,
                            target: source,
                            ability: ability.id,
                        },
                    );
                    return Ok(());
                }
                self.notify_units(
                    [Some(source), Some(target)],
                    ZoneEvent::AbilityUsed {
                        source,
                        target: Some(target),
                        ability: ability.id,
                    },
                );
                // A shot carries the bolt: it lands when the projectile does (#286).
                if let Some(kind) = ability.projectile {
                    return self.shoot_at(source, target, kind, ability.id);
                }
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
                let max_health = creature.max_health(&content, template);
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
            AbilityEffect::Stomp {
                damage,
                per_level,
                radius,
                knockback,
            } => {
                self.restart_creature_ability(creature_id, ability);
                self.notify_units(
                    [Some(source), None],
                    ZoneEvent::AbilityUsed {
                        source,
                        target: None,
                        ability: ability.id,
                    },
                );
                self.stomp(
                    creature_id,
                    Stomp {
                        damage,
                        bonus: u32::from(scaled(0, per_level, level)),
                        radius,
                        knockback,
                    },
                )?;
            }
            // The Redbrand Brute's Overhead Smash (#160): whoever still stands in the arc ahead
            // of it when the wind-up ends takes the blow and is knocked back.
            AbilityEffect::Smash {
                damage,
                per_level,
                range,
                half_angle,
                knockback,
            } => {
                self.restart_creature_ability(creature_id, ability);
                self.notify_units(
                    [Some(source), None],
                    ZoneEvent::AbilityUsed {
                        source,
                        target: None,
                        ability: ability.id,
                    },
                );
                let centre = self.unit_position(source)?;
                let facing = self
                    .creatures
                    .get(&creature_id)
                    .map_or(0, |creature| creature.facing);
                let (x, z) = crate::trig::direction(facing);
                let fallback = [i64::from(x), i64::from(z)];
                for victim in self.creature_cone_victims(creature_id, range, half_angle)? {
                    let EntityRef::Player(player_id) = victim else {
                        continue;
                    };
                    let amount = self
                        .rng
                        .inclusive(u32::from(damage[0]), u32::from(damage[1]))
                        + u32::from(scaled(0, per_level, level));
                    self.creature_damages_player(creature_id, player_id, amount, false)?;
                    self.knock_away(victim, [centre[0], centre[2]], fallback, knockback)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A creature's shot of `ability` reached `player_id` (#286): its bolt damage lands.
    pub(crate) fn creature_effect_lands(
        &mut self,
        creature_id: CreatureId,
        player_id: PlayerId,
        ability: &Ability,
    ) -> Result<(), ZoneError> {
        let AbilityEffect::Bolt { damage, per_level } = ability.effect else {
            return Ok(());
        };
        let level = self
            .creatures
            .get(&creature_id)
            .map_or(1, |creature| creature.level);
        let amount = self
            .rng
            .inclusive(u32::from(damage[0]), u32::from(damage[1]))
            + u32::from(scaled(0, per_level, level));
        self.creature_damages_player(creature_id, player_id, amount, false)
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
            // A stomp (#297) lands around the creature once its leader stands within reach.
            AbilityEffect::Stomp { radius, .. } => {
                if distance_squared(here, leader_at) > i64::from(radius).pow(2) {
                    return Ok(false);
                }
                None
            }
            AbilityEffect::Bandage { below_percent, .. } => {
                let max_health = u64::from(creature.max_health(&content, template));
                if u64::from(creature.health) * 100 >= max_health * u64::from(below_percent) {
                    return Ok(false);
                }
                None
            }
            _ => {
                let reach = ability.range.reach().unwrap_or(0);
                if distance_squared(here, leader_at) > i64::from(reach).pow(2)
                    || !self.line_of_sight(EntityRef::Creature(creature_id), leader)?
                {
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
            (AuraKind::DamageOverTime, EntityRef::Player(victim), EntityRef::Player(attacker)) => {
                self.player_damages_unit(
                    attacker,
                    EntityRef::Player(victim),
                    u32::from(amount),
                    false,
                    now,
                )
            }
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
                // Healing a player a boss fights is meaningful contribution (#276, 5.1.74).
                if let (EntityRef::Player(healer), Some(encounter)) =
                    (healer, creature.encounter.as_mut())
                {
                    encounter.contribute(healer);
                }
            }
        }
        if let EntityRef::Player(healer) = healer {
            self.gain_faith(healer, healed);
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
                Some(
                    AbilityEffect::Nova {
                        unbreakable_ticks,
                        ..
                    } | AbilityEffect::Root {
                        unbreakable_ticks,
                        ..
                    }
                ) if aura.elapsed() >= unbreakable_ticks
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
