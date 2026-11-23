//! Targeting, auto-attack, damage, death and recovery of units: tick steps
//! 1 (player intents), 6 (combat resolution) and 7 (timers).
//!
//! Well-formed intents that are not allowed right now produce an
//! [`ErrorCode`] event for the player and never fail the tick.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::physics::Vec3i;
use sim_combat::swing;

use crate::action::authored::Step;
use crate::ai::{creature_reach, distance_squared};
use crate::creature::Life;
use crate::creature_store::Activity;
use crate::entity::body_id;
use crate::events::push_event;
use crate::guard::GuardOutcome;
use crate::snapshot::{CreatureAi, PlayerIntent};
use crate::unit::{
    COMBAT_LINGER_TICKS, CORPSE_TICKS, REGEN_DELAY_TICKS, REGEN_INTERVAL_TICKS, REGEN_PERCENT,
    RELEASE_HEALTH_PERCENT, SWING_BAND_UNITS, Swing, percent_of, roll_swing,
};
use crate::zone::{Axis, physics_error};
use crate::{
    CreatureId, EntityRef, ErrorCode, INTEREST_RADIUS_UNITS, PLAYER_HALF_EXTENTS_UNITS, PlayerId,
    ZoneError, ZoneEvent, ZoneSimulation,
};

/// Whether a creature's swing step would change nothing (#1086): dead, or out of a fight with
/// no timer running and no attack winding up.
fn swing_quiet(creature: &crate::creature::CreatureState) -> bool {
    !creature.is_alive()
        || (creature.swing_timer == 0
            && creature.combat_timer == 0
            && creature.attack.is_none()
            && creature.ai != CreatureAi::Engaged)
}

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
            // Intents the action graph holds or buffers wait in the queue:
            // Warden melee during landing recovery (#155), a Shield Bash
            // before the combo opens (#157).
            let mut waiting = Vec::new();
            let mut trigger_seen = false;
            for queued in intents {
                // An authored action's trigger waits out its startup (or its
                // charge), then resolves as its active phase begins (#414).
                // A refused or held input never starts one. Its lateness
                // (#415) matters only to edges and a follow-up's window.
                let step = self.authored_step(player_id, &queued, trigger_seen);
                trigger_seen |= step.is_trigger();
                match step {
                    Step::Startup | Step::Buffer => {
                        waiting.push(queued);
                        continue;
                    }
                    Step::Start(slot) | Step::Chain(slot) => {
                        if self.start_authored(player_id, slot, &queued.intent) {
                            self.release_authored(player_id, queued.intent)?;
                        } else {
                            waiting.push(queued);
                        }
                        continue;
                    }
                    Step::Release => {
                        self.release_authored(player_id, queued.intent)?;
                        continue;
                    }
                    Step::Drop => {
                        self.update_player(player_id, |player| player.authored = None);
                        continue;
                    }
                    Step::Cancel => self.update_player(player_id, |player| player.authored = None),
                    Step::Miss => self.miss_follow_up(player_id),
                    Step::Ordinary => {}
                }
                let (waits, refused) =
                    self.players
                        .get(&player_id)
                        .map_or((false, false), |player| {
                            (
                                crate::action::waits(self.action_graph, player, &queued),
                                crate::action::refuses(self.action_graph, player, &queued.intent),
                            )
                        });
                // An experimental graph may refuse an attack in water (#413).
                if refused {
                    self.notify(
                        player_id,
                        ZoneEvent::Error {
                            code: ErrorCode::NotWhileSwimming,
                            target: None,
                        },
                    );
                } else if waits {
                    waiting.push(queued);
                } else {
                    // Edges read the intent's lateness while it resolves (#415).
                    self.resolving_late = queued.late;
                    let applied = self.apply_intent(player_id, queued.intent, self.tick + 1);
                    self.resolving_late = 0;
                    applied?;
                }
            }
            if !waiting.is_empty() {
                self.update_player(player_id, |player| {
                    waiting.append(&mut player.intents);
                    player.intents = waiting;
                });
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

    /// Applies one intent; `true` when it was refused.
    pub(crate) fn apply_intent(
        &mut self,
        player_id: PlayerId,
        intent: PlayerIntent,
        now: u64,
    ) -> Result<bool, ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(true);
        };
        let alive = player.is_alive();
        let refusal = match intent {
            PlayerIntent::Swing { .. } if !alive => Some((ErrorCode::YouAreDead, None)),
            // The swing turns the player; combat resolution swings once the timer allows.
            PlayerIntent::Swing { facing } => {
                self.update_player(player_id, |player| {
                    player.facing = facing;
                    player.swing_queued = true;
                    player.weapon_drawn = true;
                    player.sitting = crate::Sitting::No;
                });
                None
            }
            PlayerIntent::SetWeaponDrawn(_) if !alive => Some((ErrorCode::YouAreDead, None)),
            PlayerIntent::SetWeaponDrawn(drawn) => {
                self.update_player(player_id, |player| {
                    player.weapon_drawn = drawn;
                    if !drawn {
                        // Committed swings and casts continue through cosmetic stowing (#802).
                        player.guard.raised = None;
                    }
                });
                None
            }
            PlayerIntent::Notice(code) => Some((code, None)),
            PlayerIntent::ReleaseSpirit if alive => Some((ErrorCode::NotDead, None)),
            PlayerIntent::ReleaseSpirit => {
                self.release_spirit(player_id)?;
                None
            }
            PlayerIntent::ChooseClass { class, sex } => self.choose_class(player_id, class, sex),
            // The cast turns the living player to the facing it was aimed along (#884) first.
            PlayerIntent::UseAbility {
                ability,
                target,
                facing,
            } => {
                if let Some(facing) = facing.filter(|_| alive) {
                    self.update_player(player_id, |player| player.facing = facing);
                }
                self.use_ability(player_id, ability, target, now)?
            }
            PlayerIntent::DirectedStrike {
                ability,
                facing,
                aim,
            } => self.directed_strike(player_id, ability, facing, aim, now)?,
            PlayerIntent::CancelCast => {
                self.cancel_cast(player_id);
                None
            }
            PlayerIntent::CommandMinions(order) => self.command_minions(player_id, order)?,
            PlayerIntent::Repair(piece) => {
                if alive {
                    if piece.is_some()
                        && self.players.get(&player_id).is_some_and(|p| p.weapon_drawn)
                    {
                        Some((ErrorCode::WeaponDrawn, None))
                    } else {
                        self.start_repair(player_id, piece)?
                    }
                } else {
                    Some((ErrorCode::YouAreDead, None))
                }
            }
            // Held like movement; the dead hold no guard.
            PlayerIntent::Guard(lane) => {
                if alive {
                    self.update_player(player_id, |player| {
                        player.guard.raised = lane;
                        if lane.is_some() {
                            player.weapon_drawn = true;
                        }
                    });
                }
                None
            }
            // Dead players can only release their spirit.
            PlayerIntent::SelectTarget(_)
            | PlayerIntent::MoveItem { .. }
            | PlayerIntent::EquipItem { .. }
            | PlayerIntent::UnequipItem { .. }
            | PlayerIntent::BuyItem { .. }
            | PlayerIntent::SellItem { .. }
            | PlayerIntent::Loot(_)
            | PlayerIntent::AcceptQuest { .. }
            | PlayerIntent::CompleteQuest { .. }
            | PlayerIntent::UseDoor { .. }
            | PlayerIntent::Sit(_)
            | PlayerIntent::RaidAid(_)
            | PlayerIntent::RaidPrepare(_)
            | PlayerIntent::Gather(_)
            | PlayerIntent::Skin(_)
            | PlayerIntent::ClaimDepthsDrop { .. }
                if !alive =>
            {
                Some((ErrorCode::YouAreDead, None))
            }
            PlayerIntent::UseDoor { .. }
            | PlayerIntent::BuyItem { .. }
            | PlayerIntent::SellItem { .. }
            | PlayerIntent::AcceptQuest { .. }
            | PlayerIntent::CompleteQuest { .. }
            | PlayerIntent::Sit(Some(_))
            | PlayerIntent::RaidAid(_)
            | PlayerIntent::RaidPrepare(_)
            | PlayerIntent::Gather(_)
            | PlayerIntent::Skin(_)
                if self.players.get(&player_id).is_some_and(|p| p.weapon_drawn) =>
            {
                Some((ErrorCode::WeaponDrawn, None))
            }
            // Nothing economically meaningful begins at a cross-zone seam (#145).
            PlayerIntent::Loot(_)
            | PlayerIntent::ClaimDepthsDrop { .. }
            | PlayerIntent::BuyItem { .. }
            | PlayerIntent::SellItem { .. }
            | PlayerIntent::AcceptQuest { .. }
            | PlayerIntent::CompleteQuest { .. }
            | PlayerIntent::AbandonQuest { .. }
            | PlayerIntent::RaidAid(_)
            | PlayerIntent::RaidPrepare(_)
            | PlayerIntent::Gather(_)
            | PlayerIntent::Skin(_)
                if self.in_handoff_corridor(player_id) =>
            {
                Some((ErrorCode::HandoffCorridor, None))
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
                            None
                        } else {
                            Some((ErrorCode::InvalidInventoryMove, None))
                        }
                    }
                }
            }
            PlayerIntent::EquipItem { bag_slot } => {
                let class = self
                    .players
                    .get(&player_id)
                    .and_then(|player| player.class)
                    .map(|choice| choice.class);
                self.change_equipment(player_id, |equipment, bag| {
                    equipment.equip(bag, usize::from(bag_slot), class)
                })?
            }
            PlayerIntent::UnequipItem { equipment_slot } => self
                .change_equipment(player_id, |equipment, bag| {
                    equipment.unequip(bag, equipment_slot)
                })?,
            PlayerIntent::Loot(claim) => self.claim_loot(player_id, claim)?,
            PlayerIntent::UseDoor { door, open } => self.use_door(player_id, door, open)?,
            PlayerIntent::Sit(seat) => self.sit(player_id, seat)?,
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
            PlayerIntent::AcceptQuest { npc, quest } => {
                self.quest_request(player_id, crate::quest::QuestRequest::Accept { npc, quest })?
            }
            PlayerIntent::CompleteQuest { npc, quest, choice } => self.quest_request(
                player_id,
                crate::quest::QuestRequest::Complete { npc, quest, choice },
            )?,
            PlayerIntent::AbandonQuest { quest } => {
                self.quest_request(player_id, crate::quest::QuestRequest::Abandon { quest })?
            }
            PlayerIntent::UseWaystone(waystone) => {
                let refusal = self.use_waystone(player_id, waystone)?;
                if refusal.is_none() {
                    self.update_player(player_id, |player| player.sitting = crate::Sitting::No);
                }
                refusal
            }
            PlayerIntent::RaidAid(task) => self.aid_raid(player_id, task)?,
            PlayerIntent::RaidPrepare(prep) => self.prepare_raid(player_id, prep)?,
            PlayerIntent::Gather(node) => self.gather(player_id, node)?,
            PlayerIntent::Skin(creature) => self.skin(player_id, creature)?,
            // Eligible characters may answer even dead (Atlas 5.1.35).
            PlayerIntent::RollDepthsDrop { run, drop, choice } => {
                self.answer_roll(player_id, run, drop, choice)?
            }
            PlayerIntent::ClaimDepthsDrop { run, drop } => self.claim_drop(player_id, run, drop)?,
            PlayerIntent::SelectTarget(None) => {
                self.update_player(player_id, |player| player.target = None);
                None
            }
            PlayerIntent::SelectTarget(Some(target)) => {
                if self.visible_to(player_id, target)? {
                    self.update_player(player_id, |player| player.target = Some(target));
                    None
                } else {
                    Some((ErrorCode::InvalidTarget, Some(target)))
                }
            }
        };
        let refused = refusal.is_some();
        if let Some((code, target)) = refusal {
            self.notify(player_id, ZoneEvent::Error { code, target });
        }
        Ok(refused)
    }

    /// Whether the creature ignores the player's attack: it is evading, or the
    /// player stands in a handoff corridor, where no kill begins (#145).
    pub(crate) fn creature_evades(&self, player_id: PlayerId, creature_id: CreatureId) -> bool {
        self.creatures
            .get(&creature_id)
            .is_some_and(|creature| matches!(creature.ai, CreatureAi::Evading { .. }))
            || self.in_handoff_corridor(player_id)
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

    /// Observation-only delivery. Every named identity must pass the existing
    /// interest policy; participants keep their original feedback and priorities.
    pub(crate) fn notify_observers(
        &mut self,
        refs: &[EntityRef],
        excluded: &BTreeSet<PlayerId>,
        event: ZoneEvent,
    ) {
        let Some([x, _, z]) = refs
            .first()
            .and_then(|entity| self.unit_position(*entity).ok())
        else {
            return;
        };
        let (candidates, _) = self.interest.candidates(x, z);
        let recipients: Vec<_> = candidates
            .into_iter()
            .filter_map(|entity| match entity {
                EntityRef::Player(id)
                    if !excluded.contains(&id)
                        && refs
                            .iter()
                            .all(|entity| self.visible_to(id, *entity).unwrap_or(false)) =>
                {
                    Some(id)
                }
                _ => None,
            })
            .collect();
        for id in recipients {
            self.notify(id, event);
        }
    }

    fn observe_damage(&mut self, source: EntityRef, target: EntityRef, dealt: u32, critical: bool) {
        if dealt == 0 {
            return;
        }
        let participants = [source, target]
            .into_iter()
            .filter_map(|entity| match entity {
                EntityRef::Player(id) => Some(id),
                _ => None,
            })
            .collect();
        self.notify_observers(
            &[source, target],
            &participants,
            ZoneEvent::DamageObserved {
                source,
                target,
                amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                critical,
            },
        );
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
            EntityRef::Npc(npc_id) => match self.npc_feet(npc_id) {
                Some(feet) => [feet[0], 0, feet[1]],
                None => return Ok(false),
            },
            EntityRef::Minion(minion_id) => {
                if !self.minions.contains_key(&minion_id) {
                    return Ok(false);
                }
                self.unit_position(target)?
            }
        };
        Ok(
            distance_squared([center.x, center.z], [position[0], position[2]])
                <= i64::from(INTEREST_RADIUS_UNITS).pow(2),
        )
    }

    /// Where a released spirit stands: the graveyard, or the first spot on rings around it
    /// clear of every other living player, so spirits released together never stand inside
    /// each other (#534).
    fn release_spot(&self, player_id: PlayerId) -> Result<[i32; 2], ZoneError> {
        let graveyard = self
            .depths_release_point()
            .unwrap_or(self.content.graveyard());
        let spacing = 2 * PLAYER_HALF_EXTENTS_UNITS[0] + 2;
        let mut others = Vec::new();
        for (&other, player) in &self.players {
            if other != player_id && player.is_alive() {
                let position = self.player_position(other)?;
                others.push([position.x, position.z]);
            }
        }
        let clear = |[x, z]: [i32; 2]| {
            others.iter().all(|other| {
                distance_squared([x, z], *other) >= i64::from(spacing) * i64::from(spacing)
            })
        };
        let rings = (1..=8).flat_map(|ring: i32| {
            let (straight, diagonal) = (ring * spacing, ring * spacing * 7 / 10);
            [
                [straight, 0],
                [-straight, 0],
                [0, straight],
                [0, -straight],
                [diagonal, diagonal],
                [diagonal, -diagonal],
                [-diagonal, diagonal],
                [-diagonal, -diagonal],
            ]
        });
        Ok(std::iter::once([0, 0])
            .chain(rings)
            .map(|[dx, dz]| [graveyard[0] + dx, graveyard[1] + dz])
            .find(|&spot| clear(spot))
            .unwrap_or(graveyard))
    }

    /// A dead player returns to the graveyard with half health, out of combat.
    pub(crate) fn release_spirit(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        let [x, z] = self.release_spot(player_id)?;
        let body = body_id(EntityRef::Player(player_id));
        let position = Vec3i::new(x, self.stand_y(x, z, PLAYER_HALF_EXTENTS_UNITS), z);
        self.world
            .set_position(body, position)
            .map_err(physics_error)?;
        self.world
            .set_velocity(body, Vec3i::ZERO)
            .map_err(physics_error)?;
        self.update_player(player_id, |player| {
            player.health = percent_of(player.max_health(), RELEASE_HEALTH_PERCENT);
            player.swing_queued = false;
            player.swing_timer = 0;
            player.melee = crate::MeleeState::default();
            player.combat_timer = 0;
            player.calm_ticks = 0;
        });
        // A knockback or charge from before death does not follow the spirit.
        self.forced.remove(&EntityRef::Player(player_id));
        self.place_in_interest(EntityRef::Player(player_id), position.x, position.z);
        Ok(())
    }

    /// Step 6: swings in `EntityRef` order, players first.
    pub(crate) fn resolve_combat(&mut self, now: u64) -> Result<(), ZoneError> {
        let players: Vec<_> = self.players.keys().copied().collect();
        for player_id in players {
            self.player_swing(player_id, now)?;
        }
        // Only creatures whose swing bit is set (#1086): a quiet one would change nothing.
        let mut after = None;
        while let Some(creature_id) = self.creatures.next_active(Activity::Swing, after) {
            after = Some(creature_id);
            self.tick_work.creatures_visited += 1;
            self.creature_swing(creature_id)?;
            if self.creatures.get(&creature_id).is_none_or(swing_quiet) {
                self.creatures.settle(Activity::Swing, creature_id);
            }
        }
        self.minion_swings(now)
    }

    fn player_swing(&mut self, player_id: PlayerId, now: u64) -> Result<(), ZoneError> {
        let Some(player) = self.players.get_mut(&player_id) else {
            return Ok(());
        };
        // Timers run for the dead too: combat ends 5 s after the killing blow.
        player.swing_timer = player.swing_timer.saturating_sub(1);
        player.combat_timer = player.combat_timer.saturating_sub(1);
        player.guard.broken = player.guard.broken.saturating_sub(1);
        // A strike winding up (#885) lands first; no other begins meanwhile.
        if self.advance_strike(player_id, now)? {
            return Ok(());
        }
        let Some(player) = self.players.get(&player_id) else {
            return Ok(());
        };
        if !player.is_alive() || !player.swing_queued {
            return Ok(());
        }
        // A pressed swing waits out the previous one, a stun or a landing, as
        // does one the graph refuses in water. A follow-up (#928) cuts the recovery short.
        let melee = crate::action::ActionInput::WardenMelee;
        if (player.swing_timer > 0 && player.melee.follow_up == 0)
            || crate::action::holding_rule(self.action_graph, player, melee).is_some()
            || crate::action::refusing_rule(self.action_graph, player, melee).is_some()
            || crate::aura::has_kind(&player.auras, crate::AuraKind::Stun)
        {
            return Ok(());
        }
        self.begin_strike(player_id)
    }

    /// Every attackable living unit a strike toward `facing` covers (#517, #885), in
    /// `EntityRef` order: within `reach` (XZ centre distance), inside `half_width` either side
    /// of the facing and overlapping `band` in height; another player only in line of sight,
    /// as melee always required. A unit centred on the striker is always covered.
    pub(crate) fn swing_coverage(
        &self,
        player_id: PlayerId,
        facing: u16,
        reach: i32,
        half_width: u16,
        band: [i32; 2],
    ) -> Result<Vec<EntityRef>, ZoneError> {
        self.coverage(player_id, facing, reach, half_width, Some(band), false)
    }

    /// Every unit the player may attack within `reach` (XZ centre distance)
    /// and `half_width` either side of `facing`, in `EntityRef` order. A
    /// `melee` attack covers only bodies overlapping [`SWING_BAND_UNITS`] and
    /// needs line of sight to another player; any other needs it to every unit,
    /// unless lobbed `over_cover` (a throw, #884).
    /// The geometry is `sim-combat`'s shared [`swing`] test (#754).
    pub(crate) fn facing_coverage(
        &self,
        player_id: PlayerId,
        facing: u16,
        reach: i32,
        half_width: u16,
        melee: bool,
        over_cover: bool,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let band = melee.then_some(SWING_BAND_UNITS);
        self.coverage(player_id, facing, reach, half_width, band, over_cover)
    }

    /// [`Self::facing_coverage`] with a melee attack's height `band`, `None` for any other.
    fn coverage(
        &self,
        player_id: PlayerId,
        facing: u16,
        reach: i32,
        half_width: u16,
        band: Option<[i32; 2]>,
        over_cover: bool,
    ) -> Result<Vec<EntityRef>, ZoneError> {
        let origin = self.player_position(player_id)?;
        let feet = Vec3i::new(
            origin.x,
            origin.y - crate::PLAYER_HALF_EXTENTS_UNITS[1],
            origin.z,
        );
        let mut covered = Vec::new();
        for entity in self.interest.near(origin.x, origin.z, reach) {
            if !self.attackable_by_player(player_id, entity) {
                continue;
            }
            let body = self
                .world
                .body(body_id(entity))
                .ok_or_else(|| ZoneError::new("unit physics body is missing"))?;
            let (centre, half) = (body.position(), body.half_extents());
            let covers = if let Some(band) = band {
                swing::swing_covers(feet, facing, centre, half, reach, half_width, band)
            } else {
                swing::facing_covers(
                    [origin.x, origin.z],
                    facing,
                    [centre.x, centre.z],
                    reach,
                    half_width,
                )
            };
            if !covers {
                continue;
            }
            if !over_cover
                && (band.is_none() || matches!(entity, EntityRef::Player(_)))
                && !self.line_of_sight(EntityRef::Player(player_id), entity)?
            {
                continue;
            }
            covered.push(entity);
        }
        covered.sort_unstable();
        Ok(covered)
    }

    /// Resolves a player strike through the shared hit table, scaled by the Warden's `stance`
    /// when it began (#155, #885). Creatures retain evasion/tap/threat; arena opponents use
    /// player shields and damage.
    pub(crate) fn attack_unit(
        &mut self,
        player_id: PlayerId,
        target: EntityRef,
        damage: [u16; 2],
        stance: Option<crate::MeleeStance>,
        now: u64,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Player(player_id);
        if !self.attackable_by_player(player_id, target) {
            return Ok(());
        }
        if let EntityRef::Creature(creature_id) = target
            && self.creature_evades(player_id, creature_id)
        {
            self.notify(player_id, ZoneEvent::Evade { source, target });
            return Ok(());
        }
        self.notify_observers(
            &[source, target],
            &BTreeSet::new(),
            ZoneEvent::AttackPerformed {
                source,
                target: Some(target),
            },
        );
        let swing = roll_swing(self.stream(target), damage);
        let swing = match swing {
            Swing::Hit { amount, critical } => Swing::Hit {
                amount: u16::try_from(stance.map_or(u32::from(amount), |stance| {
                    crate::stance::scale(u32::from(amount), stance.damage_percent())
                }))
                .unwrap_or(u16::MAX),
                critical,
            },
            Swing::Miss => Swing::Miss,
        };
        match swing {
            Swing::Miss => {
                let EntityRef::Creature(creature_id) = target else {
                    if let EntityRef::Player(victim) = target {
                        self.update_player(victim, |player| {
                            player.combat_timer = COMBAT_LINGER_TICKS
                        });
                    }
                    self.notify_units(
                        [Some(source), Some(target)],
                        ZoneEvent::Miss { source, target },
                    );
                    return Ok(());
                };
                let prop = self.is_prop(target);
                let Some(creature) = self.creatures.get_mut(&creature_id) else {
                    return Ok(());
                };
                creature.combat_timer = COMBAT_LINGER_TICKS;
                if !prop {
                    creature.add_threat(source, 0);
                }
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
                self.player_melee_hits(player_id, target, u32::from(amount), critical, now)
            }
        }
    }

    /// A Cleric's faith for dealing `amount` damage or restoring `amount` health (#303): half
    /// of it, at least 1 and at most [`crate::class::FAITH_PER_EVENT_MAX`]; nothing for 0.
    pub(crate) fn gain_faith(&mut self, player_id: PlayerId, amount: u32) {
        if amount == 0 {
            return;
        }
        let faith = u16::try_from(amount / 2)
            .unwrap_or(u16::MAX)
            .clamp(1, crate::class::FAITH_PER_EVENT_MAX);
        self.update_player(player_id, |player| {
            if let Some(choice) = player.class
                && choice.class.resource() == crate::ResourceKind::Faith
                && player.is_alive()
            {
                player
                    .resource
                    .gain(crate::ResourceKind::Faith, player.level, faith);
            }
        });
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
        self.unit_damages_creature(player_id, source, creature_id, amount, critical, now)
    }

    /// [`Self::player_damages_creature`] dealt by `source`, the player itself or one of its
    /// minions (#295): the creature's threat goes to `source`, the tap and the credit to
    /// `player_id`.
    pub(crate) fn unit_damages_creature(
        &mut self,
        player_id: PlayerId,
        source: EntityRef,
        creature_id: CreatureId,
        amount: u32,
        critical: bool,
        now: u64,
    ) -> Result<(), ZoneError> {
        let target = EntityRef::Creature(creature_id);
        let Some(creature) = self.creatures.get(&creature_id) else {
            return Ok(());
        };
        // A boulder cannot be broken (#299).
        if !creature.is_alive()
            || self.is_boulder(creature_id)
            || self.combat_blocked(source, target)
        {
            return Ok(());
        }
        if self.creature_evades(player_id, creature_id) {
            self.notify(player_id, ZoneEvent::Evade { source, target });
            return Ok(());
        }
        // A resting boss locks its attempt's profile before the first damage resolves (#276).
        if creature.ai != CreatureAi::Engaged && self.is_boss(creature_id)? {
            self.engage(creature_id, source, true)?;
        }
        let amount = self.absorb(target, source, amount);
        self.update_player(player_id, |player| {
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        // Props (#288) keep no threat or tap: they never fight back and drop nothing.
        let prop = self.is_prop(target);
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        creature.combat_timer = COMBAT_LINGER_TICKS;
        let dealt = amount.min(creature.health);
        creature.health -= dealt;
        if !prop {
            creature.add_threat(source, dealt);
            creature.tapped_by.get_or_insert(player_id);
        }
        // Damage is meaningful contribution to a boss attempt (#276, 5.1.74).
        if dealt > 0
            && let Some(encounter) = creature.encounter.as_mut()
        {
            encounter.contribute(player_id);
        }
        let dead = creature.health == 0;
        self.gain_faith(player_id, dealt);
        self.notify(
            player_id,
            ZoneEvent::DamageDealt {
                source,
                target,
                amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                critical,
            },
        );
        self.observe_damage(source, target, dealt, critical);
        if dead {
            return self.creature_dies(creature_id, Some(source), now);
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
        self.damage_player(
            EntityRef::Creature(creature_id),
            player_id,
            amount,
            critical,
        )
    }

    /// Applies direct environmental health loss on behalf of the authoritative host.
    ///
    /// A mode owns hazard eligibility and scheduling (and must replay those calls).
    /// This is not a player command. Damage bypasses armor, guard and absorption;
    /// it grants no attack credit, rage or rewards. Projections expose the updated
    /// health, and lethal damage uses normal death cleanup with an unknown killer.
    /// Unknown players fail before mutation; zero damage and dead players are no-ops.
    /// Returns health actually lost, clamped to remaining health.
    pub fn apply_environmental_damage(
        &mut self,
        player_id: PlayerId,
        amount: u32,
    ) -> Result<u32, ZoneError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let dealt = amount.min(player.health);
        if dealt == 0 {
            return Ok(0);
        }
        player.health -= dealt;
        player.sitting = crate::Sitting::No;
        player.combat_timer = COMBAT_LINGER_TICKS;
        player.calm_ticks = 0;
        if player.health == 0 {
            self.player_dies(player_id, None)?;
        } else {
            self.break_roots(EntityRef::Player(player_id));
        }
        Ok(dealt)
    }

    /// Shared player damage resolution for eligible hostile sources.
    pub(crate) fn damage_player(
        &mut self,
        source: EntityRef,
        player_id: PlayerId,
        amount: u32,
        critical: bool,
    ) -> Result<(), ZoneError> {
        if let EntityRef::Player(attacker) = source
            && !self.players_are_hostile(attacker, player_id)
        {
            return Ok(());
        }
        let target = EntityRef::Player(player_id);
        if !self
            .players
            .get(&player_id)
            .is_some_and(|player| player.is_alive())
            || self.combat_blocked(source, target)
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
        player.sitting = crate::Sitting::No;
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
        if let EntityRef::Player(attacker) = source {
            self.notify(
                attacker,
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                    critical,
                },
            );
            self.gain_faith(attacker, dealt);
        }
        // A creature's own hit feeds the Divine Reckoning pools it concerns (#936).
        if let EntityRef::Creature(creature_id) = source {
            self.remember_harm(creature_id, player_id, dealt)?;
        }
        self.observe_damage(source, target, dealt, critical);
        if dead {
            return self.player_dies(player_id, Some(source));
        }
        self.break_roots(target);
        Ok(())
    }

    fn creature_swing(&mut self, creature_id: CreatureId) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (_, template) = Self::creature_content(&content, creature_id)?;
        // Read first: a quiet creature's swing changes nothing, so it keeps its stamp (#1086).
        if self.creatures.get(&creature_id).is_none_or(swing_quiet) {
            return Ok(());
        }
        let Some(creature) = self.creatures.get_mut(&creature_id) else {
            return Ok(());
        };
        creature.swing_timer = creature.swing_timer.saturating_sub(1);
        creature.combat_timer = creature.combat_timer.saturating_sub(1);
        // Casting and stunned creatures hold their ready swing, and drop a beast's bite
        // winding up (#889).
        let held = creature.ai != CreatureAi::Engaged
            || creature.cast.is_some()
            || crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun);
        if let Some(mut pending) = creature.attack.take() {
            if held {
                return Ok(());
            }
            pending.contact = pending.contact.saturating_sub(1);
            if pending.contact > 0 {
                creature.attack = Some(pending);
                return Ok(());
            }
            return self.creature_bite(creature_id, template, pending);
        }
        if held || creature.swing_timer > 0 {
            return Ok(());
        }
        let Some((target, victim)) = self.creature_victim(creature_id, None) else {
            return Ok(());
        };
        // A set-piece attacker (#989, #990) begins from its leap beyond its reach.
        let lunge = crate::lunge(template.family);
        let leap = lunge.map_or(0, |lunge| lunge.leap_units);
        if !self.creature_reaches(creature_id, target, leap)? {
            return Ok(());
        }
        let source = EntityRef::Creature(creature_id);
        // A beast winds up one of its two attacks first (#889): the heavy one in
        // HEAVY_ATTACK_ODDS, drawn from its own stream. A set-piece attacker has only its one attack.
        if crate::contact_ticks(template.family, crate::CreatureAttack::Quick).is_some() {
            let heavy = lunge.is_none() && self.stream(source).below(crate::HEAVY_ATTACK_ODDS) == 0;
            let attack = if heavy {
                crate::CreatureAttack::Heavy
            } else {
                crate::CreatureAttack::Quick
            };
            let contact = crate::contact_ticks(template.family, attack).unwrap_or(1);
            if let Some(creature) = self.creatures.get_mut(&creature_id) {
                creature.swing_timer = template.swing_ticks;
                creature.combat_timer = COMBAT_LINGER_TICKS;
                creature.attack = Some(crate::PendingAttack {
                    attack,
                    target,
                    contact,
                });
            }
            self.notify_observers(
                &[source, target],
                &BTreeSet::new(),
                ZoneEvent::CreatureAttackBegan {
                    source,
                    attack,
                    contact,
                },
            );
            return Ok(());
        }
        self.notify_observers(
            &[source, target],
            &BTreeSet::new(),
            ZoneEvent::AttackPerformed {
                source,
                target: Some(target),
            },
        );
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.swing_timer = template.swing_ticks;
        }
        self.creature_blow(
            creature_id,
            template,
            (target, victim),
            crate::CreatureAttack::Quick,
        )
    }

    /// The living player or minion (#295) a creature fights: its top threat, or `only` when
    /// that is still its top threat or on its table.
    fn creature_victim(
        &self,
        creature_id: CreatureId,
        only: Option<EntityRef>,
    ) -> Option<(EntityRef, Result<PlayerId, crate::MinionId>)> {
        let creature = self.creatures.get(&creature_id)?;
        let target = match only {
            Some(only) => creature
                .threat
                .iter()
                .any(|entry| entry.entity == only)
                .then_some(only)?,
            None => creature.top_threat()?,
        };
        match target {
            EntityRef::Player(player_id)
                if self
                    .players
                    .get(&player_id)
                    .is_some_and(|player| player.is_alive()) =>
            {
                Some((target, Ok(player_id)))
            }
            EntityRef::Minion(minion) if self.minions.contains_key(&minion) => {
                Some((target, Err(minion)))
            }
            _ => None,
        }
    }

    /// Whether a creature reaches `target`, `slack` units beyond its reach, and may fight it.
    fn creature_reaches(
        &self,
        creature_id: CreatureId,
        target: EntityRef,
        slack: i32,
    ) -> Result<bool, ZoneError> {
        let source = EntityRef::Creature(creature_id);
        let from = self
            .world
            .body(body_id(source))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
            .position();
        let to = self.unit_position(target)?;
        let reach = i64::from(creature_reach(target) + slack);
        Ok(
            distance_squared([from.x, from.z], [to[0], to[2]]) <= reach * reach
                && !self.combat_blocked(source, target),
        )
    }

    /// Whether a flier's (#995) bite reaches `target`'s height: its centre within
    /// [`crate::flight::BITE_HEIGHT_UNITS`] of the target's. Every other creature's does.
    fn bite_height_reaches(
        &self,
        creature_id: CreatureId,
        template: &crate::CreatureTemplate,
        target: EntityRef,
    ) -> Result<bool, ZoneError> {
        if !template.family.flies() {
            return Ok(true);
        }
        let from = self
            .world
            .body(body_id(EntityRef::Creature(creature_id)))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
            .position();
        let to = self.unit_position(target)?;
        Ok((from.y - to[1]).abs() <= crate::flight::BITE_HEIGHT_UNITS)
    }

    /// A beast's attack reaches its contact tick (#889): it bites the unit it began on if that
    /// unit still stands within reach and a step, and misses one that got away.
    fn creature_bite(
        &mut self,
        creature_id: CreatureId,
        template: &crate::CreatureTemplate,
        pending: crate::PendingAttack,
    ) -> Result<(), ZoneError> {
        let source = EntityRef::Creature(creature_id);
        let Some((target, victim)) = self.creature_victim(creature_id, Some(pending.target)) else {
            return Ok(());
        };
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.combat_timer = COMBAT_LINGER_TICKS;
        }
        if !self.creature_reaches(creature_id, target, crate::CREATURE_ATTACK_SLACK_UNITS)?
            || !self.bite_height_reaches(creature_id, template, target)?
        {
            match victim {
                Ok(player_id) => {
                    self.update_player(player_id, |player| {
                        player.combat_timer = COMBAT_LINGER_TICKS;
                    });
                    self.notify(player_id, ZoneEvent::Miss { source, target });
                }
                Err(_) => {
                    if let Some(summoner) = self.controller(target) {
                        self.notify(summoner, ZoneEvent::Miss { source, target });
                    }
                }
            }
            return Ok(());
        }
        self.creature_blow(creature_id, template, (target, victim), pending.attack)
    }

    /// A creature's blow lands on `target` through the hit table: a miss, a block, or its
    /// `attack`'s damage.
    fn creature_blow(
        &mut self,
        creature_id: CreatureId,
        template: &crate::CreatureTemplate,
        (target, victim): (EntityRef, Result<PlayerId, crate::MinionId>),
        attack: crate::CreatureAttack,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let source = EntityRef::Creature(creature_id);
        let level = self
            .creatures
            .get(&creature_id)
            .map_or(template.min_level, |creature| creature.level);
        let swing = match roll_swing(self.stream(source), template.damage_at(level)) {
            // A boss hits harder under a larger attempt's profile (#276).
            Swing::Hit { amount, critical } => Swing::Hit {
                amount: self.creatures.get(&creature_id).map_or(amount, |creature| {
                    let amount = attack.damage(u32::from(amount));
                    u16::try_from(creature.scale_damage(&content, template.id, amount))
                        .unwrap_or(u16::MAX)
                }),
                critical,
            },
            Swing::Miss => Swing::Miss,
        };
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.combat_timer = COMBAT_LINGER_TICKS;
        }
        let player_id = match victim {
            Ok(player_id) => player_id,
            Err(minion) => {
                return match swing {
                    Swing::Miss => {
                        if let Some(summoner) = self.controller(target) {
                            self.notify(summoner, ZoneEvent::Miss { source, target });
                        }
                        Ok(())
                    }
                    Swing::Hit { amount, critical } => {
                        self.damage_minion(source, minion, u32::from(amount), critical)
                    }
                };
            }
        };
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
                if self.block_player_melee(source, player_id, amount, None)? {
                    return Ok(());
                }
                // A Warden's stance exposes or braces it (#155).
                let amount = self.stance_exposure(player_id, u32::from(amount))?;
                self.creature_damages_player(creature_id, player_id, amount, critical)
            }
        }
    }

    /// Shared shield impact and feedback for creature and player melee attacks.
    pub(crate) fn block_player_melee(
        &mut self,
        source: EntityRef,
        player_id: PlayerId,
        amount: u16,
        strike_lane: Option<crate::StrikeHeight>,
    ) -> Result<bool, ZoneError> {
        let target = EntityRef::Player(player_id);
        match self.guard_outcome(source, player_id, amount, strike_lane)? {
            GuardOutcome::Unguarded => {}
            GuardOutcome::Blocked(lane) => {
                self.update_player(player_id, |player| {
                    player.combat_timer = COMBAT_LINGER_TICKS;
                });
                self.guard_feedback(
                    player_id,
                    ZoneEvent::Blocked {
                        source,
                        target,
                        lane,
                        amount,
                    },
                );
                return Ok(true);
            }
            GuardOutcome::Broken(lane) => {
                self.update_player(player_id, |player| {
                    player.guard.broken = crate::GUARD_BREAK_TICKS;
                });
                self.guard_feedback(
                    player_id,
                    ZoneEvent::GuardBroken {
                        source,
                        target,
                        lane,
                        ticks: crate::GUARD_BREAK_TICKS,
                    },
                );
            }
        }
        Ok(false)
    }

    /// A guard outcome for the defender and the players who see both units.
    pub(crate) fn guard_feedback(&mut self, player_id: PlayerId, event: ZoneEvent) {
        let (ZoneEvent::Blocked { source, target, .. }
        | ZoneEvent::GuardBroken { source, target, .. }) = event
        else {
            return;
        };
        self.notify(player_id, event);
        self.notify_observers(&[source, target], &BTreeSet::from([player_id]), event);
    }

    /// The creature leaves a corpse without a physics body; its tap stays.
    /// `killer` is `None` for a world trap (#300), the credit still follows the tap.
    pub(crate) fn creature_dies(
        &mut self,
        creature_id: CreatureId,
        killer: Option<EntityRef>,
        now: u64,
    ) -> Result<(), ZoneError> {
        let entity = EntityRef::Creature(creature_id);
        let (_, template) = Self::creature_content(&self.content, creature_id)?;
        if let Some(kind) = template.prop() {
            return self.prop_breaks(creature_id, kind, killer.unwrap_or(entity), now);
        }
        let body = self
            .world
            .remove_body(body_id(entity))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
        let mut position = body.position();
        // A flier (#995) falls: its corpse lies on the ground under it.
        if template.family.flies() {
            position.y = self.stand_y(position.x, position.z, template.half_extents);
        }
        let (spawn, _) = Self::creature_content(&self.content, creature_id)?;
        let template = spawn.template;
        // The depths boss pays through its committed outcome; his adds pay nothing (#404).
        let pays_rewards = !self.pays_no_kill_rewards(creature_id);
        // The rolls belong to this life alone (#861): no other death or fight moves them.
        let generation = self
            .creatures
            .get(&creature_id)
            .map_or(0, |creature| creature.generation);
        let mut rolls = self.creature_loot_stream(creature_id, generation);
        let mut loot = self
            .content
            .loot_table(template)
            .filter(|_| pays_rewards)
            .map(|table| {
                table.roll(crate::LootRolls {
                    money: (rolls.next_u64() >> 32) as u32,
                    outcome: (rolls.next_u64() >> 32) as u32,
                    quantity: (rolls.next_u64() >> 32) as u32,
                })
            });
        // A quest item drops for the tapper only while a quest needs it; it
        // draws nothing from the loot stream.
        let tapper = self
            .creatures
            .get(&creature_id)
            .and_then(|creature| creature.tapped_by);
        // The tapper's party shares kill XP, kill-objective credit and loot eligibility
        // (Atlas 5.3.6, 5.4.5); each member stands alone on life and range.
        let spot = [position.x, position.y, position.z];
        let mut party_shares = Vec::new();
        if let Some(player_id) = tapper {
            for mate in self.party_mates(player_id) {
                if self.kill_credit_eligible(mate, spot)? {
                    party_shares.push(mate);
                }
            }
        }
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
        // Its corruptors may spend the corpse (#935); its death clears the Corruption below.
        let corruptors = self.corruptors(creature_id);
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
        creature.wary_ticks = 0;
        // A fallen raider (#530) stays gone: its slot stands again only for a later incident.
        creature.raid = None;
        creature.reset_abilities();
        creature.loot = creature.tapped_by.and(loot);
        creature.loot_party.clone_from(&party_shares);
        let contributors = creature
            .encounter
            .take()
            .map(|encounter| encounter.contributors)
            .unwrap_or_default();
        let tapper = creature.tapped_by;
        let level = creature.level;
        let generation = creature.generation;
        let died = ZoneEvent::Died { entity, killer };
        let mut recipients = BTreeSet::new();
        // A minion's kill is its summoner's (#295).
        recipients.extend(killer.and_then(|killer| self.controller(killer)));
        recipients.extend(tapper);
        recipients.extend(party_shares.iter().copied());
        for player_id in recipients {
            self.notify(player_id, died);
        }
        self.mark_corpse(creature_id, corruptors);
        if pays_rewards {
            if let Some(player_id) = tapper
                && self.kill_credit_eligible(player_id, spot)?
            {
                self.kill_credit(player_id, level, template, creature_id, generation)?;
            }
            for &mate in &party_shares {
                self.kill_credit(mate, level, template, creature_id, generation)?;
            }
            // Every meaningful boss contributor earns the kill objective, across parties
            // (#276, 5.1.74); `credit_kill` records each life once per quest log.
            for player_id in contributors {
                if Some(player_id) != tapper
                    && !party_shares.contains(&player_id)
                    && self.kill_credit_eligible(player_id, spot)?
                {
                    self.credit_kill(player_id, template, creature_id, generation)?;
                }
            }
        }
        if let Some(rules) = self.content.boss(template).copied() {
            self.boss_falls(&rules, [position.x, position.z])?;
        }
        self.boss_defeated(creature_id, now)
    }

    /// One player's kill XP and kill-objective credit, each at most once per creature life.
    fn kill_credit(
        &mut self,
        player_id: PlayerId,
        level: u8,
        template: crate::CreatureTemplateId,
        creature_id: CreatureId,
        generation: u64,
    ) -> Result<(), ZoneError> {
        let key = crate::RewardKey::KillExperience {
            creature: creature_id,
            life: generation,
        };
        if crate::rewards::allowed(&self.rewards, player_id, key)
            && let Some(player) = self.players.get(&player_id)
        {
            let experience = crate::progression::kill_experience(player.level, level)
                .ok_or_else(|| ZoneError::new("invalid progression level"))?;
            let payout = crate::rewards::RewardPayout {
                experience,
                copper: 0,
            };
            self.pay_zone_reward(player_id, key, payout)?;
        }
        self.credit_kill(player_id, template, creature_id, generation)
    }

    /// The camp reacts to its boss's actual death (#276, 5.1.79): idle creatures of its family
    /// within the reaction radius run most of the way to the rally point and hold it.
    fn boss_falls(&mut self, rules: &crate::BossRules, at: [i32; 2]) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let family = content
            .creature_template(rules.template)
            .map(|template| template.family);
        for entity in self.interest.near(at[0], at[1], rules.reaction_radius) {
            let EntityRef::Creature(other) = entity else {
                continue;
            };
            let (spawn, template) = Self::creature_content(&content, other)?;
            if Some(template.family) != family || template.id == rules.template {
                continue;
            }
            let Some(creature) = self.creatures.get_mut(&other) else {
                continue;
            };
            if !creature.is_alive() || !matches!(creature.ai, CreatureAi::Idle { .. }) {
                continue;
            }
            // Raid survivors (#530) rally from their camp spot once they reinforce the camp; on
            // a march they keep marching.
            let home = match creature.raid {
                None => spawn.position,
                Some(member) if member.leg == crate::RaidLeg::Camp => member.anchor,
                Some(_) => continue,
            };
            let point = [
                home[0] + (rules.rally_point[0] - home[0]) * 2 / 3,
                home[1] + (rules.rally_point[1] - home[1]) * 2 / 3,
            ];
            creature.ai = CreatureAi::Rallying {
                ticks: rules.reaction_ticks,
                point,
            };
        }
        Ok(())
    }

    /// Whether the creature's template is a boss (#276).
    pub(crate) fn is_boss(&self, creature_id: CreatureId) -> Result<bool, ZoneError> {
        let (spawn, _) = Self::creature_content(&self.content, creature_id)?;
        Ok(self.content.boss(spawn.template).is_some())
    }

    /// A dead player keeps its body where it fell, stops attacking, lets go
    /// of its held movement and pending jump, and leaves every threat table.
    fn player_dies(
        &mut self,
        player_id: PlayerId,
        killer: Option<EntityRef>,
    ) -> Result<(), ZoneError> {
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
        // Only the creatures it concerns change (#1086).
        self.creatures.update_where(
            |creature| {
                creature.threat.iter().any(|entry| entry.entity == entity)
                    || creature
                        .cast
                        .is_some_and(|cast| cast.target == Some(entity))
            },
            |creature| {
                creature.forget(entity);
                // A cast at the fallen player fizzles; a released spirit is safe.
                if creature
                    .cast
                    .is_some_and(|cast| cast.target == Some(entity))
                {
                    creature.cast = None;
                }
            },
        );
        self.forget_caster(entity);
        self.update_player(player_id, |player| {
            player.health = 0;
            player.swing_queued = false;
            player.swing_timer = 0;
            player.reckoning.clear();
            player.melee = crate::MeleeState::default();
            player.calm_ticks = 0;
            // Death clears every aura and cast; rage and faith drain.
            player.cast = None;
            player.auras.clear();
            if let Some(choice) = player.class
                && choice.class.resource().is_built()
            {
                player.resource.value = 0;
                player.resource.ticks = 0;
            }
            player.forward = Axis::Zero;
            player.strafe = Axis::Zero;
            player.jump_pending = false;
            player.roll = crate::Roll::Ready;
            player.guard = crate::GuardState::default();
        });
        self.notify(player_id, ZoneEvent::Died { entity, killer });
        // Its minions crumble with it (#295); a companion fights on (#296).
        self.release_minions(player_id, false)
    }

    /// Step 7: player regeneration, minions' time (#295), then corpse despawns and respawns in
    /// creature-ID order.
    pub(crate) fn update_timers(&mut self, now: u64) -> Result<(), ZoneError> {
        self.age_minions()?;
        self.expire_harm();
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
        // Only creatures whose timer bit is set (#1086): a living creature, or one that never
        // respawns on its own, waits for nothing here.
        let mut after = None;
        while let Some(creature_id) = self.creatures.next_active(Activity::Timers, after) {
            after = Some(creature_id);
            self.tick_work.creatures_visited += 1;
            let Some((life, looted)) = self
                .creatures
                .get(&creature_id)
                .map(|creature| (creature.life, creature.loot.is_some()))
            else {
                continue;
            };
            let entity = EntityRef::Creature(creature_id);
            // A boss's corpse stays at its camp until it returns (#276, 5.1.75); its loot still
            // expires with the ordinary corpse window.
            let (spawn, template) = Self::creature_content(&content, creature_id)?;
            let lingers = |died_at: u64| {
                content.boss(spawn.template).is_some()
                    && now < died_at.saturating_add(u64::from(template.respawn_ticks))
            };
            if let Life::Corpse { died_at, .. } = life
                && now >= died_at.saturating_add(u64::from(CORPSE_TICKS))
                && lingers(died_at)
            {
                if looted && let Some(creature) = self.creatures.get_mut(&creature_id) {
                    creature.loot = None;
                }
            } else if let Life::Corpse { died_at, .. } = life
                && now >= died_at.saturating_add(u64::from(CORPSE_TICKS))
                && let Some(creature) = self.creatures.get_mut(&creature_id)
            {
                creature.life = Life::Despawned { died_at };
                creature.loot = None;
                if self.interest.remove(entity) {
                    self.interest_work.bucket_removes += 1;
                }
                for player in self.players.values_mut() {
                    if player.target == Some(entity) {
                        player.target = None;
                    }
                }
            }
            let Some(creature) = self.creatures.get(&creature_id) else {
                continue;
            };
            // A prop (#288) stands again only once no unit stands on its spot; night raiders
            // (#530) only when an incident commits them.
            let waits_for_incident = content.is_raider(creature_id)
                || content.is_depths_creature(creature_id)
                || content.is_hunt_beast(creature_id)
                || content.is_cart_bandit(creature_id);
            if let Life::Despawned { died_at } = creature.life
                && !waits_for_incident
                && now >= died_at.saturating_add(u64::from(template.respawn_ticks))
                && (template.prop().is_none() || !self.prop_spot_taken(creature_id)?)
            {
                self.spawn_creature(creature_id)?;
            }
            if self.creatures.get(&creature_id).is_none_or(|creature| {
                creature.is_alive()
                    || (matches!(creature.life, Life::Despawned { .. }) && waits_for_incident)
            }) {
                self.creatures.settle(Activity::Timers, creature_id);
            }
        }
        Ok(())
    }
}
