//! Canonical scope: complete continuation state for trusted replay and
//! recovery. Content is referenced by revision and fingerprint; the decoder
//! checks structure, and `ZoneSimulation::from_snapshot` checks the state
//! against the content.

use mmorpg_core::{
    AbilityId, Aura, CanonicalCreatureAbilities, CanonicalCreatureSnapshot,
    CanonicalPlayerAbilities, CanonicalPlayerCombat, CanonicalPlayerSnapshot,
    CanonicalZoneSnapshot, CastState, ClassChoice, Cooldown, CreatureAi, CreatureId, CreatureLife,
    EntityRef, MAX_AURAS, MAX_CHAT_PER_TICK, MAX_COOLDOWNS, MAX_CREATURE_SPAWNS,
    MAX_ENCOUNTER_PLAYERS, MAX_EVENTS_PER_PLAYER, MAX_PENDING_INTENTS, MAX_PLAYERS_PER_ZONE,
    MAX_THREAT_ENTRIES, PlayerIntent, ThreatEntry, ZoneEvent,
};

use crate::ProtocolError;
use crate::codec::Codec;
use crate::inventory::{decode_equipment, decode_inventory, encode_equipment, encode_inventory};
use crate::wire::{
    CANONICAL_SNAPSHOT_SCOPE, decode_common_header, decode_entity_ref, decode_optional_yaw,
    decode_u8_count, decode_u16_count, encode_common_header, encode_entity_ref,
    encode_optional_yaw, encode_u8_count, encode_u16_count, ensure_fully_consumed, read_bool,
    read_f32_vector, read_u8, take,
};

const SELECT_TARGET_INTENT: u8 = 1;
/// A swing (#517) carries its facing; code 3 (the retired stop-attack) is unused.
const SWING_INTENT: u8 = 2;
const RELEASE_SPIRIT_INTENT: u8 = 4;
const MOVE_ITEM_INTENT: u8 = 5;
const LOOT_INTENT: u8 = 6;
const USE_ABILITY_INTENT: u8 = 7;
const CANCEL_CAST_INTENT: u8 = 8;
const CHOOSE_CLASS_INTENT: u8 = 9;
const EQUIP_ITEM_INTENT: u8 = 10;
const UNEQUIP_ITEM_INTENT: u8 = 11;
const BUY_ITEM_INTENT: u8 = 12;
const SELL_ITEM_INTENT: u8 = 13;
const CHAT_INTENT: u8 = 14;
const ACCEPT_QUEST_INTENT: u8 = 15;
const COMPLETE_QUEST_INTENT: u8 = 16;
const ABANDON_QUEST_INTENT: u8 = 17;
const DIRECTED_STRIKE_INTENT: u8 = 18;
const GUARD_INTENT: u8 = 19;
const USE_DOOR_INTENT: u8 = 20;
const REPAIR_INTENT: u8 = 21;
const COMMAND_MINIONS_INTENT: u8 = 22;
const USE_WAYSTONE_INTENT: u8 = 23;
const RAID_AID_INTENT: u8 = 24;
const RAID_PREPARE_INTENT: u8 = 25;
const NOTICE_INTENT: u8 = 26;
const ROLL_DEPTHS_DROP_INTENT: u8 = 27;
const CLAIM_DEPTHS_DROP_INTENT: u8 = 28;
const SIT_INTENT: u8 = 29;
const SET_WEAPON_DRAWN_INTENT: u8 = 30;
const GATHER_INTENT: u8 = 31;
const SKIN_INTENT: u8 = 32;
/// Inspecting quest evidence (#1019): the spot ID (u32).
const INSPECT_INTENT: u8 = 33;
/// A quest action at an NPC (#1019): the NPC ID (u32).
const QUEST_ACTION_INTENT: u8 = 34;
/// Lifting a load (#1077): the source ID (u32), 0 for the player's own set-down load.
const LIFT_INTENT: u8 = 35;
/// Setting the carried load down (#1077).
const SET_DOWN_INTENT: u8 = 36;
const ALIVE: u8 = 1;
const CORPSE: u8 = 2;
const DESPAWNED: u8 = 3;
const IDLE: u8 = 1;
const ENGAGED: u8 = 2;
const EVADING: u8 = 3;
const RALLYING: u8 = 4;
const TRAILING: u8 = 5;

pub fn encode_canonical_snapshot(
    snapshot: &CanonicalZoneSnapshot,
) -> Result<Vec<u8>, ProtocolError> {
    let player_count = encode_u16_count(
        snapshot.players.len(),
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let creature_count = encode_u16_count(
        snapshot.creatures.len(),
        MAX_CREATURE_SPAWNS,
        "snapshot exceeds the creature spawn capacity",
    )?;
    let mut payload = Vec::new();
    encode_common_header(
        &mut payload,
        CANONICAL_SNAPSHOT_SCOPE,
        snapshot.zone_id,
        snapshot.tick,
    );
    payload.extend_from_slice(&snapshot.content_revision.to_be_bytes());
    payload.extend_from_slice(&snapshot.rng_state.to_be_bytes());
    payload.extend_from_slice(&snapshot.loot_rng_state.to_be_bytes());
    payload.extend_from_slice(&snapshot.world_seed.to_be_bytes());
    payload.extend_from_slice(&player_count.to_be_bytes());
    for player in &snapshot.players {
        player.encode(&mut payload)?;
    }
    payload.extend_from_slice(&creature_count.to_be_bytes());
    for creature in &snapshot.creatures {
        creature.encode(&mut payload)?;
    }
    encode_canonical_doors(&mut payload, &snapshot.doors)?;
    encode_projectiles(
        &mut payload,
        snapshot.next_projectile,
        &snapshot.projectiles,
    )?;
    encode_forced(&mut payload, &snapshot.forced)?;
    encode_debris(&mut payload, snapshot.next_debris, &snapshot.debris)?;
    encode_boulders(&mut payload, &snapshot.boulders)?;
    encode_traps(&mut payload, &snapshot.traps)?;
    encode_pieces(&mut payload, &snapshot.pieces)?;
    encode_summoned_terrain(
        &mut payload,
        snapshot.next_summoned_terrain,
        &snapshot.summoned_terrain,
    )?;
    encode_minions(&mut payload, snapshot.next_minion, &snapshot.minions)?;
    crate::raid::encode_state(&mut payload, &snapshot.raid)?;
    crate::depths::encode_run(&mut payload, snapshot.depths.as_ref())?;
    encode_parties(&mut payload, &snapshot.parties)?;
    encode_wakes(&mut payload, &snapshot.wakes)?;
    crate::rewards::encode_history(&mut payload, snapshot.rewards.as_ref())?;
    encode_rewind_history(&mut payload, &snapshot.rewind_history)?;
    encode_gathering(&mut payload, &snapshot.gathering)?;
    encode_escorts(&mut payload, &snapshot.escorts)?;
    encode_loads(&mut payload, &snapshot.loads)?;
    Ok(payload)
}

/// Canonical gathering (#917): `count u16 · count × ready tick u64`, one per content vein, then
/// the skinned corpses' creature IDs as an ascending set.
fn encode_gathering(
    payload: &mut Vec<u8>,
    gathering: &mmorpg_core::GatheringState,
) -> Result<(), ProtocolError> {
    let message = "snapshot gathering exceeds its limits";
    payload.extend_from_slice(
        &crate::wire::encode_u16_count(gathering.veins.len(), usize::from(u16::MAX), message)?
            .to_be_bytes(),
    );
    for ready in &gathering.veins {
        payload.extend_from_slice(&ready.to_be_bytes());
    }
    let skinned = gathering.skinned.iter().map(|id| id.get()).collect();
    crate::wire::encode_u32_set(payload, &skinned, message)
}

fn decode_gathering(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::GatheringState, ProtocolError> {
    let message = "snapshot gathering is malformed";
    let count = decode_u16_count(payload, offset, usize::from(u16::MAX), message)?;
    let mut veins = Vec::with_capacity(count);
    for _ in 0..count {
        veins.push(u64::from_be_bytes(take(payload, offset)?));
    }
    let skinned = crate::wire::decode_u32_set(payload, offset, message)?
        .into_iter()
        .map(mmorpg_core::CreatureId::new)
        .collect();
    Ok(mmorpg_core::GatheringState { veins, skinned })
}

/// A canonical player record: its whole continuation state, variable in length.
impl Codec for CanonicalPlayerSnapshot {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        let player = self;
        payload.extend_from_slice(&player.player_id.to_be_bytes());
        for component in player.position.into_iter().chain(player.velocity) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&player.facing.to_be_bytes());
        payload.extend_from_slice(&player.forward.to_be_bytes());
        payload.extend_from_slice(&player.strafe.to_be_bytes());
        payload.push(u8::from(player.jump_pending));
        payload.extend_from_slice(&player.rise.to_be_bytes());
        payload.extend_from_slice(&player.breath.remaining.to_be_bytes());
        payload.extend_from_slice(&player.breath.drowning.to_be_bytes());
        payload.extend_from_slice(&player.last_sequence.to_be_bytes());
        payload.extend_from_slice(&player.spawn_slot.to_be_bytes());
        payload.extend_from_slice(&player.inventory_revision.to_be_bytes());
        encode_inventory(payload, &player.inventory);
        encode_equipment(payload, &player.equipment);
        payload.extend_from_slice(&player.copper.to_be_bytes());
        let combat = &player.combat;
        payload.push(combat.level);
        payload.extend_from_slice(&combat.experience.to_be_bytes());
        payload.extend_from_slice(&combat.health.to_be_bytes());
        encode_entity_ref(payload, combat.target);
        payload.push(u8::from(combat.swing_queued));
        payload.push(u8::from(combat.weapon_drawn));
        for timer in [combat.swing_timer, combat.combat_timer, combat.calm_ticks] {
            payload.extend_from_slice(&timer.to_be_bytes());
        }
        payload.push(encode_u8_count(
            combat.intents.len(),
            MAX_PENDING_INTENTS,
            "player has too many pending intents",
        )?);
        for queued in &combat.intents {
            // Each intent leads with its lateness in ticks (#415).
            payload.push(queued.late);
            let (code, target) = match queued.intent {
                PlayerIntent::SelectTarget(target) => (SELECT_TARGET_INTENT, target),
                PlayerIntent::Swing { facing } => {
                    payload.push(SWING_INTENT);
                    payload.extend_from_slice(&facing.to_be_bytes());
                    continue;
                }
                PlayerIntent::Loot(claim) => {
                    payload.push(LOOT_INTENT);
                    crate::loot::encode_claim(payload, claim);
                    continue;
                }
                PlayerIntent::Skin(creature) => {
                    payload.push(SKIN_INTENT);
                    payload.extend_from_slice(&creature.get().to_be_bytes());
                    continue;
                }
                PlayerIntent::Gather(node) => {
                    payload.push(GATHER_INTENT);
                    payload.extend_from_slice(&node.to_be_bytes());
                    continue;
                }
                PlayerIntent::QuestAction(npc) => {
                    payload.push(QUEST_ACTION_INTENT);
                    payload.extend_from_slice(&npc.get().to_be_bytes());
                    continue;
                }
                PlayerIntent::Inspect(spot) => {
                    payload.push(INSPECT_INTENT);
                    payload.extend_from_slice(&spot.to_be_bytes());
                    continue;
                }
                PlayerIntent::Lift(source) => {
                    payload.push(LIFT_INTENT);
                    payload.extend_from_slice(
                        &source.map_or(0, mmorpg_core::LoadSpotId::get).to_be_bytes(),
                    );
                    continue;
                }
                PlayerIntent::SetDown => {
                    payload.push(SET_DOWN_INTENT);
                    continue;
                }
                PlayerIntent::ReleaseSpirit => (RELEASE_SPIRIT_INTENT, None),
                PlayerIntent::MoveItem {
                    source,
                    destination,
                    quantity,
                } => {
                    payload.extend_from_slice(&[MOVE_ITEM_INTENT, source, destination]);
                    payload.extend_from_slice(&quantity.to_be_bytes());
                    payload.push(0);
                    continue;
                }
                PlayerIntent::UseAbility {
                    ability,
                    target,
                    facing,
                } => {
                    payload.push(USE_ABILITY_INTENT);
                    payload.extend_from_slice(&ability.to_be_bytes());
                    encode_entity_ref(payload, target);
                    encode_optional_yaw(payload, facing);
                    continue;
                }
                PlayerIntent::DirectedStrike {
                    ability,
                    facing,
                    aim,
                } => {
                    payload.push(DIRECTED_STRIKE_INTENT);
                    payload.extend_from_slice(&ability.to_be_bytes());
                    payload.extend_from_slice(&facing.to_be_bytes());
                    payload.extend_from_slice(&crate::wire::encode_aim(aim));
                    continue;
                }
                PlayerIntent::CancelCast => (CANCEL_CAST_INTENT, None),
                PlayerIntent::Guard(lane) => {
                    payload.extend_from_slice(&[GUARD_INTENT, crate::wire::encode_lane(lane)]);
                    continue;
                }
                PlayerIntent::SetWeaponDrawn(drawn) => {
                    payload.extend_from_slice(&[SET_WEAPON_DRAWN_INTENT, u8::from(drawn)]);
                    continue;
                }
                PlayerIntent::UseDoor { door, open } => {
                    payload.push(USE_DOOR_INTENT);
                    payload.extend_from_slice(&door.get().to_be_bytes());
                    payload.push(u8::from(open));
                    continue;
                }
                PlayerIntent::Sit(seat) => {
                    payload.push(SIT_INTENT);
                    payload
                        .extend_from_slice(&seat.map_or(0, mmorpg_core::SeatId::get).to_be_bytes());
                    continue;
                }
                PlayerIntent::Repair(piece) => {
                    payload.push(REPAIR_INTENT);
                    payload.extend_from_slice(&crate::wire::encode_piece(piece).to_be_bytes());
                    continue;
                }
                PlayerIntent::CommandMinions(order) => {
                    payload.push(COMMAND_MINIONS_INTENT);
                    crate::wire::encode_minion_order(payload, order);
                    continue;
                }
                PlayerIntent::ChooseClass { class, sex } => {
                    payload.extend_from_slice(&[CHOOSE_CLASS_INTENT, class, sex]);
                    continue;
                }
                PlayerIntent::EquipItem { bag_slot } => {
                    payload.extend_from_slice(&[EQUIP_ITEM_INTENT, bag_slot]);
                    continue;
                }
                PlayerIntent::UnequipItem { equipment_slot } => {
                    payload.extend_from_slice(&[UNEQUIP_ITEM_INTENT, equipment_slot]);
                    continue;
                }
                PlayerIntent::BuyItem {
                    npc,
                    offer,
                    quantity,
                } => {
                    encode_trade_intent(payload, BUY_ITEM_INTENT, npc, offer, quantity);
                    continue;
                }
                PlayerIntent::SellItem {
                    npc,
                    bag_slot,
                    quantity,
                } => {
                    encode_trade_intent(
                        payload,
                        SELL_ITEM_INTENT,
                        npc,
                        u32::from(bag_slot),
                        quantity,
                    );
                    continue;
                }
                PlayerIntent::Chat(message) => {
                    payload.push(CHAT_INTENT);
                    crate::chat::encode_message(payload, message);
                    continue;
                }
                PlayerIntent::AcceptQuest { npc, quest } => {
                    payload.push(ACCEPT_QUEST_INTENT);
                    payload.extend_from_slice(&npc.get().to_be_bytes());
                    payload.extend_from_slice(&quest.to_be_bytes());
                    continue;
                }
                PlayerIntent::CompleteQuest { npc, quest, choice } => {
                    payload.push(COMPLETE_QUEST_INTENT);
                    payload.extend_from_slice(&npc.get().to_be_bytes());
                    payload.extend_from_slice(&quest.to_be_bytes());
                    payload.push(choice);
                    continue;
                }
                PlayerIntent::AbandonQuest { quest } => {
                    payload.push(ABANDON_QUEST_INTENT);
                    payload.extend_from_slice(&quest.to_be_bytes());
                    continue;
                }
                PlayerIntent::UseWaystone(waystone) => {
                    payload.push(USE_WAYSTONE_INTENT);
                    payload.extend_from_slice(&waystone.get().to_be_bytes());
                    continue;
                }
                PlayerIntent::RaidAid(task) => {
                    payload.push(RAID_AID_INTENT);
                    payload.extend_from_slice(&task.to_be_bytes());
                    continue;
                }
                PlayerIntent::RaidPrepare(prep) => {
                    payload.push(RAID_PREPARE_INTENT);
                    payload.extend_from_slice(&crate::raid::encode_prep(prep));
                    continue;
                }
                PlayerIntent::Notice(code) => {
                    payload.push(NOTICE_INTENT);
                    payload.extend_from_slice(&crate::wire::error_code(code).to_be_bytes());
                    continue;
                }
                PlayerIntent::RollDepthsDrop { run, drop, choice } => {
                    payload.push(ROLL_DEPTHS_DROP_INTENT);
                    payload.extend_from_slice(&run.to_be_bytes());
                    payload.push(drop);
                    payload.push(crate::depths::encode_choice(choice));
                    continue;
                }
                PlayerIntent::ClaimDepthsDrop { run, drop } => {
                    payload.push(CLAIM_DEPTHS_DROP_INTENT);
                    payload.extend_from_slice(&run.to_be_bytes());
                    payload.push(drop);
                    continue;
                }
            };
            payload.push(code);
            encode_entity_ref(payload, target);
        }
        payload.push(u8::from(combat.intents_dropped));
        payload.extend_from_slice(&combat.last_observed.to_be_bytes());
        payload.push(encode_u8_count(
            combat.events.len(),
            MAX_EVENTS_PER_PLAYER,
            "player has too many events",
        )?);
        for event in &combat.events {
            event.encode(payload)?;
        }
        let abilities = &combat.abilities;
        payload.push(
            abilities
                .class
                .map_or(0, |choice| u8::try_from(choice.appearance()).unwrap_or(0)),
        );
        for value in [
            abilities.resource,
            abilities.resource_ticks,
            abilities.mana_delay,
            abilities.global_cooldown,
            abilities.global_cooldown_total,
        ] {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.push(encode_u8_count(
            abilities.cooldowns.len(),
            MAX_COOLDOWNS,
            "player has too many cooldowns",
        )?);
        for cooldown in &abilities.cooldowns {
            payload.extend_from_slice(&cooldown.ability.get().to_be_bytes());
            payload.extend_from_slice(&cooldown.remaining.to_be_bytes());
        }
        encode_cast(payload, abilities.cast);
        encode_auras(payload, &abilities.auras)?;
        payload.push(crate::wire::encode_lane(abilities.guard.raised));
        payload.extend_from_slice(&abilities.guard.broken.to_be_bytes());
        // Divine Reckoning pools (#936): count, then creature ID (`u32`) and harm (`u16`) each.
        payload.push(encode_u8_count(
            abilities.reckoning.len(),
            mmorpg_core::MAX_RECKONING_POOLS,
            "player has too many reckoning pools",
        )?);
        for harm in &abilities.reckoning {
            payload.extend_from_slice(&harm.creature.get().to_be_bytes());
            payload.extend_from_slice(&harm.amount.to_be_bytes());
        }
        let landing = combat.landing;
        payload.push(u8::from(landing.airborne));
        payload.extend_from_slice(&landing.recovery.to_be_bytes());
        payload.push(u8::from(landing.attacked_airborne));
        encode_traversal(payload, combat.traversal);
        // What the player sits on (#619): 0 nothing, 1 the ground, 2 + a seat ID (`u32`).
        let (sitting, seat) = match combat.sitting {
            mmorpg_core::Sitting::No => (0, 0),
            mmorpg_core::Sitting::Ground => (1, 0),
            mmorpg_core::Sitting::On(seat) => (2, seat.get()),
        };
        payload.push(sitting);
        payload.extend_from_slice(&seat.to_be_bytes());
        encode_roll(payload, combat.roll);
        payload.extend_from_slice(&combat.strike_recovery.to_be_bytes());
        payload.extend_from_slice(&crate::wire::encode_piece(combat.repairing).to_be_bytes());
        encode_entity_ref(payload, combat.combo.map(|combo| combo.target));
        payload.extend_from_slice(&combat.combo.map_or(0, |combo| combo.elapsed).to_be_bytes());
        payload.push(combat.combo.map_or(0, |combo| combo.hits));
        encode_swing(payload, combat.swing);
        encode_melee(payload, combat.melee);
        payload.extend_from_slice(&player.chat_ready_at.to_be_bytes());
        payload.push(encode_u8_count(
            player.chat.len(),
            MAX_CHAT_PER_TICK,
            "player heard too many chat lines",
        )?);
        for line in &player.chat {
            crate::chat::encode_line(payload, line);
        }
        crate::quest::encode_log(payload, &player.quests)?;
        crate::wire::encode_waystones(payload, &player.waystones)?;
        crate::wire::encode_discoveries(payload, &player.discoveries)?;
        crate::wire::encode_caches(payload, &player.caches)?;
        crate::wire::encode_mastery(payload, player.mastery);
        crate::wire::encode_practice(payload, &player.practice)?;
        crate::character::encode_optional_character(payload, player.character.as_ref())?;
        crate::character::encode_depths_record(payload, player.depths);
        crate::character::encode_physique(payload, &player.physique);
        Ok(())
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let player_id = u32::from_be_bytes(take(payload, offset)?);
        let position = read_f32_vector(payload, offset)?;
        let velocity = read_f32_vector(payload, offset)?;
        let facing = u16::from_be_bytes(take(payload, offset)?);
        let forward = i8::from_be_bytes(take(payload, offset)?);
        let strafe = i8::from_be_bytes(take(payload, offset)?);
        let jump_pending = match read_u8(payload, offset)? {
            0 => false,
            1 => true,
            _ => return Err(ProtocolError::new("jump flag must be 0 or 1")),
        };
        let rise = i8::from_be_bytes(take(payload, offset)?);
        let breath = mmorpg_core::Breath {
            remaining: u16::from_be_bytes(take(payload, offset)?),
            drowning: u16::from_be_bytes(take(payload, offset)?),
        };
        let last_sequence = u32::from_be_bytes(take(payload, offset)?);
        let spawn_slot = u16::from_be_bytes(take(payload, offset)?);
        let inventory_revision = u64::from_be_bytes(take(payload, offset)?);
        let inventory = decode_inventory(payload, offset)?;
        let equipment = decode_equipment(payload, offset)?;
        let copper = u32::from_be_bytes(take(payload, offset)?);
        let level = read_u8(payload, offset)?;
        let experience = u32::from_be_bytes(take(payload, offset)?);
        let health = u32::from_be_bytes(take(payload, offset)?);
        let target = decode_entity_ref(payload, offset)?;
        let swing_queued = read_bool(payload, offset)?;
        let weapon_drawn = read_bool(payload, offset)?;
        let swing_timer = u16::from_be_bytes(take(payload, offset)?);
        let combat_timer = u16::from_be_bytes(take(payload, offset)?);
        let calm_ticks = u16::from_be_bytes(take(payload, offset)?);
        let intent_count = decode_u8_count(
            payload,
            offset,
            MAX_PENDING_INTENTS,
            "player has too many pending intents",
        )?;
        let mut intents = Vec::with_capacity(intent_count);
        let mut lateness = Vec::with_capacity(intent_count);
        for _ in 0..intent_count {
            lateness.push(read_u8(payload, offset)?);
            let code = read_u8(payload, offset)?;
            if code == LOOT_INTENT {
                intents.push(PlayerIntent::Loot(crate::loot::decode_claim(
                    payload, offset,
                )?));
                continue;
            }
            if code == SKIN_INTENT {
                intents.push(PlayerIntent::Skin(mmorpg_core::CreatureId::new(
                    u32::from_be_bytes(take(payload, offset)?),
                )));
                continue;
            }
            if code == GATHER_INTENT {
                intents.push(PlayerIntent::Gather(u32::from_be_bytes(take(
                    payload, offset,
                )?)));
                continue;
            }
            if code == QUEST_ACTION_INTENT {
                intents.push(PlayerIntent::QuestAction(mmorpg_core::NpcId::new(
                    u32::from_be_bytes(take(payload, offset)?),
                )));
                continue;
            }
            if code == INSPECT_INTENT {
                intents.push(PlayerIntent::Inspect(u32::from_be_bytes(take(
                    payload, offset,
                )?)));
                continue;
            }
            if code == LIFT_INTENT {
                intents.push(PlayerIntent::Lift(
                    match u32::from_be_bytes(take(payload, offset)?) {
                        0 => None,
                        source => Some(mmorpg_core::LoadSpotId::new(source)),
                    },
                ));
                continue;
            }
            if code == SET_DOWN_INTENT {
                intents.push(PlayerIntent::SetDown);
                continue;
            }
            if code == USE_ABILITY_INTENT {
                let ability = u32::from_be_bytes(take(payload, offset)?);
                intents.push(PlayerIntent::UseAbility {
                    ability,
                    target: decode_entity_ref(payload, offset)?,
                    facing: decode_optional_yaw(payload, offset)?,
                });
                continue;
            }
            if code == DIRECTED_STRIKE_INTENT {
                let ability = u32::from_be_bytes(take(payload, offset)?);
                let [high, low, side, height] = take(payload, offset)?;
                intents.push(PlayerIntent::DirectedStrike {
                    ability,
                    facing: u16::from_be_bytes([high, low]),
                    aim: crate::wire::decode_aim([side, height])?,
                });
                continue;
            }
            if code == SWING_INTENT {
                intents.push(PlayerIntent::Swing {
                    facing: u16::from_be_bytes(take(payload, offset)?),
                });
                continue;
            }
            if code == GUARD_INTENT {
                intents.push(PlayerIntent::Guard(crate::wire::decode_lane(read_u8(
                    payload, offset,
                )?)?));
                continue;
            }
            if code == SET_WEAPON_DRAWN_INTENT {
                intents.push(PlayerIntent::SetWeaponDrawn(read_bool(payload, offset)?));
                continue;
            }
            if code == USE_DOOR_INTENT {
                let door = u32::from_be_bytes(take(payload, offset)?);
                let open = read_u8(payload, offset)?;
                intents.push(PlayerIntent::UseDoor {
                    door: mmorpg_core::DoorId::new(door),
                    open: match open {
                        0 => false,
                        1 => true,
                        _ => return Err(ProtocolError::new("door state must be 0 or 1")),
                    },
                });
                continue;
            }
            if code == SIT_INTENT {
                let seat = u32::from_be_bytes(take(payload, offset)?);
                intents.push(PlayerIntent::Sit(
                    (seat != 0).then(|| mmorpg_core::SeatId::new(seat)),
                ));
                continue;
            }
            if code == REPAIR_INTENT {
                let piece = u32::from_be_bytes(take(payload, offset)?);
                intents.push(PlayerIntent::Repair(crate::wire::decode_piece(piece)));
                continue;
            }
            if code == COMMAND_MINIONS_INTENT {
                intents.push(PlayerIntent::CommandMinions(
                    crate::wire::decode_minion_order(payload, offset)?,
                ));
                continue;
            }
            if code == CHOOSE_CLASS_INTENT {
                let [class, sex] = take(payload, offset)?;
                intents.push(PlayerIntent::ChooseClass { class, sex });
                continue;
            }
            if code == ACCEPT_QUEST_INTENT || code == COMPLETE_QUEST_INTENT {
                let npc = mmorpg_core::NpcId::new(u32::from_be_bytes(take(payload, offset)?));
                let quest = u32::from_be_bytes(take(payload, offset)?);
                intents.push(if code == ACCEPT_QUEST_INTENT {
                    PlayerIntent::AcceptQuest { npc, quest }
                } else {
                    PlayerIntent::CompleteQuest {
                        npc,
                        quest,
                        choice: read_u8(payload, offset)?,
                    }
                });
                continue;
            }
            if code == ABANDON_QUEST_INTENT {
                intents.push(PlayerIntent::AbandonQuest {
                    quest: u32::from_be_bytes(take(payload, offset)?),
                });
                continue;
            }
            if code == USE_WAYSTONE_INTENT {
                intents.push(PlayerIntent::UseWaystone(crate::wire::decode_waystone(
                    u32::from_be_bytes(take(payload, offset)?),
                )?));
                continue;
            }
            if code == RAID_AID_INTENT {
                intents.push(PlayerIntent::RaidAid(u32::from_be_bytes(take(
                    payload, offset,
                )?)));
                continue;
            }
            if code == NOTICE_INTENT {
                let wire = u16::from_be_bytes(take(payload, offset)?);
                intents.push(PlayerIntent::Notice(crate::wire::decode_error_code(wire)?));
                continue;
            }
            if code == ROLL_DEPTHS_DROP_INTENT {
                let run = u64::from_be_bytes(take(payload, offset)?);
                let drop = read_u8(payload, offset)?;
                let choice = crate::depths::decode_choice(read_u8(payload, offset)?)?;
                intents.push(PlayerIntent::RollDepthsDrop { run, drop, choice });
                continue;
            }
            if code == CLAIM_DEPTHS_DROP_INTENT {
                let run = u64::from_be_bytes(take(payload, offset)?);
                let drop = read_u8(payload, offset)?;
                intents.push(PlayerIntent::ClaimDepthsDrop { run, drop });
                continue;
            }
            if code == RAID_PREPARE_INTENT {
                intents.push(PlayerIntent::RaidPrepare(crate::raid::decode_prep(take(
                    payload, offset,
                )?)?));
                continue;
            }
            if code == CHAT_INTENT {
                intents.push(PlayerIntent::Chat(crate::chat::decode_message(
                    payload, offset,
                )?));
                continue;
            }
            if code == BUY_ITEM_INTENT || code == SELL_ITEM_INTENT {
                let npc = mmorpg_core::NpcId::new(u32::from_be_bytes(take(payload, offset)?));
                let index = u32::from_be_bytes(take(payload, offset)?);
                let quantity = u16::from_be_bytes(take(payload, offset)?);
                intents.push(if code == BUY_ITEM_INTENT {
                    PlayerIntent::BuyItem {
                        npc,
                        offer: index,
                        quantity,
                    }
                } else {
                    PlayerIntent::SellItem {
                        npc,
                        bag_slot: u8::try_from(index).map_err(|_| {
                            ProtocolError::new("sell intent bag slot is out of range")
                        })?,
                        quantity,
                    }
                });
                continue;
            }
            if code == EQUIP_ITEM_INTENT {
                let [bag_slot] = take(payload, offset)?;
                intents.push(PlayerIntent::EquipItem { bag_slot });
                continue;
            }
            if code == UNEQUIP_ITEM_INTENT {
                let [equipment_slot] = take(payload, offset)?;
                intents.push(PlayerIntent::UnequipItem { equipment_slot });
                continue;
            }
            if code == MOVE_ITEM_INTENT {
                let [source, destination, high, low, reserved] = take(payload, offset)?;
                if reserved != 0 {
                    return Err(ProtocolError::new("malformed pending inventory intent"));
                }
                intents.push(PlayerIntent::MoveItem {
                    source,
                    destination,
                    quantity: u16::from_be_bytes([high, low]),
                });
                continue;
            }
            let target = decode_entity_ref(payload, offset)?;
            intents.push(match (code, target) {
                (SELECT_TARGET_INTENT, target) => PlayerIntent::SelectTarget(target),
                (RELEASE_SPIRIT_INTENT, None) => PlayerIntent::ReleaseSpirit,
                (CANCEL_CAST_INTENT, None) => PlayerIntent::CancelCast,
                _ => return Err(ProtocolError::new("malformed pending intent")),
            });
        }
        let intents = intents
            .into_iter()
            .zip(lateness)
            .map(|(intent, late)| mmorpg_core::QueuedIntent { intent, late })
            .collect();
        let intents_dropped = read_bool(payload, offset)?;
        let last_observed = u64::from_be_bytes(take(payload, offset)?);
        let event_count = decode_u8_count(
            payload,
            offset,
            MAX_EVENTS_PER_PLAYER,
            "player has too many events",
        )?;
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            events.push(ZoneEvent::decode(payload, offset)?);
        }
        let class_code = read_u8(payload, offset)?;
        let class = ClassChoice::from_appearance(u32::from(class_code));
        if class_code != 0 && class.is_none() {
            return Err(ProtocolError::new("unknown class choice"));
        }
        let resource = u16::from_be_bytes(take(payload, offset)?);
        let resource_ticks = u16::from_be_bytes(take(payload, offset)?);
        let mana_delay = u16::from_be_bytes(take(payload, offset)?);
        let global_cooldown = u16::from_be_bytes(take(payload, offset)?);
        let global_cooldown_total = u16::from_be_bytes(take(payload, offset)?);
        let cooldown_count = decode_u8_count(
            payload,
            offset,
            MAX_COOLDOWNS,
            "player has too many cooldowns",
        )?;
        let mut cooldowns = Vec::with_capacity(cooldown_count);
        for _ in 0..cooldown_count {
            let ability = AbilityId::new(u32::from_be_bytes(take(payload, offset)?));
            cooldowns.push(Cooldown {
                ability,
                remaining: u16::from_be_bytes(take(payload, offset)?),
            });
        }
        let abilities = CanonicalPlayerAbilities {
            class,
            resource,
            resource_ticks,
            mana_delay,
            global_cooldown,
            global_cooldown_total,
            cooldowns,
            cast: decode_cast(payload, offset)?,
            auras: decode_auras(payload, offset)?,
            guard: mmorpg_core::GuardState {
                raised: crate::wire::decode_lane(read_u8(payload, offset)?)?,
                broken: u16::from_be_bytes(take(payload, offset)?),
            },
            reckoning: {
                let count = decode_u8_count(
                    payload,
                    offset,
                    mmorpg_core::MAX_RECKONING_POOLS,
                    "player has too many reckoning pools",
                )?;
                let mut pools = Vec::with_capacity(count);
                for _ in 0..count {
                    pools.push(mmorpg_core::Harm {
                        creature: CreatureId::new(u32::from_be_bytes(take(payload, offset)?)),
                        amount: u16::from_be_bytes(take(payload, offset)?),
                    });
                }
                pools
            },
        };
        let landing = mmorpg_core::LandingState {
            airborne: read_bool(payload, offset)?,
            recovery: u16::from_be_bytes(take(payload, offset)?),
            attacked_airborne: read_bool(payload, offset)?,
        };
        let traversal = decode_traversal(payload, offset)?;
        let sitting = match (
            read_u8(payload, offset)?,
            u32::from_be_bytes(take(payload, offset)?),
        ) {
            (0, 0) => mmorpg_core::Sitting::No,
            (1, 0) => mmorpg_core::Sitting::Ground,
            (2, seat) if seat != 0 => mmorpg_core::Sitting::On(mmorpg_core::SeatId::new(seat)),
            _ => return Err(ProtocolError::new("player sit code is out of range")),
        };
        let roll = decode_roll(payload, offset)?;
        let strike_recovery = u16::from_be_bytes(take(payload, offset)?);
        let repairing = crate::wire::decode_piece(u32::from_be_bytes(take(payload, offset)?));
        let combo_target = decode_entity_ref(payload, offset)?;
        let combo_elapsed = u16::from_be_bytes(take(payload, offset)?);
        let combo_hits = read_u8(payload, offset)?;
        let combo = if let Some(target) = combo_target {
            Some(mmorpg_core::ComboOpening {
                target,
                elapsed: combo_elapsed,
                hits: combo_hits,
            })
        } else if combo_elapsed != 0 || combo_hits != 0 {
            return Err(ProtocolError::new("absent combo has nonzero fields"));
        } else {
            None
        };
        let swing = decode_swing(payload, offset)?;
        let melee = decode_melee(payload, offset)?;
        let chat_ready_at = u64::from_be_bytes(take(payload, offset)?);
        let chat_count = decode_u8_count(
            payload,
            offset,
            MAX_CHAT_PER_TICK,
            "player heard too many chat lines",
        )?;
        let mut chat = Vec::with_capacity(chat_count);
        for _ in 0..chat_count {
            chat.push(crate::chat::decode_line(payload, offset)?);
        }
        let quests = crate::quest::decode_log(payload, offset, crate::quest::QuestIdWidth::Word)?;
        let waystones = crate::wire::decode_waystones(payload, offset)?;
        let discoveries = crate::wire::decode_discoveries(payload, offset)?;
        let caches = crate::wire::decode_caches(payload, offset)?;
        let mastery = crate::wire::decode_mastery(payload, offset)?;
        let practice = crate::wire::decode_practice(payload, offset)?;
        let character = crate::character::decode_optional_character(payload, offset)?;
        let depths = crate::character::decode_depths_record(payload, offset)?;
        let physique = crate::character::decode_physique(payload, offset)?;
        Ok(CanonicalPlayerSnapshot {
            quests,
            waystones,
            discoveries,
            caches,
            mastery,
            practice,
            character,
            depths,
            physique,
            chat_ready_at,
            chat,
            player_id,
            position,
            velocity,
            facing,
            forward,
            strafe,
            jump_pending,
            rise,
            breath,
            last_sequence,
            spawn_slot,
            inventory,
            copper,
            inventory_revision,
            equipment,
            combat: CanonicalPlayerCombat {
                level,
                experience,
                health,
                target,
                swing_queued,
                swing_timer,
                combat_timer,
                weapon_drawn,
                calm_ticks,
                intents,
                intents_dropped,
                last_observed,
                events,
                abilities,
                landing,
                traversal,
                sitting,
                roll,
                combo,
                strike_recovery,
                swing,
                melee,
                repairing,
            },
        })
    }
}

/// 19 bytes (#490, #500): kind (0 free, 1 released, 2 climbing, 3 mantling), ticks, wall yaw,
/// the mantle's goal (`3 × i32`), whether it moves across, and the climb's grip or the mantle's
/// ledge code; unused fields are zero.
fn encode_traversal(payload: &mut Vec<u8>, traversal: mmorpg_core::Traversal) {
    use mmorpg_core::Traversal;
    let (kind, ticks, wall, goal, across, detail) = match traversal {
        Traversal::Free => (0, 0, 0, [0; 3], false, 0),
        Traversal::Released { ticks } => (1, ticks, 0, [0; 3], false, 0),
        Traversal::Climbing { wall, grip } => (2, 0, wall, [0; 3], false, grip.code()),
        Traversal::Mantling {
            goal,
            wall,
            ticks,
            across,
            ledge,
        } => (3, ticks, wall, goal, across, ledge.code()),
    };
    payload.push(kind);
    payload.extend_from_slice(&ticks.to_be_bytes());
    payload.extend_from_slice(&wall.to_be_bytes());
    for axis in goal {
        payload.extend_from_slice(&axis.to_be_bytes());
    }
    payload.push(u8::from(across));
    payload.push(detail);
}

fn decode_traversal(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::Traversal, ProtocolError> {
    use mmorpg_core::Traversal;
    let kind = read_u8(payload, offset)?;
    let ticks = u16::from_be_bytes(take(payload, offset)?);
    let wall = u16::from_be_bytes(take(payload, offset)?);
    let goal = [
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ];
    let across = read_bool(payload, offset)?;
    let detail = read_u8(payload, offset)?;
    let unused = |ticks: u16, wall: u16, goal: [i32; 3], across: bool| {
        ticks == 0 && wall == 0 && goal == [0; 3] && !across
    };
    let malformed = || ProtocolError::new("player climb or mantle is malformed");
    let traversal = match kind {
        0 if unused(ticks, wall, goal, across) && detail == 0 => Traversal::Free,
        1 if unused(0, wall, goal, across) && detail == 0 => Traversal::Released { ticks },
        2 if unused(ticks, 0, goal, across) => Traversal::Climbing {
            wall,
            grip: mmorpg_core::Grip::from_code(detail).ok_or_else(malformed)?,
        },
        3 => Traversal::Mantling {
            goal,
            wall,
            ticks,
            across,
            ledge: mmorpg_core::Ledge::from_code(detail).ok_or_else(malformed)?,
        },
        _ => return Err(malformed()),
    };
    if !traversal.is_valid() {
        return Err(ProtocolError::new("player climb or mantle is out of range"));
    }
    Ok(traversal)
}

/// 5 bytes (#516): kind (0 ready, 1 pending, 2 rolling, 3 recovering), the roll's yaw and its
/// ticks; unused fields are zero.
fn encode_roll(payload: &mut Vec<u8>, roll: mmorpg_core::Roll) {
    use mmorpg_core::Roll;
    let (kind, yaw, ticks) = match roll {
        Roll::Ready => (0, 0, 0),
        Roll::Pending => (1, 0, 0),
        Roll::Rolling { yaw, ticks } => (2, yaw, ticks),
        Roll::Recovering { ticks } => (3, 0, ticks),
    };
    payload.push(kind);
    payload.extend_from_slice(&yaw.to_be_bytes());
    payload.extend_from_slice(&ticks.to_be_bytes());
}

fn decode_roll(payload: &[u8], offset: &mut usize) -> Result<mmorpg_core::Roll, ProtocolError> {
    use mmorpg_core::Roll;
    let kind = read_u8(payload, offset)?;
    let yaw = u16::from_be_bytes(take(payload, offset)?);
    let ticks = u16::from_be_bytes(take(payload, offset)?);
    let roll = match (kind, yaw, ticks) {
        (0, 0, 0) => Roll::Ready,
        (1, 0, 0) => Roll::Pending,
        (2, yaw, ticks) => Roll::Rolling { yaw, ticks },
        (3, 0, ticks) => Roll::Recovering { ticks },
        _ => return Err(ProtocolError::new("player dodge roll is malformed")),
    };
    if !roll.is_valid() {
        return Err(ProtocolError::new("player dodge roll is out of range"));
    }
    Ok(roll)
}

/// 20 bytes: ability (`u32`, 0 for none), elapsed ticks, target reference, point
/// present and the point (`2 × i32`, zero when absent).
fn encode_cast(payload: &mut Vec<u8>, cast: Option<CastState>) {
    let Some(cast) = cast else {
        payload.extend_from_slice(&[0; 20]);
        return;
    };
    payload.extend_from_slice(&cast.ability.get().to_be_bytes());
    payload.extend_from_slice(&cast.elapsed.to_be_bytes());
    encode_entity_ref(payload, cast.target);
    payload.push(u8::from(cast.point.is_some()));
    for component in cast.point.unwrap_or([0; 2]) {
        payload.extend_from_slice(&component.to_be_bytes());
    }
}

fn decode_cast(payload: &[u8], offset: &mut usize) -> Result<Option<CastState>, ProtocolError> {
    let ability = u32::from_be_bytes(take(payload, offset)?);
    let elapsed = u16::from_be_bytes(take(payload, offset)?);
    let target = decode_entity_ref(payload, offset)?;
    let has_point = read_bool(payload, offset)?;
    let point = [
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ];
    let point = match (has_point, point) {
        (true, point) => Some(point),
        (false, [0, 0]) => None,
        (false, _) => return Err(ProtocolError::new("malformed cast state")),
    };
    if ability == 0 {
        if elapsed != 0 || target.is_some() || point.is_some() {
            return Err(ProtocolError::new("malformed cast state"));
        }
        return Ok(None);
    }
    Ok(Some(CastState {
        ability: AbilityId::new(ability),
        elapsed,
        target,
        point,
    }))
}

/// A count, then 13 bytes per aura: ability (`u32`), caster reference, remaining
/// ticks and amount.
fn encode_auras(payload: &mut Vec<u8>, auras: &[Aura]) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        auras.len(),
        MAX_AURAS,
        "unit has too many auras",
    )?);
    for aura in auras {
        payload.extend_from_slice(&aura.ability.get().to_be_bytes());
        encode_entity_ref(payload, Some(aura.caster));
        payload.extend_from_slice(&aura.remaining.to_be_bytes());
        payload.extend_from_slice(&aura.amount.to_be_bytes());
    }
    Ok(())
}

fn decode_auras(payload: &[u8], offset: &mut usize) -> Result<Vec<Aura>, ProtocolError> {
    let count = decode_u8_count(payload, offset, MAX_AURAS, "unit has too many auras")?;
    let mut auras = Vec::with_capacity(count);
    for _ in 0..count {
        let ability = AbilityId::new(u32::from_be_bytes(take(payload, offset)?));
        let caster = decode_entity_ref(payload, offset)?
            .ok_or_else(|| ProtocolError::new("an aura needs a caster"))?;
        auras.push(Aura {
            ability,
            caster,
            remaining: u16::from_be_bytes(take(payload, offset)?),
            amount: u16::from_be_bytes(take(payload, offset)?),
        });
    }
    Ok(auras)
}

/// A canonical creature record: its whole continuation state, variable in length.
impl Codec for CanonicalCreatureSnapshot {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        let creature = self;
        payload.extend_from_slice(&creature.creature_id.get().to_be_bytes());
        payload.extend_from_slice(&creature.generation.to_be_bytes());
        payload.extend_from_slice(&creature.rng_state.to_be_bytes());
        payload.push(creature.level);
        payload.extend_from_slice(&creature.health.to_be_bytes());
        payload.extend_from_slice(&creature.facing.to_be_bytes());
        for component in creature.position.into_iter().chain(creature.velocity) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        let (life, died_at) = match creature.life {
            CreatureLife::Alive => (ALIVE, 0),
            CreatureLife::Corpse { died_at } => (CORPSE, died_at),
            CreatureLife::Despawned { died_at } => (DESPAWNED, died_at),
        };
        payload.push(life);
        payload.extend_from_slice(&died_at.to_be_bytes());
        let (ai, timer, destination) = match creature.ai {
            CreatureAi::Idle { timer, destination } => (IDLE, timer, destination),
            CreatureAi::Engaged => (ENGAGED, 0, None),
            CreatureAi::Evading { ticks } => (EVADING, ticks, None),
            CreatureAi::Rallying { ticks, point } => (RALLYING, ticks, Some(point)),
            CreatureAi::Trailing { ticks, .. } => (TRAILING, ticks, None),
        };
        payload.push(ai);
        payload.extend_from_slice(&timer.to_be_bytes());
        payload.push(u8::from(destination.is_some()));
        for component in destination.unwrap_or([0; 2]) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        // A trailing creature (#274) names the player it follows.
        if let CreatureAi::Trailing { player, .. } = creature.ai {
            payload.extend_from_slice(&player.to_be_bytes());
        }
        payload.push(encode_u8_count(
            creature.threat.len(),
            MAX_THREAT_ENTRIES,
            "creature threat table exceeds its capacity",
        )?);
        for entry in &creature.threat {
            encode_entity_ref(payload, Some(entry.entity));
            payload.extend_from_slice(&entry.threat.to_be_bytes());
        }
        payload.extend_from_slice(&creature.swing_timer.to_be_bytes());
        encode_creature_attack(payload, creature.attack);
        payload.extend_from_slice(&creature.combat_timer.to_be_bytes());
        payload.extend_from_slice(&creature.wary_ticks.to_be_bytes());
        payload.push(u8::from(creature.tapped_by.is_some()));
        payload.extend_from_slice(&creature.tapped_by.unwrap_or(0).to_be_bytes());
        payload.push(u8::from(creature.loot.is_some()));
        if let Some(rewards) = creature.loot {
            crate::loot::encode_rewards(payload, rewards);
        }
        payload.push(encode_u8_count(
            creature.loot_party.len(),
            mmorpg_core::MAX_PARTY_MEMBERS,
            "corpse loot party is too large",
        )?);
        for player in &creature.loot_party {
            payload.extend_from_slice(&player.to_be_bytes());
        }
        let abilities = &creature.abilities;
        payload.extend_from_slice(&abilities.ability_timer.to_be_bytes());
        encode_cast(payload, abilities.cast);
        encode_auras(payload, &abilities.auras)?;
        payload.push(u8::from(creature.encounter.is_some()));
        if let Some(encounter) = &creature.encounter {
            for players in [&encounter.participants, &encounter.contributors] {
                payload.push(encode_u8_count(
                    players.len(),
                    MAX_ENCOUNTER_PLAYERS,
                    "boss encounter exceeds its capacity",
                )?);
                for player in players {
                    payload.extend_from_slice(&player.to_be_bytes());
                }
            }
        }
        crate::raid::encode_member(payload, creature.raid);
        Ok(())
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let creature_id = CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
        let generation = u64::from_be_bytes(take(payload, offset)?);
        if generation == 0 {
            return Err(ProtocolError::new("invalid creature life identity"));
        }
        let rng_state = u64::from_be_bytes(take(payload, offset)?);
        let level = read_u8(payload, offset)?;
        let health = u32::from_be_bytes(take(payload, offset)?);
        let facing = u16::from_be_bytes(take(payload, offset)?);
        let position = read_f32_vector(payload, offset)?;
        let velocity = read_f32_vector(payload, offset)?;
        let life_code = read_u8(payload, offset)?;
        let died_at = u64::from_be_bytes(take(payload, offset)?);
        let life = match (life_code, died_at) {
            (ALIVE, 0) => CreatureLife::Alive,
            (CORPSE, died_at) => CreatureLife::Corpse { died_at },
            (DESPAWNED, died_at) => CreatureLife::Despawned { died_at },
            _ => return Err(ProtocolError::new("malformed creature life cycle")),
        };
        let ai_code = read_u8(payload, offset)?;
        let timer = u16::from_be_bytes(take(payload, offset)?);
        let has_destination = read_bool(payload, offset)?;
        let destination = [
            i32::from_be_bytes(take(payload, offset)?),
            i32::from_be_bytes(take(payload, offset)?),
        ];
        let destination = match (has_destination, destination) {
            (true, destination) => Some(destination),
            (false, [0, 0]) => None,
            (false, _) => return Err(ProtocolError::new("malformed creature destination")),
        };
        let ai = match (ai_code, timer, destination) {
            (IDLE, timer, destination) => CreatureAi::Idle { timer, destination },
            (ENGAGED, 0, None) => CreatureAi::Engaged,
            (EVADING, ticks, None) => CreatureAi::Evading { ticks },
            (RALLYING, ticks, Some(point)) => CreatureAi::Rallying { ticks, point },
            (TRAILING, ticks, None) => CreatureAi::Trailing {
                player: u32::from_be_bytes(take(payload, offset)?),
                ticks,
            },
            _ => return Err(ProtocolError::new("malformed creature AI state")),
        };
        let threat_count = decode_u8_count(
            payload,
            offset,
            MAX_THREAT_ENTRIES,
            "creature threat table exceeds its capacity",
        )?;
        let mut threat = Vec::with_capacity(threat_count);
        for _ in 0..threat_count {
            let entity: Option<EntityRef> = decode_entity_ref(payload, offset)?;
            let entity = entity.ok_or_else(|| ProtocolError::new("threat entry needs a unit"))?;
            threat.push(ThreatEntry {
                entity,
                threat: u32::from_be_bytes(take(payload, offset)?),
            });
        }
        let swing_timer = u16::from_be_bytes(take(payload, offset)?);
        let attack = decode_creature_attack(payload, offset)?;
        let combat_timer = u16::from_be_bytes(take(payload, offset)?);
        let wary_ticks = u16::from_be_bytes(take(payload, offset)?);
        let tapped = read_bool(payload, offset)?;
        let tapper = u32::from_be_bytes(take(payload, offset)?);
        let tapped_by = match (tapped, tapper) {
            (true, tapper) => Some(tapper),
            (false, 0) => None,
            (false, _) => return Err(ProtocolError::new("malformed creature tap")),
        };
        let loot = if read_bool(payload, offset)? {
            Some(crate::loot::decode_rewards(payload, offset)?)
        } else {
            None
        };
        let loot_party_count = decode_u8_count(
            payload,
            offset,
            mmorpg_core::MAX_PARTY_MEMBERS,
            "corpse loot party is too large",
        )?;
        let mut loot_party = Vec::with_capacity(loot_party_count);
        for _ in 0..loot_party_count {
            loot_party.push(u32::from_be_bytes(take(payload, offset)?));
        }
        let abilities = CanonicalCreatureAbilities {
            ability_timer: u16::from_be_bytes(take(payload, offset)?),
            cast: decode_cast(payload, offset)?,
            auras: decode_auras(payload, offset)?,
        };
        let encounter = if read_bool(payload, offset)? {
            let mut lists = [Vec::new(), Vec::new()];
            for players in &mut lists {
                let count = decode_u8_count(
                    payload,
                    offset,
                    MAX_ENCOUNTER_PLAYERS,
                    "boss encounter exceeds its capacity",
                )?;
                for _ in 0..count {
                    players.push(u32::from_be_bytes(take(payload, offset)?));
                }
            }
            let [participants, contributors] = lists;
            Some(mmorpg_core::BossEncounter {
                participants,
                contributors,
            })
        } else {
            None
        };
        let raid = crate::raid::decode_member(payload, offset)?;
        Ok(CanonicalCreatureSnapshot {
            generation,
            rng_state,
            creature_id,
            level,
            health,
            facing,
            position,
            velocity,
            life,
            ai,
            threat,
            swing_timer,
            attack,
            combat_timer,
            wary_ticks,
            tapped_by,
            loot,
            loot_party,
            abilities,
            encounter,
            raid,
        })
    }
}

pub fn decode_canonical_snapshot(payload: &[u8]) -> Result<CanonicalZoneSnapshot, ProtocolError> {
    let mut offset = 0;
    let (zone_id, tick) = decode_common_header(payload, &mut offset, CANONICAL_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let rng_state = u64::from_be_bytes(take(payload, &mut offset)?);
    let loot_rng_state = u64::from_be_bytes(take(payload, &mut offset)?);
    let world_seed = u64::from_be_bytes(take(payload, &mut offset)?);
    let player_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        players.push(CanonicalPlayerSnapshot::decode(payload, &mut offset)?);
    }
    let creature_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_CREATURE_SPAWNS,
        "snapshot exceeds the creature spawn capacity",
    )?;
    let mut creatures = Vec::with_capacity(creature_count);
    for _ in 0..creature_count {
        creatures.push(CanonicalCreatureSnapshot::decode(payload, &mut offset)?);
    }
    let doors = decode_canonical_doors(payload, &mut offset)?;
    let (next_projectile, projectiles) = decode_projectiles(payload, &mut offset)?;
    let forced = decode_forced(payload, &mut offset)?;
    let (next_debris, debris) = decode_debris(payload, &mut offset)?;
    let boulders = decode_boulders(payload, &mut offset)?;
    let traps = decode_traps(payload, &mut offset)?;
    let pieces = decode_pieces(payload, &mut offset)?;
    let (next_summoned_terrain, summoned_terrain) = decode_summoned_terrain(payload, &mut offset)?;
    let (next_minion, minions) = decode_minions(payload, &mut offset)?;
    let raid = crate::raid::decode_state(payload, &mut offset)?;
    let depths = crate::depths::decode_run(payload, &mut offset)?;
    let parties = decode_parties(payload, &mut offset)?;
    let wakes = decode_wakes(payload, &mut offset)?;
    let rewards = crate::rewards::decode_history(payload, &mut offset)?;
    let rewind_history = decode_rewind_history(payload, &mut offset)?;
    let gathering = decode_gathering(payload, &mut offset)?;
    let escorts = decode_escorts(payload, &mut offset)?;
    let loads = decode_loads(payload, &mut offset)?;
    ensure_fully_consumed(payload, offset)?;
    Ok(CanonicalZoneSnapshot {
        rewards,
        zone_id,
        tick,
        content_revision,
        rng_state,
        loot_rng_state,
        world_seed,
        players,
        creatures,
        doors,
        projectiles,
        next_projectile,
        forced,
        debris,
        next_debris,
        boulders,
        traps,
        pieces,
        summoned_terrain,
        next_summoned_terrain,
        minions,
        next_minion,
        raid,
        parties,
        wakes,
        escorts,
        loads,
        depths,
        rewind_history,
        gathering,
    })
}

/// Players' party views (#404): a count (`u16`), then per player in ID order its ID and view.
fn encode_parties(
    payload: &mut Vec<u8>,
    parties: &[(mmorpg_core::PlayerId, mmorpg_core::PartyState)],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(parties.len(), MAX_PLAYERS_PER_ZONE, "too many party views")?
            .to_be_bytes(),
    );
    for (player, state) in parties {
        payload.extend_from_slice(&player.to_be_bytes());
        crate::party::encode_state(payload, state)?;
    }
    Ok(())
}

fn decode_parties(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<(mmorpg_core::PlayerId, mmorpg_core::PartyState)>, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        MAX_PLAYERS_PER_ZONE,
        "too many party views",
    )?;
    let mut parties = Vec::with_capacity(count);
    for _ in 0..count {
        let player = u32::from_be_bytes(take(payload, offset)?);
        parties.push((player, crate::party::decode_state(payload, offset)?));
    }
    Ok(parties)
}

/// Woken private-home residents (#158): a count (`u8`), then per wake in NPC-ID order its NPC
/// (`u32`), knock tick and last quest request tick (`u64` each).
fn encode_wakes(
    payload: &mut Vec<u8>,
    wakes: &[mmorpg_core::NpcWake],
) -> Result<(), ProtocolError> {
    let count = u8::try_from(wakes.len())
        .map_err(|_| ProtocolError::new("npc wakes exceed their count"))?;
    payload.push(count);
    for wake in wakes {
        payload.extend_from_slice(&wake.npc.get().to_be_bytes());
        payload.extend_from_slice(&wake.knocked.to_be_bytes());
        payload.extend_from_slice(&wake.idle_since.to_be_bytes());
    }
    Ok(())
}

/// Running quest escorts (#1019): a count (`u8`), then per escort in NPC-ID order its NPC and
/// player (`u32`), feet XZ (`i32` each), facing (`u16`), velocity (`i8` each), ticks behind
/// (`u16`), rest end tick (`u64`) and its trail: a count (`u8`) of XZ points (`i32` each).
fn encode_escorts(
    payload: &mut Vec<u8>,
    escorts: &[mmorpg_core::EscortState],
) -> Result<(), ProtocolError> {
    let count = u8::try_from(escorts.len())
        .map_err(|_| ProtocolError::new("escorts exceed their count"))?;
    payload.push(count);
    for escort in escorts {
        payload.extend_from_slice(&escort.npc.get().to_be_bytes());
        payload.extend_from_slice(&escort.player.to_be_bytes());
        for axis in escort.position {
            payload.extend_from_slice(&axis.to_be_bytes());
        }
        payload.extend_from_slice(&escort.facing.to_be_bytes());
        payload.extend(escort.velocity.map(|axis| axis.to_be_bytes()[0]));
        payload.extend_from_slice(&escort.behind.to_be_bytes());
        payload.extend_from_slice(&escort.rests_until.to_be_bytes());
        let points = u8::try_from(escort.trail.len())
            .ok()
            .filter(|points| usize::from(*points) <= mmorpg_core::MAX_ESCORT_TRAIL)
            .ok_or_else(|| ProtocolError::new("an escort trail exceeds its count"))?;
        payload.push(points);
        for point in &escort.trail {
            for axis in point {
                payload.extend_from_slice(&axis.to_be_bytes());
            }
        }
    }
    Ok(())
}

/// Every player's load (#1077): a count (`u16`), then per load in player-ID order its player
/// and source (`u32`) and whether it is set down (`u8`); a set-down load adds its base (`i32`
/// each), facing (`u16`) and the tick it is gone (`u64`).
fn encode_loads(
    payload: &mut Vec<u8>,
    loads: &[mmorpg_core::LoadState],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            loads.len(),
            mmorpg_core::MAX_ZONE_LOADS,
            "loads exceed their count",
        )?
        .to_be_bytes(),
    );
    for load in loads {
        payload.extend_from_slice(&load.player.to_be_bytes());
        payload.extend_from_slice(&load.source.get().to_be_bytes());
        payload.push(u8::from(load.placed.is_some()));
        if let Some(placed) = load.placed {
            for axis in placed.at {
                payload.extend_from_slice(&axis.to_be_bytes());
            }
            payload.extend_from_slice(&placed.facing.to_be_bytes());
            payload.extend_from_slice(&placed.until.to_be_bytes());
        }
    }
    Ok(())
}

fn decode_loads(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::LoadState>, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        mmorpg_core::MAX_ZONE_LOADS,
        "loads exceed their count",
    )?;
    (0..count)
        .map(|_| {
            let player = u32::from_be_bytes(take(payload, offset)?);
            let source = mmorpg_core::LoadSpotId::new(u32::from_be_bytes(take(payload, offset)?));
            let placed = if read_bool(payload, offset)? {
                Some(mmorpg_core::PlacedLoad {
                    at: [
                        i32::from_be_bytes(take(payload, offset)?),
                        i32::from_be_bytes(take(payload, offset)?),
                        i32::from_be_bytes(take(payload, offset)?),
                    ],
                    facing: u16::from_be_bytes(take(payload, offset)?),
                    until: u64::from_be_bytes(take(payload, offset)?),
                })
            } else {
                None
            };
            Ok(mmorpg_core::LoadState {
                player,
                source,
                placed,
            })
        })
        .collect()
}

fn decode_escorts(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::EscortState>, ProtocolError> {
    let [count] = take(payload, offset)?;
    (0..count)
        .map(|_| {
            let npc = mmorpg_core::NpcId::new(u32::from_be_bytes(take(payload, offset)?));
            let player = u32::from_be_bytes(take(payload, offset)?);
            let position = [
                i32::from_be_bytes(take(payload, offset)?),
                i32::from_be_bytes(take(payload, offset)?),
            ];
            let facing = u16::from_be_bytes(take(payload, offset)?);
            let [x, z] = take(payload, offset)?;
            let behind = u16::from_be_bytes(take(payload, offset)?);
            let rests_until = u64::from_be_bytes(take(payload, offset)?);
            let [points] = take(payload, offset)?;
            if usize::from(points) > mmorpg_core::MAX_ESCORT_TRAIL {
                return Err(ProtocolError::new("an escort trail exceeds its count"));
            }
            let trail = (0..points)
                .map(|_| {
                    Ok([
                        i32::from_be_bytes(take(payload, offset)?),
                        i32::from_be_bytes(take(payload, offset)?),
                    ])
                })
                .collect::<Result<_, ProtocolError>>()?;
            Ok(mmorpg_core::EscortState {
                npc,
                player,
                position,
                facing,
                velocity: [i8::from_be_bytes([x]), i8::from_be_bytes([z])],
                trail,
                behind,
                rests_until,
            })
        })
        .collect()
}

fn decode_wakes(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::NpcWake>, ProtocolError> {
    let [count] = crate::wire::take(payload, offset)?;
    (0..count)
        .map(|_| {
            Ok(mmorpg_core::NpcWake {
                npc: mmorpg_core::NpcId::new(u32::from_be_bytes(crate::wire::take(
                    payload, offset,
                )?)),
                knocked: u64::from_be_bytes(crate::wire::take(payload, offset)?),
                idle_since: u64::from_be_bytes(crate::wire::take(payload, offset)?),
            })
        })
        .collect()
}

/// The longest hit-rewind history a snapshot may hold, in frames (#680).
const MAX_REWIND_FRAMES: usize =
    mmorpg_core::ticks_from_ms(mmorpg_core::MAX_HIT_REWIND_MS) as usize;

/// Where bodies stood for hit rewind (#680): a frame count (`u8`), then per frame its tick
/// (`u64`) and a body count (`u16`), then per body its reference and `i32` centre.
fn encode_rewind_history(
    payload: &mut Vec<u8>,
    frames: &[mmorpg_core::RewindFrame],
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        frames.len(),
        MAX_REWIND_FRAMES,
        "hit rewind history is too long",
    )?);
    for frame in frames {
        payload.extend_from_slice(&frame.tick.to_be_bytes());
        payload.extend_from_slice(
            &encode_u16_count(
                frame.bodies.len(),
                MAX_PLAYERS_PER_ZONE + MAX_CREATURE_SPAWNS,
                "hit rewind frame has too many bodies",
            )?
            .to_be_bytes(),
        );
        for &(entity, centre) in &frame.bodies {
            encode_entity_ref(payload, Some(entity));
            for axis in centre {
                payload.extend_from_slice(&axis.to_be_bytes());
            }
        }
    }
    Ok(())
}

fn decode_rewind_history(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::RewindFrame>, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        MAX_REWIND_FRAMES,
        "hit rewind history is too long",
    )?;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        let tick = u64::from_be_bytes(take(payload, offset)?);
        let bodies = decode_u16_count(
            payload,
            offset,
            MAX_PLAYERS_PER_ZONE + MAX_CREATURE_SPAWNS,
            "hit rewind frame has too many bodies",
        )?;
        let mut frame = mmorpg_core::RewindFrame {
            tick,
            bodies: Vec::with_capacity(bodies),
        };
        for _ in 0..bodies {
            let entity = crate::wire::decode_entity_ref(payload, offset)?
                .ok_or_else(|| ProtocolError::new("hit rewind body needs a unit"))?;
            let centre = [(); 3].map(|()| take(payload, offset).map(i32::from_be_bytes));
            let [x, y, z] = centre;
            frame.bodies.push((entity, [x?, y?, z?]));
        }
        frames.push(frame);
    }
    Ok(frames)
}

/// Summoned minions (#295): the next minion ID (`u32`) and count (`u16`), then per minion in ID
/// order its ID and summoner (`u32` each), summoning ability and level (`u8` each), health
/// (`u32`), facing (`u16`), order (9 bytes), fight target (entity reference), swing timer and
/// remaining ticks (`u16` each) and exact body position and velocity (`f32` × 3 each).
fn encode_minions(
    payload: &mut Vec<u8>,
    next_minion: u32,
    minions: &[mmorpg_core::CanonicalMinion],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&next_minion.to_be_bytes());
    let count = encode_u16_count(
        minions.len(),
        mmorpg_core::MAX_MINIONS,
        "snapshot exceeds the minion capacity",
    )?;
    payload.extend_from_slice(&count.to_be_bytes());
    for record in minions {
        let minion = &record.minion;
        payload.extend_from_slice(&minion.id.get().to_be_bytes());
        payload.extend_from_slice(&minion.summoner.to_be_bytes());
        payload.extend_from_slice(&minion.ability.get().to_be_bytes());
        payload.push(minion.level);
        payload.extend_from_slice(&minion.health.to_be_bytes());
        payload.extend_from_slice(&minion.facing.to_be_bytes());
        crate::wire::encode_minion_order(payload, minion.order);
        encode_entity_ref(payload, minion.target);
        payload.extend_from_slice(&minion.swing_timer.to_be_bytes());
        payload.extend_from_slice(&minion.remaining.to_be_bytes());
        for component in record.position.into_iter().chain(record.velocity) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
    }
    Ok(())
}

fn decode_minions(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u32, Vec<mmorpg_core::CanonicalMinion>), ProtocolError> {
    let next_minion = u32::from_be_bytes(take(payload, offset)?);
    let count = decode_u16_count(
        payload,
        offset,
        mmorpg_core::MAX_MINIONS,
        "snapshot exceeds the minion capacity",
    )?;
    let minions = (0..count)
        .map(|_| {
            let id = mmorpg_core::MinionId::new(u32::from_be_bytes(take(payload, offset)?));
            let summoner = u32::from_be_bytes(take(payload, offset)?);
            let ability = AbilityId::new(u32::from_be_bytes(take(payload, offset)?));
            let level = read_u8(payload, offset)?;
            let health = u32::from_be_bytes(take(payload, offset)?);
            let facing = u16::from_be_bytes(take(payload, offset)?);
            let order = crate::wire::decode_minion_order(payload, offset)?;
            let target = decode_entity_ref(payload, offset)?;
            let swing_timer = u16::from_be_bytes(take(payload, offset)?);
            let remaining = u16::from_be_bytes(take(payload, offset)?);
            let position = read_f32_vector(payload, offset)?;
            let velocity = read_f32_vector(payload, offset)?;
            Ok(mmorpg_core::CanonicalMinion {
                minion: mmorpg_core::Minion {
                    id,
                    summoner,
                    ability,
                    level,
                    health,
                    facing,
                    order,
                    target,
                    swing_timer,
                    remaining,
                },
                position,
                velocity,
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    Ok((next_minion, minions))
}

/// Summoned terrain (#293): the next ID (`u32`), count (`u16`), then per obstacle its ID
/// (`u32`), kind (`u8`), caster (`u32`), centre (`3 × i32`), facing (`u16`), health (`u32`)
/// and expiry tick (`u64`), in ID order.
fn encode_summoned_terrain(
    payload: &mut Vec<u8>,
    next: u32,
    obstacles: &[mmorpg_core::SummonedTerrain],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&next.to_be_bytes());
    payload.extend_from_slice(
        &encode_u16_count(
            obstacles.len(),
            mmorpg_core::MAX_SUMMONED_TERRAIN,
            "too much summoned terrain",
        )?
        .to_be_bytes(),
    );
    for obstacle in obstacles {
        payload.extend_from_slice(&obstacle.id.to_be_bytes());
        payload.push(obstacle.kind.code());
        payload.extend_from_slice(&obstacle.caster.to_be_bytes());
        for axis in obstacle.centre {
            payload.extend_from_slice(&axis.to_be_bytes());
        }
        payload.extend_from_slice(&obstacle.facing.to_be_bytes());
        payload.extend_from_slice(&obstacle.health.to_be_bytes());
        payload.extend_from_slice(&obstacle.expires_at.to_be_bytes());
    }
    Ok(())
}

fn decode_summoned_terrain(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u32, Vec<mmorpg_core::SummonedTerrain>), ProtocolError> {
    let next = u32::from_be_bytes(take(payload, offset)?);
    let count = decode_u16_count(
        payload,
        offset,
        mmorpg_core::MAX_SUMMONED_TERRAIN,
        "too much summoned terrain",
    )?;
    let obstacles = (0..count)
        .map(|_| {
            let id = u32::from_be_bytes(take(payload, offset)?);
            let kind = mmorpg_core::SummonedTerrainKind::from_code(read_u8(payload, offset)?)
                .ok_or_else(|| ProtocolError::new("unknown summoned terrain kind"))?;
            let caster = u32::from_be_bytes(take(payload, offset)?);
            let centre = [
                i32::from_be_bytes(take(payload, offset)?),
                i32::from_be_bytes(take(payload, offset)?),
                i32::from_be_bytes(take(payload, offset)?),
            ];
            Ok(mmorpg_core::SummonedTerrain {
                id,
                kind,
                caster,
                centre,
                facing: u16::from_be_bytes(take(payload, offset)?),
                health: u32::from_be_bytes(take(payload, offset)?),
                expires_at: u64::from_be_bytes(take(payload, offset)?),
            })
        })
        .collect::<Result<_, ProtocolError>>()?;
    Ok((next, obstacles))
}

/// Destructible pieces (#305): count (`u16`), then per piece its ID (`u32`), health (`u32`),
/// last change tick (`u64`) and repaired regrowth ticks (`u32`), in content order.
fn encode_pieces(
    payload: &mut Vec<u8>,
    pieces: &[mmorpg_core::CanonicalPiece],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            pieces.len(),
            usize::from(u16::MAX),
            "too many structure pieces",
        )?
        .to_be_bytes(),
    );
    for piece in pieces {
        payload.extend_from_slice(&piece.piece.get().to_be_bytes());
        payload.extend_from_slice(&piece.health.to_be_bytes());
        payload.extend_from_slice(&piece.changed_tick.to_be_bytes());
        payload.extend_from_slice(&piece.repaired.to_be_bytes());
    }
    Ok(())
}

fn decode_pieces(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::CanonicalPiece>, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        usize::from(u16::MAX),
        "too many structure pieces",
    )?;
    (0..count)
        .map(|_| {
            Ok(mmorpg_core::CanonicalPiece {
                piece: mmorpg_core::PieceId::new(u32::from_be_bytes(take(payload, offset)?)),
                health: u32::from_be_bytes(take(payload, offset)?),
                changed_tick: u64::from_be_bytes(take(payload, offset)?),
                repaired: u32::from_be_bytes(take(payload, offset)?),
            })
        })
        .collect()
}

/// A running timed swing (#679): a presence byte, then ability (`u32`), `u16` facing, aim, `u16`
/// elapsed, `u16` startup and active ticks, the units it struck (#909) and `u16` rewind.
fn encode_swing(payload: &mut Vec<u8>, swing: Option<mmorpg_core::Swing>) {
    let Some(swing) = swing else {
        payload.push(0);
        return;
    };
    payload.push(1);
    payload.extend_from_slice(&swing.ability.get().to_be_bytes());
    payload.extend_from_slice(&swing.facing.to_be_bytes());
    payload.extend_from_slice(&crate::wire::encode_aim(swing.aim));
    for ticks in [swing.elapsed, swing.startup, swing.active] {
        payload.extend_from_slice(&ticks.to_be_bytes());
    }
    for unit in swing.struck {
        encode_entity_ref(payload, unit);
    }
    payload.extend_from_slice(&swing.rewind.to_be_bytes());
}

fn decode_swing(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<mmorpg_core::Swing>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    let mut swing = mmorpg_core::Swing {
        ability: mmorpg_core::AbilityId::new(u32::from_be_bytes(take(payload, offset)?)),
        facing: u16::from_be_bytes(take(payload, offset)?),
        aim: crate::wire::decode_aim(take(payload, offset)?)?,
        elapsed: u16::from_be_bytes(take(payload, offset)?),
        startup: u16::from_be_bytes(take(payload, offset)?),
        active: u16::from_be_bytes(take(payload, offset)?),
        struck: [None; mmorpg_core::SWING_HITS],
        rewind: 0,
    };
    for unit in &mut swing.struck {
        *unit = decode_entity_ref(payload, offset)?;
    }
    swing.rewind = u16::from_be_bytes(take(payload, offset)?);
    if !swing.is_valid() {
        return Err(ProtocolError::new("player timed swing is out of range"));
    }
    Ok(Some(swing))
}

/// The basic strike's state (#885): the chain (`u8`), its window (`u16`), the follow-up window
/// (`u16`, #928), then a pending strike's family code plus one (`0` for none), its variant code,
/// its `u16` contact countdown, the Warden's stance code (`0` for none) and its ticks on the
/// ground (`u8`, #928).
fn encode_melee(payload: &mut Vec<u8>, melee: mmorpg_core::MeleeState) {
    payload.push(melee.chain);
    payload.extend_from_slice(&melee.window.to_be_bytes());
    payload.extend_from_slice(&melee.follow_up.to_be_bytes());
    let Some(pending) = melee.pending else {
        payload.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        return;
    };
    payload.push(pending.family.code() + 1);
    payload.push(pending.variant.code());
    payload.extend_from_slice(&pending.contact.to_be_bytes());
    payload.push(pending.stance.map_or(0, mmorpg_core::MeleeStance::code));
    payload.push(pending.grounded);
}

fn decode_melee(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::MeleeState, ProtocolError> {
    let invalid = || ProtocolError::new("player strike state is out of range");
    let chain = read_u8(payload, offset)?;
    let window = u16::from_be_bytes(take(payload, offset)?);
    let follow_up = u16::from_be_bytes(take(payload, offset)?);
    let family = read_u8(payload, offset)?;
    let variant = read_u8(payload, offset)?;
    let contact = u16::from_be_bytes(take(payload, offset)?);
    let stance = read_u8(payload, offset)?;
    let grounded = read_u8(payload, offset)?;
    let pending = if family == 0 {
        if (variant, contact, stance, grounded) != (0, 0, 0, 0) {
            return Err(invalid());
        }
        None
    } else {
        Some(mmorpg_core::PendingStrike {
            family: mmorpg_core::WeaponFamily::from_code(family - 1).ok_or_else(invalid)?,
            variant: mmorpg_core::StrikeVariant::from_code(variant).ok_or_else(invalid)?,
            contact,
            stance: match stance {
                0 => None,
                code => Some(mmorpg_core::MeleeStance::from_code(code).ok_or_else(invalid)?),
            },
            grounded,
        })
    };
    let melee = mmorpg_core::MeleeState {
        chain,
        window,
        follow_up,
        pending,
    };
    if !melee.is_valid() {
        return Err(invalid());
    }
    Ok(melee)
}

/// A beast's pending bite (#889): `0` for none, else `1 +` its attack code, the unit it began on
/// and its contact countdown.
fn encode_creature_attack(payload: &mut Vec<u8>, attack: Option<mmorpg_core::PendingAttack>) {
    let Some(attack) = attack else {
        payload.push(0);
        return;
    };
    payload.push(1 + attack.attack.code());
    encode_entity_ref(payload, Some(attack.target));
    payload.extend_from_slice(&attack.contact.to_be_bytes());
}

fn decode_creature_attack(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<mmorpg_core::PendingAttack>, ProtocolError> {
    let code = read_u8(payload, offset)?;
    if code == 0 {
        return Ok(None);
    }
    let malformed = || ProtocolError::new("malformed creature attack");
    let attack = mmorpg_core::CreatureAttack::from_code(code - 1).ok_or_else(malformed)?;
    let target = decode_entity_ref(payload, offset)?.ok_or_else(malformed)?;
    let contact = u16::from_be_bytes(take(payload, offset)?);
    Ok(Some(mmorpg_core::PendingAttack {
        attack,
        target,
        contact,
    }))
}

/// A pending trade: code, NPC (u32), offer or bag slot (u8), quantity (u16).
fn encode_trade_intent(
    payload: &mut Vec<u8>,
    code: u8,
    npc: mmorpg_core::NpcId,
    index: u32,
    quantity: u16,
) {
    payload.push(code);
    payload.extend_from_slice(&npc.get().to_be_bytes());
    payload.extend_from_slice(&index.to_be_bytes());
    payload.extend_from_slice(&quantity.to_be_bytes());
}

const TRACKING: u8 = 1;
const FREE: u8 = 2;
const STUCK: u8 = 3;
const BURST: u8 = 4;

/// Projectiles (#286): the next ID (`u32`), a `u16` count, then per projectile its ID
/// (`u32`), kind, owner, ability, exact position and velocity, age (`u16`), bounces (`u8`) and
/// the units it pierced (#291), and flight.
fn encode_projectiles(
    payload: &mut Vec<u8>,
    next: u32,
    projectiles: &[mmorpg_core::Projectile],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&next.to_be_bytes());
    payload.extend_from_slice(
        &u16::try_from(projectiles.len())
            .ok()
            .filter(|&count| usize::from(count) <= mmorpg_core::MAX_PROJECTILES)
            .ok_or_else(|| ProtocolError::new("too many projectiles"))?
            .to_be_bytes(),
    );
    for projectile in projectiles {
        payload.extend_from_slice(&projectile.id.to_be_bytes());
        payload.push(projectile.kind.code());
        encode_entity_ref(payload, Some(projectile.owner));
        payload.extend_from_slice(&projectile.ability.get().to_be_bytes());
        for component in projectile.position.into_iter().chain(projectile.velocity) {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&projectile.age.to_be_bytes());
        payload.push(projectile.bounces);
        for unit in projectile.struck {
            encode_entity_ref(payload, unit);
        }
        match projectile.flight {
            mmorpg_core::Flight::Tracking { target, remaining } => {
                payload.push(TRACKING);
                encode_entity_ref(payload, Some(target));
                payload.extend_from_slice(&remaining.to_be_bytes());
            }
            mmorpg_core::Flight::Free => payload.push(FREE),
            mmorpg_core::Flight::Stuck {
                unit,
                offset,
                remaining,
            } => {
                payload.push(STUCK);
                encode_entity_ref(payload, unit);
                for component in offset {
                    payload.extend_from_slice(&component.to_be_bytes());
                }
                payload.extend_from_slice(&remaining.to_be_bytes());
            }
            mmorpg_core::Flight::Burst { remaining } => {
                payload.push(BURST);
                payload.extend_from_slice(&remaining.to_be_bytes());
            }
        }
    }
    Ok(())
}

fn decode_projectiles(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u32, Vec<mmorpg_core::Projectile>), ProtocolError> {
    let next = u32::from_be_bytes(take(payload, offset)?);
    let count = decode_u16_count(
        payload,
        offset,
        mmorpg_core::MAX_PROJECTILES,
        "too many projectiles",
    )?;
    let mut projectiles = Vec::with_capacity(count);
    for _ in 0..count {
        let id = u32::from_be_bytes(take(payload, offset)?);
        let kind = mmorpg_core::ProjectileKind::from_code(crate::wire::read_u8(payload, offset)?)
            .ok_or_else(|| ProtocolError::new("unknown projectile kind"))?;
        let owner = decode_entity_ref(payload, offset)?
            .ok_or_else(|| ProtocolError::new("a projectile needs an owner"))?;
        let ability = mmorpg_core::AbilityId::new(u32::from_be_bytes(take(payload, offset)?));
        let position = crate::wire::read_f32_vector(payload, offset)?;
        let velocity = crate::wire::read_f32_vector(payload, offset)?;
        let age = u16::from_be_bytes(take(payload, offset)?);
        let bounces = crate::wire::read_u8(payload, offset)?;
        if bounces > mmorpg_core::RICOCHET_BOUNCES {
            return Err(ProtocolError::new("a projectile bounced too often"));
        }
        let mut struck = [None; mmorpg_core::PIERCE_HITS - 1];
        for unit in &mut struck {
            *unit = decode_entity_ref(payload, offset)?;
        }
        let flight = match crate::wire::read_u8(payload, offset)? {
            TRACKING => mmorpg_core::Flight::Tracking {
                target: decode_entity_ref(payload, offset)?
                    .ok_or_else(|| ProtocolError::new("a tracking projectile needs a target"))?,
                remaining: u16::from_be_bytes(take(payload, offset)?),
            },
            FREE => mmorpg_core::Flight::Free,
            STUCK => mmorpg_core::Flight::Stuck {
                unit: decode_entity_ref(payload, offset)?,
                offset: crate::wire::read_f32_vector(payload, offset)?,
                remaining: u16::from_be_bytes(take(payload, offset)?),
            },
            BURST => mmorpg_core::Flight::Burst {
                remaining: u16::from_be_bytes(take(payload, offset)?),
            },
            _ => return Err(ProtocolError::new("unknown projectile flight")),
        };
        projectiles.push(mmorpg_core::Projectile {
            id,
            kind,
            owner,
            ability,
            position,
            velocity,
            flight,
            age,
            struck,
            bounces,
        });
    }
    Ok((next, projectiles))
}

/// Boulders (#299): a `u16` count, then per boulder in creature-ID order its creature ID
/// (`u32`), spin (`f32`s), a pusher flag and player (`u32`) and idle ticks (`u16`). Position
/// and velocity are the creature record's.
fn encode_boulders(
    payload: &mut Vec<u8>,
    boulders: &[mmorpg_core::CanonicalBoulder],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(boulders.len(), MAX_BOULDERS, "too many boulders")?.to_be_bytes(),
    );
    for boulder in boulders {
        payload.extend_from_slice(&boulder.creature_id.get().to_be_bytes());
        for component in boulder.spin {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.push(u8::from(boulder.pusher.is_some()));
        payload.extend_from_slice(&boulder.pusher.unwrap_or(0).to_be_bytes());
        payload.extend_from_slice(&boulder.idle.to_be_bytes());
    }
    Ok(())
}

fn decode_boulders(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::CanonicalBoulder>, ProtocolError> {
    let count = decode_u16_count(payload, offset, MAX_BOULDERS, "too many boulders")?;
    let mut boulders = Vec::with_capacity(count);
    for _ in 0..count {
        let creature_id = mmorpg_core::CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
        let spin = read_f32_vector(payload, offset)?;
        let has_pusher = read_bool(payload, offset)?;
        let pusher = u32::from_be_bytes(take(payload, offset)?);
        let idle = u16::from_be_bytes(take(payload, offset)?);
        boulders.push(mmorpg_core::CanonicalBoulder {
            creature_id,
            spin,
            pusher: has_pusher.then_some(pusher),
            idle,
        });
    }
    Ok(boulders)
}

/// World traps (#300), in trap-ID order: a `u16` count, then per trap its ID (`u32`), a fired flag
/// and the fired tick (`u64`), and a `u8` hit count with, per hit in unit order, the unit
/// reference and the tick from which it may be hit again (`u64`).
fn encode_traps(
    payload: &mut Vec<u8>,
    traps: &[mmorpg_core::CanonicalTrap],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(traps.len(), usize::from(u16::MAX), "too many traps")?.to_be_bytes(),
    );
    for trap in traps {
        payload.extend_from_slice(&trap.id.get().to_be_bytes());
        payload.push(u8::from(trap.fired_at.is_some()));
        payload.extend_from_slice(&trap.fired_at.unwrap_or(0).to_be_bytes());
        payload.push(encode_u8_count(
            trap.hits.len(),
            mmorpg_core::MAX_TRAP_HITS,
            "too many trap hits",
        )?);
        for hit in &trap.hits {
            encode_entity_ref(payload, Some(hit.unit));
            payload.extend_from_slice(&hit.until.to_be_bytes());
        }
    }
    Ok(())
}

fn decode_traps(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::CanonicalTrap>, ProtocolError> {
    let count = decode_u16_count(payload, offset, usize::from(u16::MAX), "too many traps")?;
    let mut traps = Vec::new();
    for _ in 0..count {
        let id = mmorpg_core::TrapId::new(u32::from_be_bytes(take(payload, offset)?));
        let fired = read_bool(payload, offset)?;
        let tick = u64::from_be_bytes(take(payload, offset)?);
        let hit_count = decode_u8_count(
            payload,
            offset,
            mmorpg_core::MAX_TRAP_HITS,
            "too many trap hits",
        )?;
        let mut hits = Vec::with_capacity(hit_count);
        for _ in 0..hit_count {
            let unit = decode_entity_ref(payload, offset)?
                .ok_or_else(|| ProtocolError::new("trap hit needs a unit"))?;
            let until = u64::from_be_bytes(take(payload, offset)?);
            hits.push(mmorpg_core::CanonicalTrapHit { unit, until });
        }
        traps.push(mmorpg_core::CanonicalTrap {
            id,
            fired_at: fired.then_some(tick),
            hits,
        });
    }
    Ok(traps)
}

/// Debris (#288): the next ID (`u32`), a `u16` count, then per piece its ID (`u32`), prop kind
/// and piece index (`u8` each), position, rotation quaternion, velocity and spin (`f32`s), age
/// (`u16`) and resting flag. Half extents follow from kind and piece.
fn encode_debris(
    payload: &mut Vec<u8>,
    next: u32,
    debris: &[mmorpg_core::Debris],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&next.to_be_bytes());
    payload.extend_from_slice(
        &encode_u16_count(debris.len(), mmorpg_core::MAX_DEBRIS, "too much debris")?.to_be_bytes(),
    );
    for piece in debris {
        payload.extend_from_slice(&piece.id.to_be_bytes());
        payload.push(piece.kind.code());
        payload.push(piece.piece);
        let body = &piece.body;
        for component in body
            .position
            .iter()
            .chain(&body.rotation)
            .chain(&body.velocity)
            .chain(&body.spin)
        {
            payload.extend_from_slice(&component.to_be_bytes());
        }
        payload.extend_from_slice(&piece.age.to_be_bytes());
        payload.push(u8::from(body.resting));
    }
    Ok(())
}

fn decode_debris(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u32, Vec<mmorpg_core::Debris>), ProtocolError> {
    let next = u32::from_be_bytes(take(payload, offset)?);
    let count = decode_u16_count(payload, offset, mmorpg_core::MAX_DEBRIS, "too much debris")?;
    let mut debris = Vec::with_capacity(count);
    for _ in 0..count {
        let id = u32::from_be_bytes(take(payload, offset)?);
        let kind = mmorpg_core::PropKind::from_code(read_u8(payload, offset)?)
            .ok_or_else(|| ProtocolError::new("unknown debris kind"))?;
        let piece = read_u8(payload, offset)?;
        let [hx, hy, hz] = mmorpg_core::debris_pieces(kind)
            .get(usize::from(piece))
            .ok_or_else(|| ProtocolError::new("unknown debris piece"))?
            .half_extents;
        let position = read_f32_vector(payload, offset)?;
        let rotation = [
            f32::from_be_bytes(take(payload, offset)?),
            f32::from_be_bytes(take(payload, offset)?),
            f32::from_be_bytes(take(payload, offset)?),
            f32::from_be_bytes(take(payload, offset)?),
        ];
        let velocity = read_f32_vector(payload, offset)?;
        let spin = read_f32_vector(payload, offset)?;
        let age = u16::from_be_bytes(take(payload, offset)?);
        let resting = read_bool(payload, offset)?;
        debris.push(mmorpg_core::Debris {
            id,
            kind,
            piece,
            body: mmorpg_core::physics::DebrisBody {
                position,
                rotation,
                velocity,
                spin,
                half_extents: mmorpg_core::physics::Vec3i::new(hx, hy, hz),
                shape: mmorpg_core::physics::PieceShape::Cuboid,
                resting,
            },
            age,
        });
    }
    Ok((next, debris))
}

/// At most one forced-movement state per unit.
const MAX_FORCED: usize = MAX_PLAYERS_PER_ZONE + MAX_CREATURE_SPAWNS;
const MAX_BOULDERS: usize = MAX_CREATURE_SPAWNS;

/// Forced movement (#287): a `u16` count, then per unit its reference, the move in progress
/// (flag; XZ velocity `i32`s, remaining `u16`, charge flag with target and knockback `i32`) and
/// the immunity left (`u16`).
fn encode_forced(
    payload: &mut Vec<u8>,
    forced: &[(EntityRef, mmorpg_core::ForcedState)],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(forced.len(), MAX_FORCED, "too many forced moves")?.to_be_bytes(),
    );
    for (unit, state) in forced {
        encode_entity_ref(payload, Some(*unit));
        payload.push(u8::from(state.moving.is_some()));
        if let Some(moving) = state.moving {
            for axis in moving.velocity {
                payload.extend_from_slice(&axis.to_be_bytes());
            }
            payload.extend_from_slice(&moving.remaining.to_be_bytes());
            payload.push(u8::from(moving.charge.is_some()));
            if let Some((target, knockback)) = moving.charge {
                encode_entity_ref(payload, Some(target));
                payload.extend_from_slice(&knockback.to_be_bytes());
            }
            payload
                .extend_from_slice(&moving.leap.map_or(0, |ability| ability.get()).to_be_bytes());
        }
        payload.extend_from_slice(&state.immune.to_be_bytes());
    }
    Ok(())
}

fn decode_flag(payload: &[u8], offset: &mut usize) -> Result<bool, ProtocolError> {
    match crate::wire::read_u8(payload, offset)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProtocolError::new("invalid forced-movement flag")),
    }
}

fn decode_forced(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<(EntityRef, mmorpg_core::ForcedState)>, ProtocolError> {
    let count = decode_u16_count(payload, offset, MAX_FORCED, "too many forced moves")?;
    let mut forced = Vec::with_capacity(count);
    for _ in 0..count {
        let unit = decode_entity_ref(payload, offset)?
            .ok_or_else(|| ProtocolError::new("a forced move needs a unit"))?;
        let moving = if decode_flag(payload, offset)? {
            let velocity = [
                i32::from_be_bytes(take(payload, offset)?),
                i32::from_be_bytes(take(payload, offset)?),
            ];
            let remaining = u16::from_be_bytes(take(payload, offset)?);
            let charge = if decode_flag(payload, offset)? {
                let target = decode_entity_ref(payload, offset)?
                    .ok_or_else(|| ProtocolError::new("a charge needs a target"))?;
                Some((target, i32::from_be_bytes(take(payload, offset)?)))
            } else {
                None
            };
            let leap = match u32::from_be_bytes(take(payload, offset)?) {
                0 => None,
                id => Some(mmorpg_core::AbilityId::new(id)),
            };
            Some(mmorpg_core::ForcedMove {
                velocity,
                remaining,
                charge,
                leap,
            })
        } else {
            None
        };
        let immune = u16::from_be_bytes(take(payload, offset)?);
        forced.push((unit, mmorpg_core::ForcedState { moving, immune }));
    }
    Ok(forced)
}

/// Canonical doors: the projection's door list plus each leaf's last change tick (`u64`, #158).
fn encode_canonical_doors(
    payload: &mut Vec<u8>,
    doors: &[mmorpg_core::CanonicalDoor],
) -> Result<(), ProtocolError> {
    let views: Vec<_> = doors
        .iter()
        .map(|door| mmorpg_core::DoorView {
            door: door.door,
            open: door.open,
        })
        .collect();
    crate::wire::encode_doors(payload, &views)?;
    for door in doors {
        payload.extend_from_slice(&door.changed_tick.to_be_bytes());
    }
    Ok(())
}

fn decode_canonical_doors(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::CanonicalDoor>, ProtocolError> {
    let views = crate::wire::decode_doors(payload, offset)?;
    views
        .into_iter()
        .map(|view| {
            Ok(mmorpg_core::CanonicalDoor {
                door: view.door,
                open: view.open,
                changed_tick: u64::from_be_bytes(crate::wire::take(payload, offset)?),
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::decode_snapshot;
    use mmorpg_core::{ErrorCode, NpcId, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation};

    fn queued(intents: impl IntoIterator<Item = PlayerIntent>) -> Vec<mmorpg_core::QueuedIntent> {
        intents.into_iter().map(Into::into).collect()
    }

    /// The shared canonical golden snapshot (`fixtures/protocol/canonical-snapshot.hex`).
    pub(crate) fn snapshot() -> CanonicalZoneSnapshot {
        let wolf = EntityRef::Creature(CreatureId::new(4));
        CanonicalZoneSnapshot {
            parties: vec![(
                7,
                mmorpg_core::PartyState {
                    revision: 2,
                    roster: None,
                    invite: None,
                },
            )],
            rewards: None,
            debris: vec![mmorpg_core::Debris {
                id: 7,
                kind: mmorpg_core::PropKind::Fence,
                piece: 2,
                body: mmorpg_core::physics::DebrisBody {
                    position: [12.5, 3.25, -40.0],
                    rotation: [0.0, 0.6, 0.0, 0.8],
                    velocity: [1.5, -2.0, 0.25],
                    spin: [0.1, 0.0, -0.05],
                    half_extents: mmorpg_core::physics::Vec3i::new(92, 5, 3),
                    shape: mmorpg_core::physics::PieceShape::Cuboid,
                    resting: false,
                },
                age: 9,
            }],
            next_debris: 8,
            boulders: vec![mmorpg_core::CanonicalBoulder {
                creature_id: CreatureId::new(925),
                spin: [0.1, 0.0, -0.2],
                pusher: Some(7),
                idle: 42,
            }],
            traps: vec![mmorpg_core::CanonicalTrap {
                id: mmorpg_core::TrapId::new(3),
                fired_at: Some(1_200),
                hits: vec![mmorpg_core::CanonicalTrapHit {
                    unit: EntityRef::Player(1),
                    until: 1_250,
                }],
            }],
            summoned_terrain: vec![mmorpg_core::SummonedTerrain {
                id: 4,
                kind: mmorpg_core::SummonedTerrainKind::EarthPillar,
                caster: 9,
                centre: [120, 40, -300],
                facing: 0x2000,
                health: 35,
                expires_at: 900,
            }],
            next_summoned_terrain: 5,
            forced: vec![
                (
                    EntityRef::Player(1),
                    mmorpg_core::ForcedState {
                        moving: Some(mmorpg_core::ForcedMove {
                            velocity: [0, 45],
                            remaining: 12,
                            charge: Some((wolf, 300)),
                            leap: None,
                        }),
                        immune: 0,
                    },
                ),
                (
                    EntityRef::Player(2),
                    mmorpg_core::ForcedState {
                        moving: Some(mmorpg_core::ForcedMove {
                            velocity: [100, 0],
                            remaining: 40,
                            charge: None,
                            leap: Some(mmorpg_core::AbilityId::new(51)),
                        }),
                        immune: 0,
                    },
                ),
                (
                    wolf,
                    mmorpg_core::ForcedState {
                        moving: None,
                        immune: 17,
                    },
                ),
            ],
            next_projectile: 9,
            projectiles: vec![
                mmorpg_core::Projectile {
                    id: 7,
                    kind: mmorpg_core::ProjectileKind::Arrow,
                    owner: EntityRef::Player(1),
                    ability: mmorpg_core::ability::ids::AIMED_SHOT,
                    position: [1.5, 110.25, -3.0],
                    velocity: [180.0, 0.5, 0.0],
                    flight: mmorpg_core::Flight::Tracking {
                        target: wolf,
                        remaining: 4,
                    },
                    age: 2,
                    struck: [Some(wolf), None],
                    bounces: 0,
                },
                mmorpg_core::Projectile {
                    id: 8,
                    kind: mmorpg_core::ProjectileKind::Bolt,
                    owner: wolf,
                    ability: mmorpg_core::ability::ids::CROSSBOW_BOLT,
                    position: [10.0, 20.0, -30.0],
                    velocity: [0.0, -1.0, 150.0],
                    flight: mmorpg_core::Flight::Stuck {
                        unit: Some(EntityRef::Player(1)),
                        offset: [0.0, 15.0, -30.0],
                        remaining: 29,
                    },
                    age: 6,
                    struck: [None; 2],
                    bounces: 1,
                },
                mmorpg_core::Projectile {
                    id: 9,
                    kind: mmorpg_core::ProjectileKind::Bomb,
                    owner: EntityRef::Player(1),
                    ability: mmorpg_core::ability::ids::BLASTING_BOMB,
                    position: [40.0, 1.5, -12.0],
                    velocity: [0.0; 3],
                    flight: mmorpg_core::Flight::Burst { remaining: 11 },
                    age: 150,
                    struck: [None; 2],
                    bounces: 0,
                },
            ],
            loot_rng_state: 0,
            world_seed: 0x5eed_0000_0000_1111,
            zone_id: ZoneId::new(42),
            tick: 99,
            content_revision: 3,
            rng_state: u64::MAX,
            doors: vec![mmorpg_core::CanonicalDoor {
                door: mmorpg_core::DoorId::new(1),
                open: true,
                changed_tick: 3,
            }],
            players: vec![
                CanonicalPlayerSnapshot {
                    chat_ready_at: 0,
                    chat: Vec::new(),
                    copper: 0,
                    player_id: 7,
                    position: [10.0, 20.0, -30.0],
                    velocity: [1.0, -2.0, 70000.0],
                    facing: 0xfedc,
                    forward: -1,
                    strafe: 1,
                    jump_pending: true,
                    rise: -1,
                    breath: mmorpg_core::Breath {
                        remaining: 0,
                        drowning: 7,
                    },
                    last_sequence: 81,
                    spawn_slot: 3,
                    inventory: {
                        let mut bag = mmorpg_core::Inventory::default();
                        bag.insert(mmorpg_core::ItemId::new(1), 27).unwrap();
                        bag.insert(mmorpg_core::ItemId::new(2), 2).unwrap();
                        bag
                    },
                    inventory_revision: 4,
                    equipment: mmorpg_core::Equipment::from_slots([
                        Some(mmorpg_core::ItemId::new(3)),
                        None,
                        None,
                        Some(mmorpg_core::ItemId::new(7)),
                        None,
                        Some(mmorpg_core::ItemId::new(9)),
                    ])
                    .unwrap(),
                    combat: CanonicalPlayerCombat {
                        experience: 63,
                        level: 2,
                        health: 17,
                        target: Some(wolf),
                        swing_queued: true,
                        weapon_drawn: true,
                        swing_timer: 12,
                        combat_timer: 149,
                        calm_ticks: 0,
                        intents: queued([
                            PlayerIntent::SelectTarget(Some(EntityRef::Npc(NpcId::new(2)))),
                            PlayerIntent::SelectTarget(None),
                            PlayerIntent::CancelCast,
                            PlayerIntent::Swing { facing: 0x4000 },
                            PlayerIntent::ReleaseSpirit,
                            PlayerIntent::MoveItem {
                                source: 1,
                                destination: 15,
                                quantity: 4,
                            },
                        ]),
                        // The decoder checks structure; core checks that only a full queue drops.
                        intents_dropped: true,
                        last_observed: 0x0102_0304_0506,
                        events: vec![
                            ZoneEvent::DamageTaken {
                                source: wolf,
                                target: EntityRef::Player(7),
                                amount: 4,
                                critical: true,
                            },
                            ZoneEvent::Error {
                                code: ErrorCode::YouAreDead,
                                target: None,
                            },
                        ],
                        abilities: CanonicalPlayerAbilities::default(),
                        landing: mmorpg_core::LandingState {
                            airborne: false,
                            recovery: 7,
                            attacked_airborne: false,
                        },
                        sitting: mmorpg_core::Sitting::On(mmorpg_core::SeatId::new(3)),
                        traversal: mmorpg_core::Traversal::Mantling {
                            goal: [10, 20, -30],
                            wall: 16_384,
                            ticks: 5,
                            across: true,
                            ledge: mmorpg_core::Ledge::Mid,
                        },
                        roll: mmorpg_core::Roll::Recovering { ticks: 9 },
                        combo: Some(mmorpg_core::ComboOpening {
                            target: EntityRef::Creature(CreatureId::new(1)),
                            elapsed: 3,
                            hits: 2,
                        }),
                        strike_recovery: 4,
                        swing: Some(mmorpg_core::Swing {
                            ability: mmorpg_core::ability::ids::HEROIC_STRIKE,
                            facing: 1_234,
                            aim: mmorpg_core::StrikeAim::new(
                                mmorpg_core::StrikeSide::Right,
                                mmorpg_core::StrikeHeight::High,
                            ),
                            elapsed: 5,
                            startup: 18,
                            active: 9,
                            struck: [
                                Some(mmorpg_core::EntityRef::Creature(
                                    mmorpg_core::CreatureId::new(4),
                                )),
                                None,
                                None,
                            ],
                            rewind: 7,
                        }),
                        melee: mmorpg_core::MeleeState {
                            chain: 2,
                            window: 40,
                            follow_up: 0,
                            pending: Some(mmorpg_core::PendingStrike {
                                family: mmorpg_core::WeaponFamily::Spear,
                                variant: mmorpg_core::StrikeVariant::Second,
                                contact: 9,
                                stance: Some(mmorpg_core::MeleeStance::Planted),
                                grounded: 0,
                            }),
                        },
                        repairing: Some(mmorpg_core::PieceId::new(3)),
                    },
                    quests: mmorpg_core::QuestLog::default(),
                    waystones: mmorpg_core::WaystoneSet::EMPTY,
                    discoveries: std::collections::BTreeSet::new(),
                    caches: std::collections::BTreeSet::new(),
                    mastery: mmorpg_core::WeaponMastery {
                        sword: mmorpg_core::Mastery {
                            level: 1,
                            progress: 7,
                        },
                    },
                    practice: vec![mmorpg_core::Practice {
                        creature: CreatureId::new(1),
                        life: 2,
                        points: 3,
                    }],
                    character: None,
                    depths: mmorpg_core::DepthsRecord::default(),
                    physique: mmorpg_core::PhysiquePreset::Heavyset.physique(),
                },
                CanonicalPlayerSnapshot {
                    chat_ready_at: 0,
                    chat: Vec::new(),
                    copper: 0,
                    player_id: 8,
                    position: [0.0; 3],
                    velocity: [0.0; 3],
                    facing: 0,
                    forward: 0,
                    strafe: 0,
                    jump_pending: false,
                    rise: 0,
                    breath: mmorpg_core::Breath::FULL,
                    last_sequence: 0,
                    spawn_slot: 0,
                    inventory: mmorpg_core::Inventory::default(),
                    inventory_revision: 1,
                    equipment: mmorpg_core::Equipment::default(),
                    combat: CanonicalPlayerCombat::default(),
                    quests: mmorpg_core::QuestLog::default(),
                    waystones: mmorpg_core::WaystoneSet::EMPTY,
                    discoveries: std::collections::BTreeSet::new(),
                    caches: std::collections::BTreeSet::new(),
                    mastery: mmorpg_core::WeaponMastery::default(),
                    practice: Vec::new(),
                    character: None,
                    depths: mmorpg_core::DepthsRecord::default(),
                    physique: mmorpg_core::Physique::NEUTRAL,
                },
            ],
            creatures: vec![
                CanonicalCreatureSnapshot {
                    generation: 1,
                    rng_state: 0,
                    loot: None,
                    creature_id: CreatureId::new(4),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [100.0, 45.0, 200.0],
                    velocity: [-19.0, -1.0, 0.0],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Engaged,
                    threat: vec![ThreatEntry {
                        entity: EntityRef::Player(7),
                        threat: 12,
                    }],
                    swing_timer: 5,
                    attack: None,
                    combat_timer: 150,
                    wary_ticks: 0,
                    tapped_by: Some(7),
                    loot_party: Vec::new(),
                    abilities: CanonicalCreatureAbilities::default(),
                    encounter: Some(mmorpg_core::BossEncounter {
                        participants: vec![7],
                        contributors: vec![7],
                    }),
                    raid: None,
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
                    rng_state: 0,
                    loot: None,
                    creature_id: CreatureId::new(5),
                    level: 1,
                    health: 0,
                    facing: 3,
                    position: [300.0, 45.0, 200.0],
                    velocity: [0.0; 3],
                    life: CreatureLife::Corpse { died_at: 90 },
                    ai: CreatureAi::Idle {
                        timer: 0,
                        destination: None,
                    },
                    threat: Vec::new(),
                    swing_timer: 0,
                    attack: None,
                    combat_timer: 0,
                    wary_ticks: 0,
                    tapped_by: Some(8),
                    loot_party: Vec::new(),
                    abilities: CanonicalCreatureAbilities::default(),
                    encounter: None,
                    raid: Some(mmorpg_core::RaidMember {
                        incident: 2,
                        leg: mmorpg_core::RaidLeg::Retreat,
                        waypoint: 4,
                        anchor: [6_400, -1_200],
                        since: 5,
                    }),
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
                    rng_state: 0,
                    loot: None,
                    creature_id: CreatureId::new(6),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [500.0, 45.0, 200.0],
                    velocity: [8.0, 0.0, 0.0],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Idle {
                        timer: 290,
                        destination: Some([-5, 900]),
                    },
                    threat: Vec::new(),
                    swing_timer: 0,
                    attack: None,
                    combat_timer: 0,
                    wary_ticks: 0,
                    tapped_by: None,
                    loot_party: Vec::new(),
                    abilities: CanonicalCreatureAbilities::default(),
                    encounter: None,
                    raid: None,
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
                    rng_state: 0,
                    loot: None,
                    creature_id: CreatureId::new(7),
                    level: 1,
                    health: 12,
                    facing: 3,
                    position: [700.0, 45.0, 200.0],
                    velocity: [0.0, 0.0, 24.0],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Evading { ticks: 44 },
                    threat: Vec::new(),
                    swing_timer: 0,
                    attack: None,
                    combat_timer: 0,
                    wary_ticks: 0,
                    tapped_by: None,
                    loot_party: Vec::new(),
                    abilities: CanonicalCreatureAbilities::default(),
                    encounter: None,
                    raid: None,
                },
            ],
            pieces: vec![mmorpg_core::CanonicalPiece {
                piece: mmorpg_core::PieceId::new(3),
                health: 40,
                changed_tick: 77,
                repaired: 8,
            }],
            minions: vec![mmorpg_core::CanonicalMinion {
                minion: mmorpg_core::Minion {
                    id: mmorpg_core::MinionId::new(5),
                    summoner: 1,
                    ability: mmorpg_core::ability::ids::RAISE_SKELETON,
                    level: 6,
                    health: 80,
                    facing: 0x4000,
                    order: mmorpg_core::MinionOrder::Attack(wolf),
                    target: Some(wolf),
                    swing_timer: 12,
                    remaining: 900,
                },
                position: [1.5, 80.0, -2.25],
                velocity: [0.5, 0.0, -0.75],
            }],
            next_minion: 6,
            raid: mmorpg_core::RaidState {
                incidents: 2,
                active: Some(mmorpg_core::RaidIncident {
                    id: 2,
                    phase: 2,
                    phase_started: 3,
                    scaled_for: vec![0, 2],
                    participants: vec![1, 2],
                }),
                damage: 40,
                last: Some(mmorpg_core::RaidResolution {
                    incident: 1,
                    at: 1,
                    outcome: mmorpg_core::RaidOutcome::Lost,
                }),
                aftermath: std::collections::BTreeSet::new(),
                rewarded: vec![1, 2],
                cart_rewarded: vec![2],
                scouted: vec![2],
                warned: true,
                braced: std::collections::BTreeSet::from([0, 70_000]),
            },
            wakes: vec![mmorpg_core::NpcWake {
                npc: mmorpg_core::NpcId::new(1),
                knocked: 2,
                idle_since: 3,
            }],
            escorts: vec![mmorpg_core::EscortState {
                npc: mmorpg_core::NpcId::new(44),
                player: 1,
                position: [-5_600, 1_450],
                facing: 32_768,
                velocity: [-3, 4],
                trail: vec![[-5_500, 1_400], [-5_400, 1_300]],
                behind: 5,
                rests_until: 0,
            }],
            loads: vec![
                mmorpg_core::LoadState {
                    player: 1,
                    source: mmorpg_core::LoadSpotId::new(1),
                    placed: None,
                },
                mmorpg_core::LoadState {
                    player: 2,
                    source: mmorpg_core::LoadSpotId::new(1),
                    placed: Some(mmorpg_core::PlacedLoad {
                        at: [-6_000, 1_040, 1_100],
                        facing: 16_384,
                        until: 9_000,
                    }),
                },
            ],
            depths: None,
            rewind_history: vec![mmorpg_core::RewindFrame {
                tick: 41,
                bodies: vec![
                    (EntityRef::Player(7), [10, 90, -20]),
                    (
                        EntityRef::Creature(mmorpg_core::CreatureId::new(3)),
                        [-400, 45, 900],
                    ),
                ],
            }],
            gathering: mmorpg_core::GatheringState {
                veins: vec![0, 7_300],
                skinned: [mmorpg_core::CreatureId::new(3)].into(),
            },
        }
    }

    #[test]
    fn canonical_loot_stream_balance_rewards_and_pending_fences_round_trip() {
        let mut state = snapshot();
        state.loot_rng_state = 0xfedc_ba98_7654_3210;
        state.players[0].copper = u32::MAX;
        state.players[0]
            .combat
            .intents
            .push(mmorpg_core::QueuedIntent {
                intent: PlayerIntent::Loot(mmorpg_core::LootClaim {
                    creature: CreatureId::new(5),
                    died_at: 90,
                }),
                late: 9,
            });
        for item in [
            None,
            Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 20).unwrap()),
        ] {
            state.creatures[1].loot = Some(mmorpg_core::LootRewards {
                money: u32::MAX,
                item,
                // A quest item rides along whenever the test adds an item.
                quest_item: item
                    .map(|_| mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(10), 1).unwrap()),
            });
            let encoded = encode_canonical_snapshot(&state).unwrap();
            assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), state);
            for length in 0..encoded.len() {
                assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
            }
        }
    }

    #[test]
    fn canonical_class_casts_cooldowns_auras_and_creature_abilities_round_trip() {
        let mut state = snapshot();
        let wolf = EntityRef::Creature(CreatureId::new(4));
        let blizzard = AbilityId::new(12);
        state.players[0].combat.intents = queued([
            PlayerIntent::UseAbility {
                ability: 9,
                target: Some(wolf),
                facing: None,
            },
            PlayerIntent::UseAbility {
                ability: u32::MAX,
                target: None,
                facing: Some(0x8001),
            },
            PlayerIntent::DirectedStrike {
                ability: 1,
                facing: 0xabcd,
                aim: mmorpg_core::StrikeAim::new(
                    mmorpg_core::StrikeSide::Right,
                    mmorpg_core::StrikeHeight::Low,
                ),
            },
            PlayerIntent::CancelCast,
            PlayerIntent::ChooseClass {
                class: u8::MAX,
                sex: 1,
            },
            PlayerIntent::EquipItem { bag_slot: u8::MAX },
            PlayerIntent::UnequipItem { equipment_slot: 5 },
        ]);
        state.players[0].combat.abilities = CanonicalPlayerAbilities {
            class: Some(ClassChoice {
                class: mmorpg_core::PlayerClass::Arcanist,
                sex: mmorpg_core::Sex::Male,
            }),
            resource: 132,
            resource_ticks: 29,
            mana_delay: 150,
            global_cooldown: 44,
            global_cooldown_total: 90,
            guard: mmorpg_core::GuardState {
                raised: Some(mmorpg_core::StrikeHeight::Low),
                broken: 7,
            },
            // Pools in creature order (#936); the wire does not judge the class.
            reckoning: vec![
                mmorpg_core::Harm {
                    creature: CreatureId::new(4),
                    amount: 37,
                },
                mmorpg_core::Harm {
                    creature: CreatureId::new(9),
                    amount: 200,
                },
            ],
            cooldowns: vec![
                Cooldown {
                    ability: AbilityId::new(10),
                    remaining: 600,
                },
                Cooldown {
                    ability: AbilityId::new(11),
                    remaining: 1,
                },
            ],
            cast: Some(CastState {
                ability: blizzard,
                elapsed: 31,
                target: Some(wolf),
                point: Some([i32::MIN, i32::MAX]),
            }),
            auras: vec![Aura {
                ability: AbilityId::new(11),
                caster: EntityRef::Player(7),
                remaining: 600,
                amount: u16::MAX,
            }],
        };
        state.creatures[0].abilities = CanonicalCreatureAbilities {
            ability_timer: u16::MAX,
            cast: Some(CastState {
                ability: AbilityId::new(13),
                elapsed: 44,
                target: Some(EntityRef::Player(7)),
                point: None,
            }),
            auras: (0..8)
                .map(|caster| Aura {
                    ability: AbilityId::new(6),
                    caster: EntityRef::Player(caster),
                    remaining: 1,
                    amount: 34,
                })
                .collect(),
        };
        let encoded = encode_canonical_snapshot(&state).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), state);
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let mut too_many = state.clone();
        too_many.creatures[0].abilities.auras.push(Aura {
            ability: AbilityId::new(7),
            caster: EntityRef::Player(9),
            remaining: 1,
            amount: 50,
        });
        assert_eq!(
            encode_canonical_snapshot(&too_many)
                .unwrap_err()
                .to_string(),
            "unit has too many auras"
        );
        // Structural checks: a class code beyond the six choices, a point
        // flag without a point and an aura without a caster.
        // The intent count follows the player's timers; the seven intents
        // occupy 13 + 13 + 9 + 6 + 3 + 2 + 2 bytes after their lateness byte each, then
        // the dropped flag, the last observation (u64) and two events.
        let intents = 16 + 32 + 2 + 198;
        let player_abilities =
            intents + 1 + (14 + 14 + 10 + 7 + 4 + 3 + 3) + 1 + 8 + 1 + 2 * EVENT_BYTES;
        assert_eq!(encoded[player_abilities], 6, "Arcanist, male");
        let cast = player_abilities + 1 + 10 + 1 + 2 * 6;
        let aura = cast + 20 + 1;
        for (offset, value, message) in [
            (player_abilities, 15, "unknown class choice"),
            // The directed strike's side code.
            (intents + 1 + 14 + 14 + 8, 3, "unknown strike aim"),
            (cast + 11, 2, "boolean field must be 0 or 1"),
            (aura + 4, 0, "an absent entity must have ID 0"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let mut no_cast = state;
        no_cast.players[0].combat.abilities.cast = None;
        let encoded = encode_canonical_snapshot(&no_cast).unwrap();
        let mut stray = encoded;
        stray[cast + 5] = 1;
        assert_eq!(
            decode_canonical_snapshot(&stray).unwrap_err().to_string(),
            "malformed cast state"
        );
    }

    const EVENT_BYTES: usize = crate::EVENT_RECORD_BYTES;

    #[test]
    fn canonical_snapshot_round_trip_preserves_continuation_state() {
        let snapshot = snapshot();
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(
            crate::fixture::golden_wire_hex(
                mmorpg_core::golden::workspace_root()
                    .join("fixtures/protocol/canonical-snapshot.hex"),
                &encoded,
            ),
            encoded
        );
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        // A canonical snapshot is never a player projection.
        assert!(decode_snapshot(&encoded).is_err());
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_canonical_snapshot(&trailing).is_err());
        // A lurker trailing a diver (#274) names the player it follows.
        let mut trailer = snapshot.clone();
        trailer.creatures[0].ai = CreatureAi::Trailing {
            player: 7,
            ticks: 120,
        };
        let bytes = encode_canonical_snapshot(&trailer).unwrap();
        assert_eq!(bytes.len(), encoded.len() + 4);
        assert_eq!(decode_canonical_snapshot(&bytes).unwrap(), trailer);
        // Header 16, identity and world seed 32, then the first player record.
        let jump_flag = 16 + 32 + 2 + 32;
        assert_eq!(encoded[jump_flag], 1);
        for (offset, value, message) in [
            (jump_flag, 2, "jump flag must be 0 or 1"),
            (0, encoded[0] ^ 1, "unsupported snapshot protocol id"),
            (3, 2, "unexpected snapshot scope"),
            (48, 2, "snapshot exceeds configured zone player capacity"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn pending_trades_round_trip_with_exact_records() {
        let mut snapshot = snapshot();
        let npc = mmorpg_core::NpcId::new(0x0102_0304);
        snapshot.players[0].combat.intents = queued([
            PlayerIntent::BuyItem {
                npc,
                offer: 6,
                quantity: 0x0a0b,
            },
            PlayerIntent::SellItem {
                npc: mmorpg_core::NpcId::new(3),
                bag_slot: 15,
                quantity: u16::MAX,
            },
        ]);
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        let buy = [0, BUY_ITEM_INTENT, 1, 2, 3, 4, 0, 0, 0, 6, 0x0a, 0x0b];
        let sell = [0, SELL_ITEM_INTENT, 0, 0, 0, 3, 0, 0, 0, 15, 0xff, 0xff];
        let records: Vec<u8> = [buy, sell].concat();
        assert!(
            encoded
                .windows(records.len())
                .any(|window| window == records.as_slice()),
            "pending trades are 11-byte records after their lateness"
        );
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
    }

    /// The quest log below: two turned-in quests, then quests 3 and 32 with their progress.
    const LOG: [u8; 25] = [
        0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 2, 0, 0, 0, 3, 1, 0, 0, 0, 0, 0, 32, 0xff, 2, 7,
    ];

    #[test]
    fn quest_logs_and_pending_quest_intents_round_trip_with_exact_records() {
        let mut snapshot = snapshot();
        let npc = mmorpg_core::NpcId::new(0x0102_0304);
        snapshot.players[0].combat.intents = queued([
            PlayerIntent::AcceptQuest { npc, quest: 2 },
            PlayerIntent::CompleteQuest {
                npc: mmorpg_core::NpcId::new(1),
                quest: 9,
                choice: u8::MAX,
            },
            PlayerIntent::AbandonQuest { quest: 0 },
            PlayerIntent::Inspect(0x0506_0708),
            PlayerIntent::QuestAction(mmorpg_core::NpcId::new(0x0102_0304)),
            PlayerIntent::Lift(Some(mmorpg_core::LoadSpotId::new(0x0A0B_0C0D))),
            PlayerIntent::Lift(None),
            PlayerIntent::SetDown,
        ]);
        snapshot.players[0].quests = mmorpg_core::QuestLog {
            entries: vec![
                mmorpg_core::QuestEntry {
                    quest: mmorpg_core::QuestId::new(3),
                    progress: [1, 0, 0],
                },
                mmorpg_core::QuestEntry {
                    quest: mmorpg_core::QuestId::new(32),
                    progress: [u8::MAX, 2, 7],
                },
            ],
            completed: [mmorpg_core::QuestId::new(1), mmorpg_core::QuestId::new(2)].into(),
        };
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        let intents = [
            &[0, ACCEPT_QUEST_INTENT, 1, 2, 3, 4, 0, 0, 0, 2][..],
            &[0, COMPLETE_QUEST_INTENT, 0, 0, 0, 1, 0, 0, 0, 9, 0xff],
            &[0, ABANDON_QUEST_INTENT, 0, 0, 0, 0],
            &[0, INSPECT_INTENT, 5, 6, 7, 8],
            &[0, QUEST_ACTION_INTENT, 1, 2, 3, 4],
            &[0, LIFT_INTENT, 0x0A, 0x0B, 0x0C, 0x0D],
            &[0, LIFT_INTENT, 0, 0, 0, 0],
            &[0, SET_DOWN_INTENT],
        ]
        .concat();
        let log = LOG.to_vec();
        for records in [intents, log] {
            assert!(
                encoded
                    .windows(records.len())
                    .any(|window| window == records.as_slice()),
                "{records:?}"
            );
        }
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let at = encoded
            .windows(LOG.len())
            .position(|window| window == LOG)
            .unwrap();
        for (offset, value, message) in [
            (at + 5, 0, "quest id is out of range"),
            (at + 9, 1, "turned-in quests are not ordered"),
            (at + 10, 11, "quest log exceeds its capacity"),
            (at + 14, 0, "quest id is out of range"),
            (at + 21, 3, "quest log entries are not ordered"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn canonical_decoding_rejects_malformed_unit_state() {
        let encoded = encode_canonical_snapshot(&snapshot()).unwrap();
        // Player 7's queued-swing flag follows its target reference.
        let player = 16 + 32 + 2;
        let swing_queued = player + 44 + 132 + 1 + 4 + 4 + 5;
        let intents = swing_queued + 2 + 6;
        let mut cases = vec![
            (swing_queued, 2, "boolean field must be 0 or 1"),
            (swing_queued + 1, 2, "boolean field must be 0 or 1"),
            (intents, 17, "player has too many pending intents"),
            // Each intent is its lateness, then its code and fields.
            (intents + 2, 99, "malformed pending intent"),
        ];
        // The third intent (CancelCast) carries no target.
        cases.push((intents + 1 + 2 * 7 + 2, 2, "malformed pending intent"));
        // The dropped-intent flag follows the six intents; the fourth (a swing) is 4 bytes.
        cases.push((intents + 1 + 6 * 7 - 3, 2, "boolean field must be 0 or 1"));
        cases.push((
            intents + 1 + 5 * 7 + 3,
            1,
            "malformed pending inventory intent",
        ));
        for (offset, value, message) in cases {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let without_players = CanonicalZoneSnapshot {
            players: Vec::new(),
            ..snapshot()
        };
        let encoded = encode_canonical_snapshot(&without_players).unwrap();
        let creature = 16 + 32 + 2 + 2;
        let life = creature + 4 + 8 + 8 + 1 + 4 + 2 + 24;
        let ai = life + 9;
        for (offset, value, message) in [
            (life, 4, "malformed creature life cycle"),
            (life + 8, 1, "malformed creature life cycle"),
            (ai, 9, "malformed creature AI state"),
            (ai + 3, 2, "boolean field must be 0 or 1"),
            (ai + 4, 1, "malformed creature destination"),
            (ai + 12, 65, "creature threat table exceeds its capacity"),
            (ai + 13, 0, "an absent entity must have ID 0"),
            // A pending bite (#889) follows the swing timer: `0` for none.
            (ai + 12 + 1 + 9 + 2, 3, "malformed creature attack"),
            // The tap follows the combat timer and the wary ticks (#949).
            (ai + 12 + 1 + 9 + 7, 2, "boolean field must be 0 or 1"),
            (ai + 12 + 1 + 9 + 7, 0, "malformed creature tap"),
            (50, 5, "snapshot exceeds the creature spawn capacity"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn canonical_equipment_rejects_unknown_and_misplaced_items() {
        let encoded = encode_canonical_snapshot(&snapshot()).unwrap();
        // Player 7's equipment follows its 104-byte inventory block (revision and slots).
        let equipment = 16 + 32 + 2 + 44 + 104;
        assert_eq!(encoded[equipment..equipment + 4], [0, 0, 0, 3]);
        for (slot, item, message) in [
            (0, 99, "unknown item"),
            (1, 2, "item cannot be equipped there"),
            (2, 1, "item cannot be equipped there"),
        ] {
            let mut invalid = encoded.clone();
            invalid[equipment + 4 * slot + 3] = item;
            assert_eq!(
                decode_canonical_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "slot {slot} = item {item}"
            );
        }
    }

    #[test]
    fn canonical_wire_restores_a_zone_and_its_content_identity() {
        let mut zone =
            ZoneSimulation::with_definition(ZoneId::new(1), ZoneDefinition::default()).unwrap();
        zone.add_player(1).unwrap();
        let state = zone.snapshot().unwrap();
        let encoded = encode_canonical_snapshot(&state).unwrap();
        let decoded = decode_canonical_snapshot(&encoded).unwrap();
        assert_eq!(decoded, state);
        let restored = ZoneSimulation::from_snapshot(decoded, zone.content().clone()).unwrap();
        assert_eq!(restored.snapshot().unwrap(), state);
        let other = ZoneSimulation::with_definition(
            ZoneId::new(1),
            ZoneDefinition::new(mmorpg_core::STANDARD_GRAVITY, Vec::new()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ZoneSimulation::from_snapshot(state, other.content().clone())
                .err()
                .unwrap()
                .message(),
            "snapshot content identity does not match the supplied zone content"
        );
    }
    #[test]
    fn canonical_combo_keeps_player_and_creature_target_kinds_distinct() {
        for target in [
            EntityRef::Player(2),
            EntityRef::Creature(CreatureId::new(2)),
        ] {
            let mut state = snapshot();
            state.players[0].combat.combo = Some(mmorpg_core::ComboOpening {
                target,
                elapsed: 3,
                hits: 1,
            });
            let decoded =
                decode_canonical_snapshot(&encode_canonical_snapshot(&state).unwrap()).unwrap();
            assert_eq!(
                decoded.players[0].combat.combo,
                state.players[0].combat.combo
            );
        }
    }

    /// A finite `f32` from anywhere in its range.
    fn finite(rng: &mut crate::codec::generate::Gen) -> f32 {
        loop {
            let value = f32::from_bits(rng.int());
            if value.is_finite() {
                return value;
            }
        }
    }

    /// The golden snapshot's players and creatures, each with its free physics state and
    /// identity drawn at random, round-trip record by record.
    #[test]
    fn canonical_players_and_creatures_round_trip() {
        use crate::codec::generate::{CASES, Gen, round_trip};
        let sample = snapshot();
        assert!(!sample.players.is_empty() && !sample.creatures.is_empty());
        let mut rng = Gen::new(9);
        for _ in 0..CASES / 8 {
            for player in &sample.players {
                let mut player = player.clone();
                player.position = [finite(&mut rng), finite(&mut rng), finite(&mut rng)];
                player.velocity = [finite(&mut rng), finite(&mut rng), finite(&mut rng)];
                player.facing = rng.int();
                player.last_sequence = rng.int();
                round_trip(&player, None);
            }
            for creature in &sample.creatures {
                let mut creature = creature.clone();
                creature.position = [finite(&mut rng), finite(&mut rng), finite(&mut rng)];
                creature.velocity = [finite(&mut rng), finite(&mut rng), finite(&mut rng)];
                creature.facing = rng.int();
                creature.generation = rng.int::<u64>().max(1);
                creature.rng_state = rng.int();
                round_trip(&creature, None);
            }
        }
    }
}
