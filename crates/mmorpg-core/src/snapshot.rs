//! Canonical snapshots: every authoritative field needed to continue a zone
//! exactly, for trusted replay and recovery. Content is referenced by its
//! revision (a content hash), never embedded: recovery needs the identical
//! [`ZoneContent`] and fails closed on any mismatch.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::physics::RigidBody;

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
use crate::zone::{Axis, MAX_PENDING_INTENTS, PLAYER_HALF_EXTENTS, PlayerState, physics_error};
use crate::{
    Aura, CastState, ClassChoice, Cooldown, CreatureId, EntityRef, MAX_EVENTS_PER_PLAYER,
    MAX_PLAYERS_PER_ZONE, PlayerId, ZoneContent, ZoneError, ZoneEvent, ZoneId, ZoneSimulation,
};

/// A queued discrete intent, consumed by the next tick in sequence order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerIntent {
    SelectTarget(Option<EntityRef>),
    Loot(crate::LootClaim),
    /// A weapon swing (#517) toward `facing`, resolved by the next tick or once the swing
    /// timer allows.
    Swing {
        facing: u16,
    },
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
    /// A directed strike (#154), resolved by the next tick.
    DirectedStrike {
        ability: u8,
        facing: u16,
        aim: crate::StrikeAim,
    },
    /// Holds or releases the shield guard (#156), in order with strikes so
    /// the last-pressed gesture wins.
    Guard(Option<crate::StrikeHeight>),
    UseDoor {
        door: crate::DoorId,
        open: bool,
    },
    /// Starts or stops repairing a destructible piece (#305).
    Repair(Option<crate::PieceId>),
    CancelCast,
    /// A group order for the player's minions (#295).
    CommandMinions(crate::MinionOrder),
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
    /// Waystone travel (#145).
    UseWaystone(crate::WaystoneId),
}

/// A pending intent and the ticks of lateness its command's observation
/// reported (Atlas 5.9.6, #415): at most
/// [`crate::action::LATE_INPUT_GRACE_TICKS`], 0 without an observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedIntent {
    pub intent: PlayerIntent,
    pub late: u8,
}

impl From<PlayerIntent> for QueuedIntent {
    /// An intent that is not late.
    fn from(intent: PlayerIntent) -> Self {
        Self { intent, late: 0 }
    }
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
    /// Holding a boss's rally `point` for `ticks` more ticks after the boss fell (#276).
    Rallying { ticks: u16, point: [i32; 2] },
    /// An aquatic creature following submerged `player` for `ticks` more ticks, without
    /// combat (#274).
    Trailing { player: crate::PlayerId, ticks: u16 },
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
    /// A pressed weapon swing (#517) waiting for the swing timer, a stun or a held landing.
    pub swing_queued: bool,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub calm_ticks: u16,
    /// Queued intents in sequence order.
    pub intents: Vec<QueuedIntent>,
    /// A later intent found the queue full and was dropped; the next tick
    /// reports it. Only a full queue drops intents.
    pub intents_dropped: bool,
    /// The latest zone tick a command of the player reported having observed
    /// (#415); a later observation may not be older. 0 before the first.
    pub last_observed: u64,
    /// Events of the current tick.
    pub events: Vec<ZoneEvent>,
    pub abilities: CanonicalPlayerAbilities,
    /// Airborne state and landing recovery (#155).
    pub landing: crate::LandingState,
    /// A climb or mantle in progress (#490).
    pub traversal: crate::Traversal,
    /// A pending press, a dodge roll in progress or its cooldown (#516).
    pub roll: crate::Roll,
    /// A pending sword → Shield Bash follow-up (#157).
    pub combo: Option<crate::ComboOpening>,
    /// Ticks left of a directed strike's committed recovery (#156).
    pub strike_recovery: u16,
    /// The destructible piece the player is repairing (#305).
    pub repairing: Option<crate::PieceId>,
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
    /// The held shield guard and its break recovery (#156).
    pub guard: crate::GuardState,
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
            swing_queued: false,
            swing_timer: 0,
            combat_timer: 0,
            calm_ticks: 0,
            intents: Vec::new(),
            intents_dropped: false,
            last_observed: 0,
            events: Vec::new(),
            abilities: CanonicalPlayerAbilities::default(),
            landing: crate::LandingState::default(),
            traversal: crate::Traversal::Free,
            roll: crate::Roll::Ready,
            combo: None,
            strike_recovery: 0,
            repairing: None,
        }
    }
}

/// `position` (body centre, units) and `velocity` (units per tick) are the exact `f32`
/// physics state (#271), so recovery continues exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalPlayerSnapshot {
    pub player_id: PlayerId,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub facing: u16,
    pub forward: i8,
    pub strafe: i8,
    pub jump_pending: bool,
    /// Held swim ascent or descent, `-1..=1` (#274).
    pub rise: i8,
    /// Breath and drowning clock (#274).
    pub breath: crate::Breath,
    pub last_sequence: u32,
    pub spawn_slot: u16,
    pub copper: u32,
    pub inventory: crate::Inventory,
    pub inventory_revision: u64,
    /// Equipped items; validated against the catalog by construction.
    pub equipment: crate::Equipment,
    pub combat: CanonicalPlayerCombat,
    /// The first tick the player may speak again.
    pub chat_ready_at: u64,
    /// Chat lines the player heard this tick, at most [`crate::MAX_CHAT_PER_TICK`].
    pub chat: Vec<crate::ChatLine>,
    /// Active and turned-in quests; validated against the content.
    pub quests: crate::QuestLog,
    /// Waystones unlocked by exploration (#145).
    pub waystones: crate::WaystoneSet,
}

/// A creature; `position` is its body or corpse, zero once despawned, and
/// `velocity` is zero unless it is alive.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalCreatureSnapshot {
    pub generation: u64,
    pub creature_id: CreatureId,
    pub level: u8,
    pub health: u32,
    pub facing: u16,
    /// Exact `f32` body state while alive; a corpse's whole-unit position.
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub life: CreatureLife,
    pub ai: CreatureAi,
    /// Ordered by entity.
    pub threat: Vec<ThreatEntry>,
    pub swing_timer: u16,
    pub combat_timer: u16,
    pub tapped_by: Option<PlayerId>,
    pub loot: Option<crate::LootRewards>,
    pub abilities: CanonicalCreatureAbilities,
    /// A fighting boss's attempt (#276): locked participants and contributors.
    pub encounter: Option<crate::BossEncounter>,
    /// A night raider's incident and march (#530).
    pub raid: Option<crate::RaidMember>,
}

/// Everything authoritative about a zone at a tick boundary, with its
/// content referenced by identity.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalZoneSnapshot {
    pub rewards: Option<crate::RewardHistory>,
    pub zone_id: ZoneId,
    pub tick: u64,
    pub content_revision: u64,
    pub rng_state: u64,
    pub loot_rng_state: u64,
    /// Ordered by player ID.
    pub players: Vec<CanonicalPlayerSnapshot>,
    /// One record per content spawn, in creature-ID order.
    pub creatures: Vec<CanonicalCreatureSnapshot>,
    /// One state per content door, in door-ID order (#158).
    pub doors: Vec<crate::CanonicalDoor>,
    /// Flying and stuck projectiles in ID order, and the next projectile ID (#286).
    pub projectiles: Vec<crate::Projectile>,
    pub next_projectile: u32,
    /// Forced moves in progress and immunities, in unit order (#287).
    pub forced: Vec<(crate::EntityRef, crate::ForcedState)>,
    /// Pieces of broken props in ID order, and the next piece ID (#288).
    pub debris: Vec<crate::Debris>,
    pub next_debris: u32,
    /// Every destructible piece's health and last change, in content order (#305).
    pub pieces: Vec<crate::CanonicalPiece>,
    /// Summoned minions in ID order, and the next minion ID (#295).
    pub minions: Vec<crate::CanonicalMinion>,
    pub next_minion: u32,
    /// The night raid (#530).
    pub raid: crate::RaidState,
    /// Private-home residents a knock woke, in NPC-ID order (#158).
    pub wakes: Vec<crate::NpcWake>,
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
                Ok(CanonicalPlayerSnapshot {
                    player_id,
                    position: body.exact_position(),
                    velocity: body.exact_velocity(),
                    facing: state.facing,
                    forward: state.forward.get(),
                    strafe: state.strafe.get(),
                    jump_pending: state.jump_pending,
                    rise: state.rise.get(),
                    breath: state.breath,
                    last_sequence: state.last_sequence,
                    spawn_slot: state.spawn_slot,
                    copper: state.copper,
                    inventory: state.inventory.clone(),
                    inventory_revision: state.inventory_revision,
                    equipment: state.equipment,
                    combat: CanonicalPlayerCombat {
                        level: state.level,
                        experience: state.experience,
                        health: state.health,
                        target: state.target,
                        swing_queued: state.swing_queued,
                        swing_timer: state.swing_timer,
                        combat_timer: state.combat_timer,
                        calm_ticks: state.calm_ticks,
                        intents: state.intents.clone(),
                        intents_dropped: state.intents_dropped,
                        last_observed: state.last_observed,
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
                            guard: state.guard,
                        },
                        landing: state.landing,
                        traversal: state.traversal,
                        roll: state.roll,
                        combo: state.combo,
                        strike_recovery: state.strike_recovery,
                        repairing: state.repairing,
                    },
                    chat_ready_at: state.chat_ready_at,
                    chat: state.chat.clone(),
                    quests: state.quests.clone(),
                    waystones: state.waystones,
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
                        // A movable prop's exact slide state (#58).
                        let (position, velocity) = self
                            .prop_motion_of(creature_id)
                            .map_or((body.exact_position(), body.exact_velocity()), |motion| {
                                (motion.position, motion.velocity)
                            });
                        (position, velocity, CreatureLife::Alive)
                    }
                    Life::Corpse { died_at, position } => (
                        position.map(|axis| axis as f32),
                        [0.0; 3],
                        CreatureLife::Corpse { died_at },
                    ),
                    Life::Despawned { died_at } => {
                        ([0.0; 3], [0.0; 3], CreatureLife::Despawned { died_at })
                    }
                };
                Ok(CanonicalCreatureSnapshot {
                    generation: creature.generation,
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
                    encounter: creature.encounter.clone(),
                    raid: creature.raid,
                })
            })
            .collect::<Result<Vec<_>, ZoneError>>()?;
        Ok(CanonicalZoneSnapshot {
            rewards: self.rewards.clone(),
            zone_id: self.zone_id,
            tick: self.tick,
            content_revision: self.content.revision(),
            rng_state: self.rng.state(),
            loot_rng_state: self.loot_rng.state(),
            players,
            creatures,
            doors: self.canonical_doors(),
            projectiles: self.projectiles.values().copied().collect(),
            next_projectile: self.next_projectile,
            forced: self.forced_states(),
            debris: self.debris.values().copied().collect(),
            next_debris: self.next_debris,
            pieces: self.canonical_pieces(),
            minions: self.canonical_minions()?,
            next_minion: self.next_minion,
            raid: self.raid.clone(),
            wakes: self.canonical_wakes(),
        })
    }

    /// Restores a zone from a canonical snapshot and the content it was
    /// taken with. Any content identity mismatch or inconsistent state fails
    /// closed; the restored zone continues exactly.
    pub fn from_snapshot(
        snapshot: CanonicalZoneSnapshot,
        content: Arc<ZoneContent>,
    ) -> Result<Self, ZoneError> {
        if snapshot.content_revision != content.revision() {
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
        snapshot.raid.validate(snapshot.tick)?;
        if zone.content.raid().is_none() && snapshot.raid != crate::RaidState::default() {
            return Err(ZoneError::new("snapshot raid state needs a raid site"));
        }
        zone.raid = snapshot.raid;
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
        zone.restore_minions(snapshot.minions, snapshot.next_minion)?;
        let doors = zone.content.doors().to_vec();
        if snapshot.doors.len() != doors.len()
            || snapshot
                .doors
                .iter()
                .zip(&doors)
                .any(|(view, door)| view.door != door.id)
        {
            return Err(ZoneError::new(
                "snapshot doors do not match the content doors",
            ));
        }
        zone.restore_pieces(&snapshot.pieces, snapshot.tick)?;
        for (view, door) in snapshot.doors.iter().zip(&doors) {
            if view.changed_tick > snapshot.tick {
                return Err(ZoneError::new(
                    "snapshot door changed after the snapshot tick",
                ));
            }
            if view.open || view.changed_tick != 0 {
                zone.set_door(door, view.open, view.changed_tick)?;
            }
        }
        if snapshot.projectiles.len() > crate::MAX_PROJECTILES
            || snapshot
                .projectiles
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || snapshot.projectiles.iter().any(|projectile| {
                crate::ability_by_id(projectile.ability).is_none()
                    || projectile
                        .position
                        .iter()
                        .chain(&projectile.velocity)
                        .any(|value| !value.is_finite())
            })
        {
            return Err(ZoneError::new("snapshot projectiles are invalid"));
        }
        zone.projectiles = snapshot
            .projectiles
            .into_iter()
            .map(|projectile| (projectile.id, projectile))
            .collect();
        zone.next_projectile = snapshot.next_projectile;
        zone.restore_forced(snapshot.forced)?;
        zone.restore_debris(snapshot.debris, snapshot.next_debris)?;
        // The snapshot tick's routine placements and platforms (#56); doors are already as that
        // tick left them.
        zone.restore_wakes(&snapshot.wakes, snapshot.tick)?;
        zone.place_routine_npcs(false)?;
        zone.schedule_platforms(zone.tick, true)?;
        if let Some(rewards) = &snapshot.rewards {
            rewards.validate()?;
            if rewards
                .players
                .keys()
                .any(|player| !zone.players.contains_key(player))
                || rewards
                    .grants
                    .iter()
                    .any(|grant| !zone.players.contains_key(&grant.player))
            {
                return Err(ZoneError::new("reward history references an absent player"));
            }
            for key in rewards
                .world
                .iter()
                .chain(rewards.players.values().flatten())
            {
                if key
                    .creature_life()
                    .is_some_and(|(id, _)| !zone.creatures.contains_key(&id))
                {
                    return Err(ZoneError::new(
                        "reward history references an unknown creature",
                    ));
                }
            }
        }
        zone.rewards = snapshot.rewards;
        zone.validate_references()?;
        zone.rebuild_interest()?;
        zone.track_swimming()?;
        Ok(zone)
    }

    fn restore_player(
        &mut self,
        player: CanonicalPlayerSnapshot,
        spawn_slots: &mut BTreeSet<u16>,
    ) -> Result<(), ZoneError> {
        let forward = Axis::new(player.forward)?;
        let strafe = Axis::new(player.strafe)?;
        let rise = Axis::new(player.rise)?;
        if !player.breath.is_valid() {
            return Err(ZoneError::new("player breath is out of range"));
        }
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
        if player.inventory_revision == 0 {
            return Err(ZoneError::new("player inventory revision is invalid"));
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
        crate::quest::validate_log(&self.content, &player.quests)?;
        if combat.intents_dropped && combat.intents.len() != MAX_PENDING_INTENTS {
            return Err(ZoneError::new("only a full intent queue drops intents"));
        }
        if combat.last_observed > self.tick
            || combat
                .intents
                .iter()
                .any(|queued| queued.late > crate::action::LATE_INPUT_GRACE_TICKS)
        {
            return Err(ZoneError::new("player observation state is out of range"));
        }
        if combat.health == 0 && combat.swing_queued {
            return Err(ZoneError::new("a dead player cannot hold a queued swing"));
        }
        // Dead players hold no movement. Earlier builds kept it at death, so a
        // v11 checkpoint may still carry it: recovery lets go of it, exactly as
        // death now does, instead of refusing a checkpoint of the same format.
        let alive = combat.health > 0;
        let (forward, strafe, rise, jump_pending) = if alive {
            (forward, strafe, rise, player.jump_pending)
        } else {
            (Axis::Zero, Axis::Zero, Axis::Zero, false)
        };
        if combat.abilities.class.is_some() && !self.content.binds_abilities() {
            return Err(ZoneError::new(
                "a class player needs content that binds the ability catalog",
            ));
        }
        validate_player_abilities(&combat.abilities, combat.level, combat.health > 0)?;
        if combat
            .combo
            .is_some_and(|combo| !combo.is_valid() || combat.health == 0)
        {
            return Err(ZoneError::new("player combo state is out of range"));
        }
        if combat.strike_recovery > crate::STRIKE_RECOVERY_TICKS {
            return Err(ZoneError::new("player strike recovery is out of range"));
        }
        if combat.repairing.is_some_and(|piece| {
            !alive || !self.content.pieces().iter().any(|known| known.id == piece)
        }) {
            return Err(ZoneError::new("player repair is invalid"));
        }
        if !combat.landing.is_valid() {
            return Err(ZoneError::new("player landing state is out of range"));
        }
        if !combat.traversal.is_valid() {
            return Err(ZoneError::new("player climb or mantle is out of range"));
        }
        if !combat.roll.is_valid() || (!alive && combat.roll != crate::Roll::Ready) {
            return Err(ZoneError::new("player dodge roll is out of range"));
        }
        let guard = combat.abilities.guard;
        if guard.broken > crate::GUARD_BREAK_TICKS
            || (!alive && guard != crate::GuardState::default())
        {
            return Err(ZoneError::new("player guard state is out of range"));
        }
        self.world
            .add_body(RigidBody::moving(
                body_id(EntityRef::Player(player.player_id)),
                player.position,
                player.velocity,
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
                rise,
                breath: player.breath,
                guard,
                landing: combat.landing,
                traversal: if alive {
                    combat.traversal
                } else {
                    crate::Traversal::Free
                },
                roll: combat.roll,
                swimming: false,
                impulse: None,
                authored: None,
                holds: crate::action::authored::Holds::default(),
                combo: combat.combo,
                strike_recovery: combat.strike_recovery,
                repairing: combat.repairing,
                last_sequence: player.last_sequence,
                spawn_slot: player.spawn_slot,
                copper: player.copper,
                inventory: player.inventory,
                inventory_revision: player.inventory_revision,
                equipment: player.equipment,
                level: combat.level,
                experience: combat.experience,
                health: combat.health,
                target: combat.target,
                swing_queued: combat.swing_queued,
                swing_timer: combat.swing_timer,
                combat_timer: combat.combat_timer,
                calm_ticks: combat.calm_ticks,
                intents: combat.intents,
                intents_dropped: combat.intents_dropped,
                last_observed: combat.last_observed,
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
                waystones: player.waystones,
            },
        );
        Ok(())
    }

    fn restore_creature(&mut self, record: CanonicalCreatureSnapshot) -> Result<(), ZoneError> {
        if record.generation == 0 {
            return Err(ZoneError::new("creature life identity must be nonzero"));
        }
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
        self.validate_raid_member(&record)?;
        // Bodies and corpses stay inside the world limits and wander points
        // near their spawn (a raider's near its anchor), so AI arithmetic stays in range.
        let home = record.raid.map_or(spawn.position, |member| member.anchor);
        let wanders_off = matches!(
            record.ai,
            CreatureAi::Idle { destination: Some(to), .. }
                if !is_wander_point(home, to, spawn.wander_radius)
        );
        if !record
            .position
            .iter()
            .chain(&record.velocity)
            .all(|axis| axis.is_finite())
            || !whole_units(record.position)
                .into_iter()
                .all(within_content_range)
            || wanders_off
            || matches!(record.ai, CreatureAi::Rallying { point, .. }
                if !point.into_iter().all(within_content_range))
            || matches!(record.ai, CreatureAi::Trailing { ticks, .. }
                if ticks > crate::water::TRAIL_TICKS)
        {
            return Err(ZoneError::new(
                "creature position or wander destination is out of range",
            ));
        }
        if !(template.min_level..=template.max_level).contains(&record.level)
            || record.health > scaled_max_health(&content, template, &record)
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
            && record.velocity == [0.0; 3]
            && record.health == 0;
        let life = match record.life {
            CreatureLife::Alive => {
                if record.health == 0 {
                    return Err(ZoneError::new("a living creature needs health"));
                }
                let movable = template.prop().is_some_and(crate::PropKind::movable);
                if template.prop().is_some()
                    && !movable
                    && (record.velocity != [0.0; 3] || record.position.map(f32::fract) != [0.0; 3])
                {
                    return Err(ZoneError::new("a standing prop is fixed in whole units"));
                }
                if movable {
                    self.place_movable_prop(record.creature_id, record.position);
                    if let Some(motion) = self.prop_motion.get_mut(&record.creature_id) {
                        motion.velocity = record.velocity;
                    }
                }
                self.world
                    .add_body(crate::creature::creature_body(
                        record.creature_id,
                        template,
                        record.facing,
                        record.position,
                        record.velocity,
                    ))
                    .map_err(physics_error)?;
                Life::Alive
            }
            CreatureLife::Corpse { died_at }
                if at_rest
                    && died_at <= self.tick
                    && record.position.map(|axis| axis.fract()) == [0.0; 3] =>
            {
                Life::Corpse {
                    died_at,
                    position: whole_units(record.position),
                }
            }
            CreatureLife::Despawned { died_at }
                if at_rest && died_at <= self.tick && record.position == [0.0; 3] =>
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
        if let Some(encounter) = &record.encounter {
            let ordered = |players: &[crate::PlayerId]| {
                players.len() <= crate::MAX_ENCOUNTER_PLAYERS
                    && players.windows(2).all(|pair| pair[0] < pair[1])
            };
            if content.boss(spawn.template).is_none()
                || record.life != CreatureLife::Alive
                || record.ai != CreatureAi::Engaged
                || encounter.participants.is_empty()
                || !ordered(&encounter.participants)
                || !ordered(&encounter.contributors)
            {
                return Err(ZoneError::new("only a fighting boss has an encounter"));
            }
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
                generation: record.generation,
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
                encounter: record.encounter,
                raid: record.raid,
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
            EntityRef::Minion(minion_id) => self.minions.contains_key(&minion_id),
        };
        let unit_refs_exist = |cast: Option<CastState>, auras: &[Aura]| {
            cast.and_then(|cast| cast.target).is_none_or(exists)
                && auras.iter().all(|aura| exists(aura.caster))
        };
        if self.players.iter().any(|(&attacker, player)| {
            player.combo.is_some_and(|combo| match combo.target {
                EntityRef::Creature(id) => !self.creatures.contains_key(&id),
                EntityRef::Player(victim) => !self.players_are_hostile(attacker, victim),
                EntityRef::Npc(_) | EntityRef::Minion(_) => true,
            })
        }) {
            return Err(ZoneError::new("a combo follows up on an ineligible unit"));
        }
        // Players cast at creatures or explicitly eligible arena opponents; creatures at players.
        let player_targets = self
            .players
            .iter()
            .filter_map(|(&id, player)| Some((id, player.cast?.target?)));
        let creature_targets = self
            .creatures
            .values()
            .filter_map(|creature| creature.cast?.target);
        if player_targets
            .into_iter()
            .any(|(attacker, target)| match target {
                EntityRef::Creature(_) => false,
                EntityRef::Player(victim) => !self.players_are_hostile(attacker, victim),
                EntityRef::Npc(_) | EntityRef::Minion(_) => true,
            })
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
            let ability = ability_by_id(aura.ability);
            let on_caster = ability.is_some_and(|ability| ability.aura_on_caster());
            let friendly = ability.is_some_and(|ability| ability.is_friendly());
            learned
                && if friendly {
                    // A Cleric's heal or shield (#303) rests on the caster or a friendly player.
                    unit == aura.caster
                        || matches!(unit, EntityRef::Player(friend) if !self.players_are_hostile(caster_id, friend))
                } else if on_caster {
                    unit == aura.caster
                } else {
                    matches!(unit, EntityRef::Creature(_))
                        || matches!(unit, EntityRef::Player(victim) if self.players_are_hostile(caster_id, victim))
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
                // Minions (#295) exist only while their summoner lives.
                if let EntityRef::Minion(minion) = entry.entity
                    && self.minions.contains_key(&minion)
                {
                    continue;
                }
                let EntityRef::Player(player_id) = entry.entity else {
                    return Err(ZoneError::new(
                        "only players and minions can be on a threat table",
                    ));
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
/// below its cast time, a hostile ability has a target (unless it aims freely, #289) and only a
/// channel keeps a target point.
fn validate_cast(cast: CastState) -> Result<(), ZoneError> {
    let valid = ability_by_id(cast.ability).is_some_and(|ability| {
        cast.elapsed < ability.cast.ticks()
            && cast.point.is_some() == ability.cast.is_channel()
            && (cast.target.is_some() == ability.needs_target()
                || (cast.target.is_none() && ability.aims_freely()))
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

pub(crate) fn validate_player_abilities(
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
    // Death drains rage and faith and their decay clock.
    if !alive
        && class.resource().is_built()
        && (abilities.resource != 0 || abilities.resource_ticks != 0)
    {
        return Err(ZoneError::new("a dead player has no rage or faith"));
    }
    Ok(())
}

/// The record's maximum health under its boss attempt, if any (#276).
fn scaled_max_health(
    content: &crate::ZoneContent,
    template: &crate::CreatureTemplate,
    record: &CanonicalCreatureSnapshot,
) -> u32 {
    let base = template.max_health(record.level);
    match (content.boss(template.id), &record.encounter) {
        (Some(rules), Some(encounter)) => encounter.scale_health(rules, base),
        _ => base,
    }
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

/// An `f32` position rounded to whole units; the caller has checked it is finite.
#[allow(clippy::cast_possible_truncation)]
fn whole_units(position: [f32; 3]) -> [i32; 3] {
    position.map(|axis| axis.round() as i32)
}
