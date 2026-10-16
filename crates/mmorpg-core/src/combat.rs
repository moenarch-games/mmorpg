//! Targeting, auto-attack, damage, death and recovery of units: tick steps
//! 1 (player intents), 6 (combat resolution) and 7 (timers).
//!
//! Well-formed intents that are not allowed right now produce an
//! [`ErrorCode`] event for the player and never fail the tick.

use std::collections::BTreeSet;
use std::sync::Arc;

use physics_engine::Vec3i;

use crate::ai::{creature_reach, distance_squared};
use crate::creature::Life;
use crate::entity::body_id;
use crate::events::push_event;
use crate::snapshot::{CreatureAi, PlayerIntent};
use crate::unit::{
    COMBAT_LINGER_TICKS, CORPSE_TICKS, OUT_OF_RANGE_ERROR_INTERVAL_TICKS, PLAYER_REACH_UNITS,
    PLAYER_SWING_TICKS, REGEN_DELAY_TICKS, REGEN_INTERVAL_TICKS, REGEN_PERCENT,
    RELEASE_HEALTH_PERCENT, Swing, percent_of, roll_swing,
};
use crate::zone::{Axis, physics_error};
use crate::{
    CreatureId, EntityRef, ErrorCode, INTEREST_RADIUS_UNITS, PLAYER_HALF_EXTENTS_UNITS, PlayerId,
    ZoneError, ZoneEvent, ZoneSimulation,
};

impl ZoneSimulation {
    /// Step 1: every player's queued intents in `(player id, sequence)` order.
    pub(crate) fn consume_intents(&mut self) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.players.keys().copied().collect();
        for player_id in ids {
            let (intents, dropped) = match self.players.get_mut(&player_id) {
                Some(player) => (
                    std::mem::take(&mut player.intents),
                    std::mem::take(&mut player.intents_dropped),
                ),
                None => continue,
            };
            for intent in intents {
                self.apply_intent(player_id, intent, self.tick + 1)?;
            }
            // The dropped intents followed every queued one.
            if dropped {
                self.notify(
                    player_id,
                    ZoneEvent::Error {
                        code: ErrorCode::TooManyIntents,
                        target: None,
                    },
                );
            }
        }
        Ok(())
    }

    fn apply_intent(
        &mut self,
        player_id: PlayerId,
        intent: PlayerIntent,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        let alive = player.is_alive();
        let current_target = player.target;
        let refusal = match intent {
            PlayerIntent::StopAttack => {
                self.update_player(player_id, |player| player.auto_attack = false);
                None
            }
            PlayerIntent::ReleaseSpirit if alive => Some((ErrorCode::NotDead, None)),
            PlayerIntent::ReleaseSpirit => {
                self.release_spirit(player_id)?;
                None
            }
            PlayerIntent::ChooseClass { class, sex } => self.choose_class(player_id, class, sex),
            PlayerIntent::UseAbility { ability, target } => {
                self.use_ability(player_id, ability, target, now)?
            }
            PlayerIntent::CancelCast => {
                self.cancel_cast(player_id);
                None
            }
            // Dead players can only release their spirit.
            PlayerIntent::SelectTarget(_)
            | PlayerIntent::StartAttack
            | PlayerIntent::MoveItem { .. }
            | PlayerIntent::EquipItem { .. }
            | PlayerIntent::UnequipItem { .. }
            | PlayerIntent::BuyItem { .. }
            | PlayerIntent::SellItem { .. }
            | PlayerIntent::Loot(_)
            | PlayerIntent::AcceptQuest { .. }
            | PlayerIntent::CompleteQuest { .. }
                if !alive =>
            {
                Some((ErrorCode::YouAreDead, None))
            }
            PlayerIntent::MoveItem {
                source,
                destination,
                quantity,
            } => {
                let player = self
                    .players
                    .get_mut(&player_id)
                    .ok_or_else(|| ZoneError::new("inventory owner is absent"))?;
                let mut candidate = player.inventory.clone();
                match candidate.move_stack(usize::from(source), usize::from(destination), quantity)
                {
                    Err(crate::InventoryError::NoCapacity) => {
                        Some((ErrorCode::InventoryFull, None))
                    }
                    Err(_) => Some((ErrorCode::InvalidInventoryMove, None)),
                    Ok(()) if candidate == player.inventory => None,
                    Ok(()) => {
                        if let Some(revision) = player.inventory_revision.checked_add(1) {
                            player.inventory = candidate;
                            player.inventory_revision = revision;
                            player.inventory_changed_at = self.tick + 1;
                            None
                        } else {
                            Some((ErrorCode::InvalidInventoryMove, None))
                        }
                    }
                }
            }
            PlayerIntent::EquipItem { bag_slot } => self
                .change_equipment(player_id, |equipment, bag| {
                    equipment.equip(bag, usize::from(bag_slot))
                })?,
            PlayerIntent::UnequipItem { equipment_slot } => self
                .change_equipment(player_id, |equipment, bag| {
                    equipment.unequip(bag, equipment_slot)
                })?,
            PlayerIntent::Loot(claim) => self.claim_loot(player_id, claim)?,
            PlayerIntent::Chat(message) => {
                self.speak(player_id, message, now)?;
                None
            }
            PlayerIntent::BuyItem {
                npc,
                offer,
                quantity,
            } => self.trade(
                player_id,
                npc,
                crate::vendor::Trade::Buy { offer, quantity },
            )?,
            PlayerIntent::SellItem {
                npc,
                bag_slot,
                quantity,
            } => self.trade(
                player_id,
                npc,
                crate::vendor::Trade::Sell { bag_slot, quantity },
            )?,
            PlayerIntent::AcceptQuest { npc, quest } => self.quest_request(
                player_id,
                crate::quest::QuestRequest::Accept { npc, quest },
                now,
            )?,
            PlayerIntent::CompleteQuest { npc, quest, choice } => self.quest_request(
                player_id,
                crate::quest::QuestRequest::Complete { npc, quest, choice },
                now,
            )?,
            PlayerIntent::AbandonQuest { quest } => self.quest_request(
                player_id,
                crate::quest::QuestRequest::Abandon { quest },
                now,
            )?,
            PlayerIntent::SelectTarget(None) => {
                self.update_player(player_id, |player| {
                    player.target = None;
                    player.auto_attack = false;
                });
                None
            }
            PlayerIntent::SelectTarget(Some(target)) => {
                if self.visible_to(player_id, target)? {
                    self.update_player(player_id, |player| {
                        if player.target != Some(target) {
                            player.auto_attack = false;
                        }
                        player.target = Some(target);
                    });
                    None
                } else {
                    Some((ErrorCode::InvalidTarget, Some(target)))
                }
            }
            PlayerIntent::StartAttack => match current_target {
                None => Some((ErrorCode::NoTarget, None)),
                Some(target @ (EntityRef::Player(_) | EntityRef::Npc(_))) => {
                    Some((ErrorCode::NotAttackable, Some(target)))
                }
                Some(target @ EntityRef::Creature(creature_id)) => {
                    match self
                        .creatures
                        .get(&creature_id)
                        .map(|creature| creature.life)
                    {
                        Some(Life::Alive) => {
                            self.update_player(player_id, |player| player.auto_attack = true);
                            None
                        }
                        Some(Life::Corpse { .. }) => Some((ErrorCode::TargetDead, Some(target))),
                        Some(Life::Despawned { .. }) | None => {
                            Some((ErrorCode::InvalidTarget, Some(target)))
                        }
                    }
                }
            },
        };
        if let Some((code, target)) = refusal {
            self.notify(player_id, ZoneEvent::Error { code, target });
        }
        Ok(())
    }

    pub(crate) fn update_player(
        &mut self,
        player_id: PlayerId,
        update: impl FnOnce(&mut crate::zone::PlayerState),
    ) {
        if let Some(player) = self.players.get_mut(&player_id) {
            update(player);
        }
    }

    pub(crate) fn notify(&mut self, player_id: PlayerId, event: ZoneEvent) {
        if let Some(player) = self.players.get_mut(&player_id) {
            push_event(&mut player.events, event);
        }
    }

    /// Whether `target` exists and is within the viewer's interest radius:
    /// a player, a living creature or corpse, or an NPC.
    pub(crate) fn visible_to(
        &self,
        viewer: PlayerId,
        target: EntityRef,
    ) -> Result<bool, ZoneError> {
        let center = self.player_position(viewer)?;
        let position = match target {
            EntityRef::Player(player_id) => {
                if !self.players.contains_key(&player_id) {
                    return Ok(false);
                }
                let position = self.player_position(player_id)?;
                [position.x, position.y, position.z]
            }
            EntityRef::Creature(creature_id) => match self.creatures.get(&creature_id) {
                Some(creature) => match self.creature_position(creature_id, creature)? {
                    Some(position) => position,
                    None => return Ok(false),
                },
                None => return Ok(false),
            },
            EntityRef::Npc(npc_id) => match self.content.npc(npc_id) {
                Some(npc) => [npc.position[0], 0, npc.position[1]],
                None => return Ok(false),
            },
        };
        Ok(
            distance_squared([center.x, center.z], [position[0], position[2]])
                <= i64::from(INTEREST_RADIUS_UNITS).pow(2),
        )
    }

    /// A dead player returns to the graveyard with half health, out of combat.
    fn release_spirit(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        let graveyard = self.content.graveyard();
        let body = body_id(EntityRef::Player(player_id));
        let position = Vec3i::new(graveyard[0], PLAYER_HALF_EXTENTS_UNITS[1], graveyard[1]);
        self.world
            .set_position(body, position)
            .map_err(physics_error)?;
        self.world
            .set_velocity(body, Vec3i::ZERO)
            .map_err(physics_error)?;
        self.update_player(player_id, |player| {
            player.health = percent_of(player.max_health(), RELEASE_HEALTH_PERCENT);
            player.auto_attack = false;
            player.swing_timer = 0;
            player.combat_timer = 0;
            player.calm_ticks = 0;
            player.error_cooldown = 0;
        });
        self.place_in_interest(EntityRef::Player(player_id), position.x, position.z);
        Ok(())
    }

    /// Step 6: swings in `EntityRef` order, players first.
    pub(crate) fn resolve_combat(&mut self, now: u64) -> Result<(), ZoneError> {
        let players: Vec<_> = self.players.keys().copied().collect();
        for player_id in players {
            self.player_swing(player_id, now)?;
        }
        let creatures: Vec<_> = self.creatures.keys().copied().collect();
        for creature_id in creatures {
            self.creature_swing(creature_id)?;
        }
        Ok(())
    }

    fn player_swing(&mut self, player_id: PlayerId, now: u64) -> Result<(), ZoneError> {
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        // Timers run for the dead too: combat ends 5 s after the killing blow.
        player.swing_timer = player.swing_timer.saturating_sub(1);
        player.combat_timer = player.combat_timer.saturating_sub(1);
        player.error_cooldown = player.error_cooldown.saturating_sub(1);
        if !player.is_alive() || !player.auto_attack {
            return Ok(());
        }
        let (target, creature_id) = match player.target {
            Some(target @ EntityRef::Creature(creature_id)) => (target, creature_id),
            _ => {
                player.auto_attack = false;
                return Ok(());
            }
        };
        // The target's death stops auto-attack.
        if !self
            .creatures
            .get(&creature_id)
            .is_some_and(|creature| creature.is_alive())
        {
            self.update_player(player_id, |player| player.auto_attack = false);
            return Ok(());
        }
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        // A stunned player's ready swing waits.
        if player.swing_timer > 0 || crate::aura::has_kind(&player.auras, crate::AuraKind::Stun) {
            return Ok(());
        }
        let (damage, error_ready) = (player.melee_damage(), player.error_cooldown == 0);
        let swing_ticks = crate::aura::hasted_swing(PLAYER_SWING_TICKS, &player.auras);
        let from = self.player_position(player_id)?;
        let to = self
            .world
            .body(body_id(target))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
            .position();
        if distance_squared([from.x, from.z], [to.x, to.z]) > i64::from(PLAYER_REACH_UNITS).pow(2) {
            // Out of range holds the ready swing; the error is throttled.
            if error_ready {
                self.update_player(player_id, |player| {
                    player.error_cooldown = OUT_OF_RANGE_ERROR_INTERVAL_TICKS;
                });
                self.notify(
                    player_id,
                    ZoneEvent::Error {
                        code: ErrorCode::OutOfRange,
                        target: Some(target),
                    },
                );
            }
            return Ok(());
        }
        self.update_player(player_id, |player| {
            player.swing_timer = swing_ticks;
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        self.attack_creature(player_id, creature_id, damage, now)
    }

    /// Resolves one player swing against a creature: evading creatures
    /// ignore it; otherwise the hit table decides, damage lands, the first
    /// damaging player taps the creature and threat rises by the damage.
    fn attack_creature(
        &mut self,
        player_id: PlayerId,
        creature_id: CreatureId,
        damage: [u16; 2],
        now: u64,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        let target = EntityRef::Creature(creature_id);
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(());
        };
        if matches!(creature.ai, CreatureAi::Evading { .. }) {
            self.notify(player_id, ZoneEvent::Evade { source, target });
            return Ok(());
        }
        let swing = roll_swing(&mut self.rng, damage);
        match swing {
            Swing::Miss => {
                let Some(creature) = self.creatures.get_mut(&creature_id) else {
                    return Ok(());
                };
                creature.combat_timer = COMBAT_LINGER_TICKS;
                creature.add_threat(source, 0);
                self.notify(player_id, ZoneEvent::Miss { source, target });
                self.engage(creature_id, source, true)
            }
            Swing::Hit { amount, critical } => {
                self.update_player(player_id, |player| {
                    if let Some(choice) = player.class
                        && choice.class.resource() == crate::ResourceKind::Rage
                    {
                        player.resource.gain(
                            crate::ResourceKind::Rage,
                            player.level,
                            crate::class::RAGE_PER_HIT_DEALT,
                        );
                    }
                });
                self.player_damages_creature(
                    player_id,
                    creature_id,
                    u32::from(amount),
                    critical,
                    now,
                )
            }
        }
    }

    /// Damage from a player to a living creature, by a swing, an ability or
    /// an aura: evading creatures ignore it; otherwise it lands, the first
    /// damaging player taps the creature, threat rises by the damage, a
    /// breakable root breaks, and the creature dies or engages the player.
    pub(crate) fn player_damages_creature(
        &mut self,
        player_id: PlayerId,
        creature_id: CreatureId,
        amount: u32,
        critical: bool,
        now: u64,
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
        let amount = self.absorb(target, source, amount);
        self.update_player(player_id, |player| {
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        creature.combat_timer = COMBAT_LINGER_TICKS;
        let dealt = amount.min(creature.health);
        creature.health -= dealt;
        creature.add_threat(source, dealt);
        creature.tapped_by.get_or_insert(player_id);
        let dead = creature.health == 0;
        self.notify(
            player_id,
            ZoneEvent::DamageDealt {
                source,
                target,
                amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                critical,
            },
        );
        if dead {
            return self.creature_dies(creature_id, source, now);
        }
        self.break_roots(target);
        // Being attacked engages an idle creature (and its family); an
        // engaged one already has the attacker on its threat table.
        self.engage(creature_id, source, true)
    }

    /// Damage from a creature to a living player, by a swing or an ability:
    /// shields absorb first, a Warden gains rage for the hit, a breakable
    /// root breaks, and the player may die.
    pub(crate) fn creature_damages_player(
        &mut self,
        creature_id: CreatureId,
        player_id: PlayerId,
        amount: u32,
        critical: bool,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Creature(creature_id);
        let target = EntityRef::Player(player_id);
        if !self
            .players
            .get(&player_id)
            .is_some_and(|player| player.is_alive())
        {
            return Ok(());
        }
        let left = self.absorb(target, source, amount);
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        player.combat_timer = COMBAT_LINGER_TICKS;
        if let Some(choice) = player.class
            && choice.class.resource() == crate::ResourceKind::Rage
        {
            player.resource.gain(
                crate::ResourceKind::Rage,
                player.level,
                crate::class::RAGE_PER_HIT_TAKEN,
            );
        }
        if left == 0 && amount > 0 {
            return Ok(());
        }
        let dealt = left.min(player.health);
        player.health -= dealt;
        let dead = player.health == 0;
        push_event(
            &mut player.events,
            ZoneEvent::DamageTaken {
                source,
                target,
                amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                critical,
            },
        );
        if dead {
            return self.player_dies(player_id, source);
        }
        self.break_roots(target);
        Ok(())
    }

    fn creature_swing(&mut self, creature_id: CreatureId) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (_, template) = Self::creature_content(&content, creature_id)?;
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        if !creature.is_alive() {
            return Ok(());
        }
        creature.swing_timer = creature.swing_timer.saturating_sub(1);
        creature.combat_timer = creature.combat_timer.saturating_sub(1);
        // Casting and stunned creatures hold their ready swing.
        if creature.ai != CreatureAi::Engaged
            || creature.swing_timer > 0
            || creature.cast.is_some()
            || crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun)
        {
            return Ok(());
        }
        let Some(target @ EntityRef::Player(player_id)) = creature.top_threat() else {
            return Ok(());
        };
        let level = creature.level;
        if !self
            .players
            .get(&player_id)
            .is_some_and(|player| player.is_alive())
        {
            return Ok(());
        }
        let source = EntityRef::Creature(creature_id);
        let from = self
            .world
            .body(body_id(source))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
            .position();
        let to = self.player_position(player_id)?;
        let reach = i64::from(creature_reach(target));
        if distance_squared([from.x, from.z], [to.x, to.z]) > reach * reach {
            return Ok(());
        }
        let swing = roll_swing(&mut self.rng, template.damage_at(level));
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.swing_timer = template.swing_ticks;
            creature.combat_timer = COMBAT_LINGER_TICKS;
        }
        match swing {
            Swing::Miss => {
                let Some(player) = self.players.get_mut(&player_id) else {
                    return Ok(());
                };
                player.combat_timer = COMBAT_LINGER_TICKS;
                push_event(&mut player.events, ZoneEvent::Miss { source, target });
                Ok(())
            }
            Swing::Hit { amount, critical } => {
                self.creature_damages_player(creature_id, player_id, u32::from(amount), critical)
            }
        }
    }

    /// The creature leaves a corpse without a physics body; its tap stays.
    fn creature_dies(
        &mut self,
        creature_id: CreatureId,
        killer: EntityRef,
        now: u64,
    ) -> Result<(), ZoneError> {
        let entity = EntityRef::Creature(creature_id);
        let body = self
            .world
            .remove_body(body_id(entity))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
        let position = body.position();
        let (spawn, _) = Self::creature_content(&self.content, creature_id)?;
        let template = spawn.template;
        let mut loot = self.content.loot_table(template).map(|table| {
            table.roll(crate::LootRolls {
                money: (self.loot_rng.next_u64() >> 32) as u32,
                outcome: (self.loot_rng.next_u64() >> 32) as u32,
                quantity: (self.loot_rng.next_u64() >> 32) as u32,
            })
        });
        // A quest item drops for the tapper only while a quest needs it; it
        // draws nothing from the loot stream.
        let tapper = self
            .creatures
            .get(&creature_id)
            .and_then(|creature| creature.tapped_by);
        let quest_item = tapper
            .and_then(|player_id| self.players.get(&player_id))
            .and_then(|player| {
                crate::quest::quest_drop(&self.content, &player.quests, &player.inventory, template)
            });
        if let Some(item) = quest_item {
            let stack = crate::ItemStack::new(item, 1)
                .map_err(|_| ZoneError::new("quest item is not in the catalog"))?;
            let rewards = loot.get_or_insert(crate::LootRewards {
                money: 0,
                item: None,
                quest_item: None,
            });
            rewards.quest_item = Some(stack);
        }
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        creature.life = Life::Corpse {
            died_at: now,
            position: [position.x, position.y, position.z],
        };
        creature.ai = CreatureAi::Idle {
            timer: 0,
            destination: None,
        };
        creature.threat.clear();
        creature.swing_timer = 0;
        creature.combat_timer = 0;
        creature.reset_abilities();
        creature.loot = creature.tapped_by.and(loot);
        let tapper = creature.tapped_by;
        let level = creature.level;
        let died = ZoneEvent::Died {
            entity,
            killer: Some(killer),
        };
        let mut recipients = BTreeSet::new();
        if let EntityRef::Player(player_id) = killer {
            recipients.insert(player_id);
        }
        recipients.extend(tapper);
        for player_id in recipients {
            self.notify(player_id, died);
        }
        if let Some(player_id) = tapper
            && self.kill_credit_eligible(player_id, [position.x, position.y, position.z])?
        {
            self.reward_kill_experience(player_id, level)?;
            self.credit_kill(player_id, template, now);
        }
        Ok(())
    }

    /// A dead player keeps its body where it fell, stops attacking, lets go
    /// of its held movement and pending jump, and leaves every threat table.
    fn player_dies(&mut self, player_id: PlayerId, killer: EntityRef) -> Result<(), ZoneError> {
        let entity = EntityRef::Player(player_id);
        let body = body_id(entity);
        let vertical = self
            .world
            .body(body)
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?
            .velocity()
            .y;
        self.world
            .set_velocity(body, Vec3i::new(0, vertical, 0))
            .map_err(physics_error)?;
        for creature in self.creatures.values_mut() {
            creature.forget(entity);
            // A cast at the fallen player fizzles; a released spirit is safe.
            if creature
                .cast
                .is_some_and(|cast| cast.target == Some(entity))
            {
                creature.cast = None;
            }
        }
        self.forget_caster(entity);
        self.update_player(player_id, |player| {
            player.health = 0;
            player.auto_attack = false;
            player.swing_timer = 0;
            player.calm_ticks = 0;
            // Death clears every aura and cast; rage drains.
            player.cast = None;
            player.auras.clear();
            if let Some(choice) = player.class
                && choice.class.resource() == crate::ResourceKind::Rage
            {
                player.resource.value = 0;
                player.resource.ticks = 0;
            }
            player.forward = Axis::Zero;
            player.strafe = Axis::Zero;
            player.jump_pending = false;
        });
        self.notify(
            player_id,
            ZoneEvent::Died {
                entity,
                killer: Some(killer),
            },
        );
        Ok(())
    }

    /// Players on any living creature's threat table.
    pub(crate) fn threatened_players(&self) -> BTreeSet<PlayerId> {
        self.creatures
            .values()
            .filter(|creature| creature.is_alive())
            .flat_map(|creature| &creature.threat)
            .filter_map(|entry| match entry.entity {
                EntityRef::Player(player_id) => Some(player_id),
                EntityRef::Creature(_) | EntityRef::Npc(_) => None,
            })
            .collect()
    }

    /// Step 7: player regeneration, then corpse despawns and respawns in
    /// creature-ID order.
    pub(crate) fn update_timers(&mut self, now: u64) -> Result<(), ZoneError> {
        let threatened = self.threatened_players();
        for (player_id, player) in &mut self.players {
            if !player.is_alive() {
                continue;
            }
            if let Some(choice) = player.class {
                let in_combat = player.combat_timer > 0 || threatened.contains(player_id);
                player
                    .resource
                    .advance(choice.class.resource(), player.level, in_combat);
            }
            let max_health = player.max_health();
            // Calm time only counts while there is health to regain, so a
            // resting, healthy player's state does not change.
            if player.combat_timer > 0
                || threatened.contains(player_id)
                || player.health >= max_health
            {
                player.calm_ticks = 0;
                continue;
            }
            player.calm_ticks = player.calm_ticks.saturating_add(1);
            if player.calm_ticks >= REGEN_DELAY_TICKS {
                // Regenerate now and again every interval while calm.
                player.calm_ticks = REGEN_DELAY_TICKS - REGEN_INTERVAL_TICKS;
                player.health = player
                    .health
                    .saturating_add(percent_of(max_health, REGEN_PERCENT))
                    .min(max_health);
            }
        }

        let content = Arc::clone(&self.content);
        let ids: Vec<_> = self.creatures.keys().copied().collect();
        for creature_id in ids {
            let Some(creature) = self.creatures.get_mut(&creature_id) else {
                continue;
            };
            let entity = EntityRef::Creature(creature_id);
            if let Life::Corpse { died_at, .. } = creature.life
                && now >= died_at.saturating_add(u64::from(CORPSE_TICKS))
            {
                creature.life = Life::Despawned { died_at };
                creature.loot = None;
                if self.interest.remove(entity) {
                    self.interest_work.bucket_removes += 1;
                }
                for player in self.players.values_mut() {
                    if player.target == Some(entity) {
                        player.target = None;
                        player.auto_attack = false;
                    }
                }
            }
            let Some(creature) = self.creatures.get(&creature_id) else {
                continue;
            };
            let (_, template) = Self::creature_content(&content, creature_id)?;
            if let Life::Despawned { died_at } = creature.life
                && now >= died_at.saturating_add(u64::from(template.respawn_ticks))
            {
                self.spawn_creature(creature_id)?;
            }
        }
        Ok(())
    }
}
