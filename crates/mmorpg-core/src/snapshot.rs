//! Canonical snapshots: every authoritative field needed to continue a zone
//! exactly, for trusted replay and recovery. Content is referenced by
//! revision and fingerprint, never embedded: recovery needs the identical
//! [`ZoneContent`] and fails closed on any mismatch.

use std::collections::BTreeSet;
use std::sync::Arc;

use physics_engine::RigidBody;

use crate::ability::{
    AbilityId, AuraKind, CREATURE_ABILITY_JITTER_TICKS, GLOBAL_COOLDOWN_TICKS, MAX_AURAS,
    MAX_COOLDOWNS, ability_by_id, learned,
};
use crate::ai::is_wander_point;
use crate::class::ResourceClock;
use crate::content::within_content_range;
use crate::creature::{CreatureState, Life, MAX_THREAT_ENTRIES};
use crate::entity::body_id;
use crate::rng::ZoneRng;
use crate::unit::{PLAYER_START_LEVEL, player_max_health};
use crate::zone::{
    Axis, MAX_PENDING_INTENTS, PLAYER_HALF_EXTENTS, PlayerState, physics_error, vector,
};
use crate::{
    Aura, CastState, ClassChoice, Cooldown, CreatureId, EntityRef, MAX_EVENTS_PER_PLAYER,
    MAX_PLAYERS_PER_ZONE, PlayerId, SNAPSHOT_SCHEMA_VERSION, ZoneContent, ZoneError, ZoneEvent,
    ZoneId, ZoneSimulation,
};

/// A queued discrete intent, consumed by the next tick in sequence order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerIntent {
    SelectTarget(Option<EntityRef>),
    Loot(crate::LootClaim),
    StartAttack,
    StopAttack,
    ReleaseSpirit,
    MoveItem {
        source: u8,
        destination: u8,
        quantity: u16,
    },
    UseAbility {
        ability: u8,
        target: Option<EntityRef>,
    },
    CancelCast,
    ChooseClass {
        class: u8,
        sex: u8,
    },
    EquipItem {
        bag_slot: u8,
    },
    UnequipItem {
        equipment_slot: u8,
    },
    BuyItem {
        npc: crate::NpcId,
        offer: u8,
        quantity: u16,
    },
    SellItem {
        npc: crate::NpcId,
        bag_slot: u8,
        quantity: u16,
    },
    /// A chat line or an emote (#69).
    Chat(crate::ChatMessage),
    AcceptQuest {
        npc: crate::NpcId,
        quest: u8,
    },
    CompleteQuest {
        npc: crate::NpcId,
        quest: u8,
        choice: u8,
    },
    AbandonQuest {
        quest: u8,
    },
}

/// A creature's decision state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreatureAi {
    /// Waiting `timer` ticks, or walking to `destination` for at most `timer` ticks.
    Idle {
        timer: u16,
        destination: Option<[i32; 2]>,
    },
    /// Fighting its threat leader.
    Engaged,
    /// Running home for `ticks` ticks so far.
    Evading { ticks: u16 },
}

/// A creature's life cycle; the tick numbers are those of its death.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreatureLife {
    Alive,
    Corpse { died_at: u64 },
    Despawned { died_at: u64 },
}

/// One threat table row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreatEntry {
    pub entity: EntityRef,
    pub threat: u32,
}

/// Unit and combat state of a player.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalPlayerCombat {
    pub level: u8,
    /// Current-level XP; zero at the starter cap.
    pub experience: u32,
    /// Zero means dead.
    pub health: u32,
    pub target: Option<EntityRef>,
    pub auto_attack: bool,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub calm_ticks: u16,
    pub error_cooldown: u16,
    /// Queued intents in sequence order.
    pub intents: Vec<PlayerIntent>,
    /// A later intent found the queue full and was dropped; the next tick
    /// reports it. Only a full queue drops intents.
    pub intents_dropped: bool,
    /// Events of the current tick.
    pub events: Vec<ZoneEvent>,
    pub abilities: CanonicalPlayerAbilities,
}

/// Class, resource and ability state of a player; all zero and empty until
/// the player chooses a class.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CanonicalPlayerAbilities {
    pub class: Option<ClassChoice>,
    pub resource: u16,
    /// Ticks counted toward the next resource step.
    pub resource_ticks: u16,
    /// Arcanist five-second rule: ticks left without mana regeneration.
    pub mana_delay: u16,
    pub global_cooldown: u16,
    /// Running cooldowns in ability order.
    pub cooldowns: Vec<Cooldown>,
    pub cast: Option<CastState>,
    /// Auras in slot order.
    pub auras: Vec<Aura>,
}

/// Ability state of a creature: its content-bound ability's timer, its cast
/// and its auras.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CanonicalCreatureAbilities {
    pub ability_timer: u16,
    pub cast: Option<CastState>,
    /// Auras in slot order.
    pub auras: Vec<Aura>,
}

impl Default for CanonicalPlayerCombat {
    /// A freshly admitted player: level 1, full health, no target.
    fn default() -> Self {
        Self {
            level: PLAYER_START_LEVEL,
            experience: 0,
            health: player_max_health(PLAYER_START_LEVEL),
            target: None,
            auto_attack: false,
            swing_timer: 0,
            combat_timer: 0,
            calm_ticks: 0,
            error_cooldown: 0,
            intents: Vec::new(),
            intents_dropped: false,
            events: Vec::new(),
            abilities: CanonicalPlayerAbilities::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalPlayerSnapshot {
    pub player_id: PlayerId,
    pub position: [i32; 3],
    pub velocity: [i32; 3],
    pub facing: u16,
    pub forward: i8,
    pub strafe: i8,
    pub jump_pending: bool,
    pub last_sequence: u32,
    pub spawn_slot: u16,
    pub copper: u32,
    pub inventory: crate::Inventory,
    pub inventory_revision: u64,
    pub inventory_changed_at: u64,
    /// Equipped items; validated against the catalog by construction.
    pub equipment: crate::Equipment,
    pub combat: CanonicalPlayerCombat,
    /// The first tick the player may speak again.
    pub chat_ready_at: u64,
    /// Chat lines the player heard this tick, at most [`crate::MAX_CHAT_PER_TICK`].
    pub chat: Vec<crate::ChatLine>,
    /// Active and turned-in quests; validated against the content.
    pub quests: crate::QuestLog,
    /// The tick the quest log last changed.
    pub quests_changed_at: u64,
}

/// A creature; `position` is its body or corpse, zero once despawned, and
/// `velocity` is zero unless it is alive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalCreatureSnapshot {
    pub creature_id: CreatureId,
    pub level: u8,
    pub health: u32,
    pub facing: u16,
    pub position: [i32; 3],
    pub velocity: [i32; 3],
    pub life: CreatureLife,
    pub ai: CreatureAi,
    /// Ordered by entity.
    pub threat: Vec<ThreatEntry>,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub tapped_by: Option<PlayerId>,
    pub loot: Option<crate::LootRewards>,
    pub abilities: CanonicalCreatureAbilities,
}

/// Everything authoritative about a zone at a tick boundary, with its
/// content referenced by identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalZoneSnapshot {
    pub schema_version: u16,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub content_revision: u64,
    pub content_fingerprint: u64,
    pub rng_state: u64,
    pub loot_rng_state: u64,
    /// Ordered by player ID.
    pub players: Vec<CanonicalPlayerSnapshot>,
    /// One record per content spawn, in creature-ID order.
    pub creatures: Vec<CanonicalCreatureSnapshot>,
}

impl ZoneSimulation {
    pub fn snapshot(&self) -> Result<CanonicalZoneSnapshot, ZoneError> {
        let players = self
            .players
            .iter()
            .map(|(&player_id, state)| {
                let body = self
                    .world
                    .body(body_id(EntityRef::Player(player_id)))
                    .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
                let (position, velocity) = (body.position(), body.velocity());
                Ok(CanonicalPlayerSnapshot {
                    player_id,
                    position: [position.x, position.y, position.z],
                    velocity: [velocity.x, velocity.y, velocity.z],
                    facing: state.facing,
                    forward: state.forward.get(),
                    strafe: state.strafe.get(),
                    jump_pending: state.jump_pending,
                    last_sequence: state.last_sequence,
                    spawn_slot: state.spawn_slot,
                    copper: state.copper,
                    inventory: state.inventory.clone(),
                    inventory_revision: state.inventory_revision,
                    inventory_changed_at: state.inventory_changed_at,
                    equipment: state.equipment,
                    combat: CanonicalPlayerCombat {
                        level: state.level,
                        experience: state.experience,
                        health: state.health,
                        target: state.target,
                        auto_attack: state.auto_attack,
                        swing_timer: state.swing_timer,
                        combat_timer: state.combat_timer,
                        calm_ticks: state.calm_ticks,
                        error_cooldown: state.error_cooldown,
                        intents: state.intents.clone(),
                        intents_dropped: state.intents_dropped,
                        events: state.events.clone(),
                        abilities: CanonicalPlayerAbilities {
                            class: state.class,
                            resource: state.resource.value,
                            resource_ticks: state.resource.ticks,
                            mana_delay: state.resource.delay,
                            global_cooldown: state.global_cooldown,
                            cooldowns: state.cooldowns.clone(),
                            cast: state.cast,
                            auras: state.auras.clone(),
                        },
                    },
                    chat_ready_at: state.chat_ready_at,
                    chat: state.chat.clone(),
                    quests: state.quests.clone(),
                    quests_changed_at: state.quests_changed_at,
                })
            })
            .collect::<Result<Vec<_>, ZoneError>>()?;
        let creatures = self
            .creatures
            .iter()
            .map(|(&creature_id, creature)| {
                let (position, velocity, life) = match creature.life {
                    Life::Alive => {
                        let body = self
                            .world
                            .body(body_id(EntityRef::Creature(creature_id)))
                            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
                        let (position, velocity) = (body.position(), body.velocity());
                        (
                            [position.x, position.y, position.z],
                            [velocity.x, velocity.y, velocity.z],
                            CreatureLife::Alive,
                        )
                    }
                    Life::Corpse { died_at, position } => {
                        (position, [0; 3], CreatureLife::Corpse { died_at })
                    }
                    Life::Despawned { died_at } => {
                        ([0; 3], [0; 3], CreatureLife::Despawned { died_at })
                    }
                };
                Ok(CanonicalCreatureSnapshot {
                    creature_id,
                    level: creature.level,
                    health: creature.health,
                    facing: creature.facing,
                    position,
                    velocity,
                    life,
                    ai: creature.ai,
                    threat: creature.threat.clone(),
                    swing_timer: creature.swing_timer,
                    combat_timer: creature.combat_timer,
                    tapped_by: creature.tapped_by,
                    loot: creature.loot,
                    abilities: CanonicalCreatureAbilities {
                        ability_timer: creature.ability_timer,
                        cast: creature.cast,
                        auras: creature.auras.clone(),
                    },
                })
            })
            .collect::<Result<Vec<_>, ZoneError>>()?;
        Ok(CanonicalZoneSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: self.zone_id,
            tick: self.tick,
            content_revision: self.content.revision(),
            content_fingerprint: self.content.fingerprint(),
            rng_state: self.rng.state(),
            loot_rng_state: self.loot_rng.state(),
            players,
            creatures,
        })
    }

    /// Restores a zone from a canonical snapshot and the content it was
    /// taken with. Any content identity mismatch, schema mismatch or
    /// inconsistent state fails closed; the restored zone continues exactly.
    pub fn from_snapshot(
        snapshot: CanonicalZoneSnapshot,
        content: Arc<ZoneContent>,
    ) -> Result<Self, ZoneError> {
        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(ZoneError::new("unsupported snapshot schema version"));
        }
        if snapshot.content_revision != content.revision()
            || snapshot.content_fingerprint != content.fingerprint()
        {
            return Err(ZoneError::new(
                "snapshot content identity does not match the supplied zone content",
            ));
        }
        if snapshot.players.len() > MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }
        let mut zone = Self::without_creatures(snapshot.zone_id, content)?;
        zone.tick = snapshot.tick;
        zone.rng = ZoneRng::from_state(snapshot.rng_state);
        zone.loot_rng = ZoneRng::from_state(snapshot.loot_rng_state);
        let mut spawn_slots = BTreeSet::new();
        for player in snapshot.players {
            zone.restore_player(player, &mut spawn_slots)?;
        }
        if snapshot.creatures.len() != zone.content.creature_spawns().len() {
            return Err(ZoneError::new(
                "snapshot creatures do not match the content spawns",
            ));
        }
        for record in snapshot.creatures {
            zone.restore_creature(record)?;
        }
        zone.validate_references()?;
        zone.rebuild_interest()?;
        Ok(zone)
    }

    fn restore_player(
        &mut self,
        player: CanonicalPlayerSnapshot,
        spawn_slots: &mut BTreeSet<u16>,
    ) -> Result<(), ZoneError> {
        let forward = Axis::new(player.forward)?;
        let strafe = Axis::new(player.strafe)?;
        if usize::from(player.spawn_slot) >= MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("player spawn slot is out of range"));
        }
        if self.players.contains_key(&player.player_id) {
            return Err(ZoneError::new("snapshot contains duplicate player"));
        }
        if !spawn_slots.insert(player.spawn_slot) {
            return Err(ZoneError::new("snapshot contains duplicate spawn slot"));
        }
        let combat = player.combat;
        if player.inventory_revision == 0 || player.inventory_changed_at > self.tick {
            return Err(ZoneError::new(
                "player inventory revision or change tick is invalid",
            ));
        }
        let max_health = player_max_health(combat.level) + player.equipment.totals().bonus_health();
        if !crate::progression::valid_progression(combat.level, combat.experience)
            || combat.health > max_health
        {
            return Err(ZoneError::new("player unit state is out of range"));
        }
        if combat.intents.len() > MAX_PENDING_INTENTS || combat.events.len() > MAX_EVENTS_PER_PLAYER
        {
            return Err(ZoneError::new("player queues exceed their capacity"));
        }
        if player.chat.len() > crate::MAX_CHAT_PER_TICK
            || player.chat_ready_at > self.tick.saturating_add(crate::CHAT_INTERVAL_TICKS)
        {
            return Err(ZoneError::new("player chat state is out of range"));
        }
        if player.quests_changed_at > self.tick {
            return Err(ZoneError::new("player quest change tick is invalid"));
        }
        crate::quest::validate_log(&self.content, &player.quests)?;
        if combat.intents_dropped && combat.intents.len() != MAX_PENDING_INTENTS {
            return Err(ZoneError::new("only a full intent queue drops intents"));
        }
        if combat.health == 0 && combat.auto_attack {
            return Err(ZoneError::new("a dead player cannot auto-attack"));
        }
        // Dead players hold no movement. Earlier builds kept it at death, so a
        // v11 checkpoint may still carry it: recovery lets go of it, exactly as
        // death now does, instead of refusing a checkpoint of the same format.
        let alive = combat.health > 0;
        let (forward, strafe, jump_pending) = if alive {
            (forward, strafe, player.jump_pending)
        } else {
            (Axis::Zero, Axis::Zero, false)
        };
        if combat.abilities.class.is_some() && self.content.ability_revision() == 0 {
            return Err(ZoneError::new(
                "a class player needs content that binds the ability catalog",
            ));
        }
        validate_player_abilities(&combat.abilities, combat.level, combat.health > 0)?;
        self.world
            .add_body(RigidBody::dynamic(
                body_id(EntityRef::Player(player.player_id)),
                vector(player.position),
                vector(player.velocity),
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(physics_error)?;
        self.players.insert(
            player.player_id,
            PlayerState {
                facing: player.facing,
                forward,
                strafe,
                jump_pending,
                last_sequence: player.last_sequence,
                spawn_slot: player.spawn_slot,
                copper: player.copper,
                inventory: player.inventory,
                inventory_revision: player.inventory_revision,
                inventory_changed_at: player.inventory_changed_at,
                equipment: player.equipment,
                level: combat.level,
                experience: combat.experience,
                health: combat.health,
                target: combat.target,
                auto_attack: combat.auto_attack,
                swing_timer: combat.swing_timer,
                combat_timer: combat.combat_timer,
                calm_ticks: combat.calm_ticks,
                error_cooldown: combat.error_cooldown,
                intents: combat.intents,
                intents_dropped: combat.intents_dropped,
                events: combat.events,
                class: combat.abilities.class,
                resource: ResourceClock {
                    value: combat.abilities.resource,
                    ticks: combat.abilities.resource_ticks,
                    delay: combat.abilities.mana_delay,
                },
                global_cooldown: combat.abilities.global_cooldown,
                cooldowns: combat.abilities.cooldowns,
                cast: combat.abilities.cast,
                auras: combat.abilities.auras,
                chat_ready_at: player.chat_ready_at,
                chat: player.chat,
                quests: player.quests,
                quests_changed_at: player.quests_changed_at,
            },
        );
        Ok(())
    }

    fn restore_creature(&mut self, record: CanonicalCreatureSnapshot) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let expected = content
            .creature_spawns()
            .get(self.creatures.len())
            .map(|spawn| spawn.id);
        if expected != Some(record.creature_id) {
            return Err(ZoneError::new(
                "snapshot creatures do not match the content spawns",
            ));
        }
        let (spawn, template) = Self::creature_content(&content, record.creature_id)?;
        // Bodies and corpses stay inside the world limits and wander points
        // near their spawn, so AI arithmetic stays in range.
        let wanders_off = matches!(
            record.ai,
            CreatureAi::Idle { destination: Some(to), .. }
                if !is_wander_point(spawn.position, to, spawn.wander_radius)
        );
        if !record.position.into_iter().all(within_content_range) || wanders_off {
            return Err(ZoneError::new(
                "creature position or wander destination is out of range",
            ));
        }
        if !(template.min_level..=template.max_level).contains(&record.level)
            || record.health > template.max_health(record.level)
            || record.threat.len() > MAX_THREAT_ENTRIES
            || record
                .threat
                .windows(2)
                .any(|pair| pair[0].entity >= pair[1].entity)
        {
            return Err(ZoneError::new("creature state is out of range"));
        }
        let at_rest = record.ai
            == CreatureAi::Idle {
                timer: 0,
                destination: None,
            }
            && record.threat.is_empty()
            && record.swing_timer == 0
            && record.combat_timer == 0
            && record.velocity == [0; 3]
            && record.health == 0;
        let life = match record.life {
            CreatureLife::Alive => {
                if record.health == 0 {
                    return Err(ZoneError::new("a living creature needs health"));
                }
                self.world
                    .add_body(RigidBody::dynamic(
                        body_id(EntityRef::Creature(record.creature_id)),
                        vector(record.position),
                        vector(record.velocity),
                        vector(template.half_extents),
                    ))
                    .map_err(physics_error)?;
                Life::Alive
            }
            CreatureLife::Corpse { died_at } if at_rest && died_at <= self.tick => Life::Corpse {
                died_at,
                position: record.position,
            },
            CreatureLife::Despawned { died_at }
                if at_rest && died_at <= self.tick && record.position == [0; 3] =>
            {
                Life::Despawned { died_at }
            }
            CreatureLife::Corpse { .. } | CreatureLife::Despawned { .. } => {
                return Err(ZoneError::new("dead creature state is inconsistent"));
            }
        };
        if let Some(rewards) = record.loot {
            let valid_corpse = matches!(record.life, CreatureLife::Corpse { died_at }
                if self.tick < died_at.saturating_add(u64::from(crate::unit::CORPSE_TICKS)));
            let valid_rewards = content.loot_table(spawn.template).map_or(
                rewards.money == 0 && rewards.item.is_none(),
                |table| {
                    let [min, max] = table.money_range();
                    (min..=max).contains(&rewards.money)
                        && table
                            .outcomes()
                            .iter()
                            .any(|outcome| match (outcome, rewards.item) {
                                (crate::LootOutcome::Nothing { .. }, None) => true,
                                (crate::LootOutcome::Item { item, quantity, .. }, Some(stack)) => {
                                    stack.item() == *item
                                        && (quantity[0]..=quantity[1]).contains(&stack.quantity())
                                }
                                _ => false,
                            })
                },
            );
            let valid_quest_item = rewards.quest_item.is_none_or(|stack| {
                stack.quantity() == 1
                    && crate::quest::drops_quest_item(&content, spawn.template, stack.item())
            });
            if !valid_corpse || record.tapped_by.is_none() || !valid_rewards || !valid_quest_item {
                return Err(ZoneError::new(
                    "corpse loot is inconsistent with life, owner or content",
                ));
            }
        }
        if !matches!(record.ai, CreatureAi::Engaged) && !record.threat.is_empty() {
            return Err(ZoneError::new("only engaged creatures have threat"));
        }
        validate_creature_abilities(
            &record.abilities,
            content.creature_ability(spawn.template),
            record.life == CreatureLife::Alive,
            record.ai == CreatureAi::Engaged,
        )?;
        self.creatures.insert(
            record.creature_id,
            CreatureState {
                level: record.level,
                health: record.health,
                facing: record.facing,
                life,
                ai: record.ai,
                threat: record.threat,
                swing_timer: record.swing_timer,
                combat_timer: record.combat_timer,
                tapped_by: record.tapped_by,
                loot: record.loot,
                ability_timer: record.abilities.ability_timer,
                cast: record.abilities.cast,
                auras: record.abilities.auras,
            },
        );
        Ok(())
    }

    /// Player selections and threat tables name units that exist; threat
    /// tables hold only living players.
    fn validate_references(&self) -> Result<(), ZoneError> {
        let exists = |entity: EntityRef| match entity {
            EntityRef::Player(player_id) => self.players.contains_key(&player_id),
            EntityRef::Creature(creature_id) => self.creatures.contains_key(&creature_id),
            EntityRef::Npc(npc_id) => self.content.npc(npc_id).is_some(),
        };
        let unit_refs_exist = |cast: Option<CastState>, auras: &[Aura]| {
            cast.and_then(|cast| cast.target).is_none_or(exists)
                && auras.iter().all(|aura| exists(aura.caster))
        };
        // Players cast at creatures and creatures at players.
        let player_targets = self
            .players
            .values()
            .filter_map(|player| player.cast?.target);
        let creature_targets = self
            .creatures
            .values()
            .filter_map(|creature| creature.cast?.target);
        if player_targets
            .into_iter()
            .any(|target| !matches!(target, EntityRef::Creature(_)))
            || creature_targets
                .clone()
                .any(|target| !matches!(target, EntityRef::Player(_)))
        {
            return Err(ZoneError::new("a cast names a target of the wrong kind"));
        }
        // A player's death removes every creature cast aimed at them.
        if creature_targets.into_iter().any(|target| {
            let EntityRef::Player(player_id) = target else {
                return false;
            };
            self.players
                .get(&player_id)
                .is_some_and(|player| !player.is_alive())
        }) {
            return Err(ZoneError::new("a creature casts at a dead player"));
        }
        for player in self.players.values() {
            if player.target.is_some_and(|target| !exists(target)) {
                return Err(ZoneError::new("player target does not exist"));
            }
            if !unit_refs_exist(player.cast, &player.auras) {
                return Err(ZoneError::new(
                    "a cast target or aura caster does not exist",
                ));
            }
        }
        for creature in self.creatures.values() {
            if !unit_refs_exist(creature.cast, &creature.auras) {
                return Err(ZoneError::new(
                    "a cast target or aura caster does not exist",
                ));
            }
        }
        // Every aura is reachable: a living player cast it with a learned
        // ability, on itself for self-centred auras and on a creature otherwise.
        let reachable = |unit: EntityRef, aura: &Aura| {
            let EntityRef::Player(caster_id) = aura.caster else {
                return false;
            };
            let learned = self.players.get(&caster_id).is_some_and(|caster| {
                caster.is_alive()
                    && caster.class.is_some_and(|choice| {
                        learned(choice.class, caster.level, aura.ability).is_some()
                    })
            });
            let on_caster =
                ability_by_id(aura.ability).is_some_and(|ability| ability.aura_on_caster());
            learned
                && if on_caster {
                    unit == aura.caster
                } else {
                    matches!(unit, EntityRef::Creature(_))
                }
        };
        let units = self
            .players
            .iter()
            .map(|(&id, player)| (EntityRef::Player(id), &player.auras))
            .chain(
                self.creatures
                    .iter()
                    .map(|(&id, creature)| (EntityRef::Creature(id), &creature.auras)),
            );
        for (unit, auras) in units {
            if !auras.iter().all(|aura| reachable(unit, aura)) {
                return Err(ZoneError::new(
                    "an aura's caster or recipient is unreachable",
                ));
            }
        }
        for creature in self.creatures.values() {
            if creature
                .tapped_by
                .is_some_and(|player| !self.players.contains_key(&player))
            {
                return Err(ZoneError::new("creature tap names an absent player"));
            }
            for entry in &creature.threat {
                let EntityRef::Player(player_id) = entry.entity else {
                    return Err(ZoneError::new("only players can be on a threat table"));
                };
                if !self
                    .players
                    .get(&player_id)
                    .is_some_and(PlayerState::is_alive)
                {
                    return Err(ZoneError::new("threat table names an absent player"));
                }
            }
        }
        Ok(())
    }
}

/// Auras on a recovered unit: bounded, from catalog abilities that apply an
/// aura, with time left within the duration and the amount their kind fixes.
fn validate_auras(auras: &[Aura]) -> Result<(), ZoneError> {
    if auras.len() > MAX_AURAS {
        return Err(ZoneError::new("unit has too many auras"));
    }
    for (index, aura) in auras.iter().enumerate() {
        let Some(spec) = aura.spec() else {
            return Err(ZoneError::new("aura names an ability without an aura"));
        };
        // Fixed amounts must match; variable ones lie in 1..= the largest
        // amount any player level reaches (an absorb only shrinks).
        let top = crate::progression::MAX_PLAYER_LEVEL;
        let valid_amount = match ability_by_id(aura.ability).map(|ability| ability.effect) {
            Some(
                crate::AbilityEffect::Snare { percent, .. }
                | crate::AbilityEffect::Haste { percent, .. },
            ) => aura.amount == percent,
            Some(
                crate::AbilityEffect::DamageOverTime {
                    base, per_level, ..
                }
                | crate::AbilityEffect::Absorb {
                    base, per_level, ..
                },
            ) => (1..=crate::ability::scaled(base, per_level, top)).contains(&aura.amount),
            Some(crate::AbilityEffect::HealOverTime { percent, .. }) => {
                let most_health = player_max_health(top)
                    + u32::from(crate::MAX_EQUIPPED_STAMINA) * crate::HEALTH_PER_STAMINA;
                let most = crate::unit::percent_of(most_health, u32::from(percent));
                aura.amount > 0 && u32::from(aura.amount) <= most
            }
            _ => matches!(spec.kind, AuraKind::Root | AuraKind::Stun) && aura.amount == 0,
        };
        if aura.remaining == 0 || aura.remaining > spec.duration || !valid_amount {
            return Err(ZoneError::new("aura state is out of range"));
        }
        if auras[..index]
            .iter()
            .any(|other| other.ability == aura.ability && other.caster == aura.caster)
        {
            return Err(ZoneError::new("a caster's aura of one ability is unique"));
        }
    }
    Ok(())
}

/// A recovered cast: `ability` casts or channels, the elapsed time stays
/// below its cast time, a hostile ability has a target and only a channel
/// keeps a target point.
fn validate_cast(cast: CastState) -> Result<(), ZoneError> {
    let valid = ability_by_id(cast.ability).is_some_and(|ability| {
        cast.elapsed < ability.cast.ticks()
            && cast.point.is_some() == ability.cast.is_channel()
            && cast.target.is_some() == ability.needs_target()
            && cast
                .point
                .is_none_or(|point| point.into_iter().all(within_content_range))
    });
    if valid {
        Ok(())
    } else {
        Err(ZoneError::new("cast state is out of range"))
    }
}

fn validate_player_abilities(
    abilities: &CanonicalPlayerAbilities,
    level: u8,
    alive: bool,
) -> Result<(), ZoneError> {
    let Some(ClassChoice { class, .. }) = abilities.class else {
        if *abilities != CanonicalPlayerAbilities::default() {
            return Err(ZoneError::new(
                "a player without a class has no ability state",
            ));
        }
        return Ok(());
    };
    let clock = ResourceClock {
        value: abilities.resource,
        ticks: abilities.resource_ticks,
        delay: abilities.mana_delay,
    };
    let cooldowns_valid = abilities.cooldowns.len() <= MAX_COOLDOWNS
        && abilities
            .cooldowns
            .windows(2)
            .all(|pair| pair[0].ability < pair[1].ability)
        && abilities.cooldowns.iter().all(|cooldown: &Cooldown| {
            learned(class, level, cooldown.ability).is_some_and(|ability| {
                cooldown.remaining > 0 && cooldown.remaining <= ability.cooldown
            })
        });
    if !clock.is_valid(class.resource(), level)
        || abilities.global_cooldown > GLOBAL_COOLDOWN_TICKS
        || !cooldowns_valid
    {
        return Err(ZoneError::new("player ability state is out of range"));
    }
    if let Some(cast) = abilities.cast {
        if learned(class, level, cast.ability).is_none() {
            return Err(ZoneError::new("cast state is out of range"));
        }
        validate_cast(cast)?;
    }
    validate_auras(&abilities.auras)?;
    if !alive && (abilities.cast.is_some() || !abilities.auras.is_empty()) {
        return Err(ZoneError::new("the dead neither cast nor keep auras"));
    }
    // Death drains rage and its decay clock.
    if !alive
        && class.resource() == crate::ResourceKind::Rage
        && (abilities.resource != 0 || abilities.resource_ticks != 0)
    {
        return Err(ZoneError::new("a dead Warden has no rage"));
    }
    Ok(())
}

fn validate_creature_abilities(
    abilities: &CanonicalCreatureAbilities,
    bound: Option<AbilityId>,
    alive: bool,
    engaged: bool,
) -> Result<(), ZoneError> {
    if !alive && *abilities != CanonicalCreatureAbilities::default() {
        return Err(ZoneError::new("dead creature state is inconsistent"));
    }
    let timer_limit = bound.and_then(ability_by_id).map_or(0, |ability| {
        (ability.cooldown + CREATURE_ABILITY_JITTER_TICKS)
            .max(crate::casting::SHIELD_BASH_LOCKOUT_TICKS)
    });
    if abilities.ability_timer > timer_limit {
        return Err(ZoneError::new("creature ability timer is out of range"));
    }
    if let Some(cast) = abilities.cast {
        if Some(cast.ability) != bound || !engaged {
            return Err(ZoneError::new("cast state is out of range"));
        }
        validate_cast(cast)?;
    }
    validate_auras(&abilities.auras)
}
