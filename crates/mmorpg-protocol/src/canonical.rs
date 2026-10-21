//! Canonical scope: complete continuation state for trusted replay and
//! recovery. Content is referenced by revision and fingerprint; the decoder
//! checks structure, and `ZoneSimulation::from_snapshot` checks the state
//! against the content.

use mmorpg_core::{
    AbilityId, Aura, CanonicalCreatureAbilities, CanonicalCreatureSnapshot,
    CanonicalPlayerAbilities, CanonicalPlayerCombat, CanonicalPlayerSnapshot,
    CanonicalZoneSnapshot, CastState, ClassChoice, Cooldown, CreatureAi, CreatureId, CreatureLife,
    EntityRef, MAX_AURAS, MAX_CHAT_PER_TICK, MAX_COOLDOWNS, MAX_CREATURE_SPAWNS,
    MAX_EVENTS_PER_PLAYER, MAX_PENDING_INTENTS, MAX_PLAYERS_PER_ZONE, MAX_THREAT_ENTRIES,
    PlayerIntent, ThreatEntry,
};

use crate::ProtocolError;
use crate::inventory::{decode_equipment, decode_inventory, encode_equipment, encode_inventory};
use crate::wire::{
    CANONICAL_SNAPSHOT_SCOPE, decode_common_header, decode_entity_ref, decode_event,
    decode_u8_count, decode_u16_count, encode_common_header, encode_entity_ref, encode_event,
    encode_u8_count, encode_u16_count, ensure_fully_consumed, read_bool, read_f32_vector, read_u8,
    take,
};

const SELECT_TARGET_INTENT: u8 = 1;
const START_ATTACK_INTENT: u8 = 2;
const STOP_ATTACK_INTENT: u8 = 3;
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
const ALIVE: u8 = 1;
const CORPSE: u8 = 2;
const DESPAWNED: u8 = 3;
const IDLE: u8 = 1;
const ENGAGED: u8 = 2;
const EVADING: u8 = 3;

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
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
    )?;
    payload.extend_from_slice(&snapshot.content_revision.to_be_bytes());
    payload.extend_from_slice(&snapshot.content_fingerprint.to_be_bytes());
    payload.extend_from_slice(&snapshot.rng_state.to_be_bytes());
    payload.extend_from_slice(&snapshot.loot_rng_state.to_be_bytes());
    payload.extend_from_slice(&player_count.to_be_bytes());
    for player in &snapshot.players {
        encode_player(&mut payload, player)?;
    }
    payload.extend_from_slice(&creature_count.to_be_bytes());
    for creature in &snapshot.creatures {
        encode_creature(&mut payload, creature)?;
    }
    encode_canonical_doors(&mut payload, &snapshot.doors)?;
    encode_projectiles(
        &mut payload,
        snapshot.next_projectile,
        &snapshot.projectiles,
    )?;
    encode_forced(&mut payload, &snapshot.forced)?;
    encode_debris(&mut payload, snapshot.next_debris, &snapshot.debris)?;
    encode_pieces(&mut payload, &snapshot.pieces)?;
    crate::rewards::encode_history(&mut payload, snapshot.rewards.as_ref())?;
    Ok(payload)
}

fn encode_player(
    payload: &mut Vec<u8>,
    player: &CanonicalPlayerSnapshot,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&player.player_id.to_be_bytes());
    for component in player.position.into_iter().chain(player.velocity) {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&player.facing.to_be_bytes());
    payload.extend_from_slice(&player.forward.to_be_bytes());
    payload.extend_from_slice(&player.strafe.to_be_bytes());
    payload.push(u8::from(player.jump_pending));
    payload.extend_from_slice(&player.last_sequence.to_be_bytes());
    payload.extend_from_slice(&player.spawn_slot.to_be_bytes());
    payload.extend_from_slice(&player.inventory_revision.to_be_bytes());
    payload.extend_from_slice(&player.inventory_changed_at.to_be_bytes());
    encode_inventory(payload, &player.inventory);
    encode_equipment(payload, &player.equipment);
    payload.extend_from_slice(&player.copper.to_be_bytes());
    let combat = &player.combat;
    payload.push(combat.level);
    payload.extend_from_slice(&combat.experience.to_be_bytes());
    payload.extend_from_slice(&combat.health.to_be_bytes());
    encode_entity_ref(payload, combat.target);
    payload.push(u8::from(combat.auto_attack));
    for timer in [
        combat.swing_timer,
        combat.combat_timer,
        combat.calm_ticks,
        combat.error_cooldown,
    ] {
        payload.extend_from_slice(&timer.to_be_bytes());
    }
    payload.push(encode_u8_count(
        combat.intents.len(),
        MAX_PENDING_INTENTS,
        "player has too many pending intents",
    )?);
    for intent in &combat.intents {
        let (code, target) = match *intent {
            PlayerIntent::SelectTarget(target) => (SELECT_TARGET_INTENT, target),
            PlayerIntent::StartAttack => (START_ATTACK_INTENT, None),
            PlayerIntent::Loot(claim) => {
                payload.push(LOOT_INTENT);
                crate::loot::encode_claim(payload, claim);
                continue;
            }
            PlayerIntent::StopAttack => (STOP_ATTACK_INTENT, None),
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
            PlayerIntent::UseAbility { ability, target } => {
                payload.extend_from_slice(&[USE_ABILITY_INTENT, ability]);
                encode_entity_ref(payload, target);
                continue;
            }
            PlayerIntent::DirectedStrike {
                ability,
                facing,
                aim,
            } => {
                payload.extend_from_slice(&[DIRECTED_STRIKE_INTENT, ability]);
                payload.extend_from_slice(&facing.to_be_bytes());
                payload.extend_from_slice(&crate::wire::encode_aim(aim));
                continue;
            }
            PlayerIntent::CancelCast => (CANCEL_CAST_INTENT, None),
            PlayerIntent::Guard(lane) => {
                payload.extend_from_slice(&[GUARD_INTENT, crate::wire::encode_lane(lane)]);
                continue;
            }
            PlayerIntent::UseDoor { door, open } => {
                payload.extend_from_slice(&[USE_DOOR_INTENT, door.get(), u8::from(open)]);
                continue;
            }
            PlayerIntent::Repair(piece) => {
                payload.push(REPAIR_INTENT);
                payload.extend_from_slice(&crate::wire::encode_piece(piece).to_be_bytes());
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
                encode_trade_intent(payload, SELL_ITEM_INTENT, npc, bag_slot, quantity);
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
                payload.push(quest);
                continue;
            }
            PlayerIntent::CompleteQuest { npc, quest, choice } => {
                payload.push(COMPLETE_QUEST_INTENT);
                payload.extend_from_slice(&npc.get().to_be_bytes());
                payload.extend_from_slice(&[quest, choice]);
                continue;
            }
            PlayerIntent::AbandonQuest { quest } => {
                payload.extend_from_slice(&[ABANDON_QUEST_INTENT, quest]);
                continue;
            }
        };
        payload.push(code);
        encode_entity_ref(payload, target);
    }
    payload.push(u8::from(combat.intents_dropped));
    payload.push(encode_u8_count(
        combat.events.len(),
        MAX_EVENTS_PER_PLAYER,
        "player has too many events",
    )?);
    for event in &combat.events {
        encode_event(payload, event);
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
    ] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.push(encode_u8_count(
        abilities.cooldowns.len(),
        MAX_COOLDOWNS,
        "player has too many cooldowns",
    )?);
    for cooldown in &abilities.cooldowns {
        payload.push(cooldown.ability.get());
        payload.extend_from_slice(&cooldown.remaining.to_be_bytes());
    }
    encode_cast(payload, abilities.cast);
    encode_auras(payload, &abilities.auras)?;
    payload.push(crate::wire::encode_lane(abilities.guard.raised));
    payload.extend_from_slice(&abilities.guard.broken.to_be_bytes());
    let landing = combat.landing;
    payload.push(u8::from(landing.airborne));
    payload.extend_from_slice(&landing.recovery.to_be_bytes());
    payload.push(u8::from(landing.attacked_airborne));
    payload.extend_from_slice(&combat.strike_recovery.to_be_bytes());
    payload.extend_from_slice(&crate::wire::encode_piece(combat.repairing).to_be_bytes());
    encode_entity_ref(payload, combat.combo.map(|combo| combo.target));
    payload.extend_from_slice(&combat.combo.map_or(0, |combo| combo.elapsed).to_be_bytes());
    payload.extend_from_slice(&player.chat_ready_at.to_be_bytes());
    payload.push(encode_u8_count(
        player.chat.len(),
        MAX_CHAT_PER_TICK,
        "player heard too many chat lines",
    )?);
    for line in &player.chat {
        crate::chat::encode_line(payload, line);
    }
    crate::quest::encode_log(payload, &player.quests, player.quests_changed_at)
}

/// 17 bytes: ability (0 for none), elapsed ticks, target reference, point
/// present and the point (`2 × i32`, zero when absent).
fn encode_cast(payload: &mut Vec<u8>, cast: Option<CastState>) {
    let Some(cast) = cast else {
        payload.extend_from_slice(&[0; 17]);
        return;
    };
    payload.push(cast.ability.get());
    payload.extend_from_slice(&cast.elapsed.to_be_bytes());
    encode_entity_ref(payload, cast.target);
    payload.push(u8::from(cast.point.is_some()));
    for component in cast.point.unwrap_or([0; 2]) {
        payload.extend_from_slice(&component.to_be_bytes());
    }
}

fn decode_cast(payload: &[u8], offset: &mut usize) -> Result<Option<CastState>, ProtocolError> {
    let ability = read_u8(payload, offset)?;
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

/// A count, then 10 bytes per aura: ability, caster reference, remaining
/// ticks and amount.
fn encode_auras(payload: &mut Vec<u8>, auras: &[Aura]) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        auras.len(),
        MAX_AURAS,
        "unit has too many auras",
    )?);
    for aura in auras {
        payload.push(aura.ability.get());
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
        let ability = AbilityId::new(read_u8(payload, offset)?);
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

fn encode_creature(
    payload: &mut Vec<u8>,
    creature: &CanonicalCreatureSnapshot,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(&creature.creature_id.get().to_be_bytes());
    payload.extend_from_slice(&creature.generation.to_be_bytes());
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
    };
    payload.push(ai);
    payload.extend_from_slice(&timer.to_be_bytes());
    payload.push(u8::from(destination.is_some()));
    for component in destination.unwrap_or([0; 2]) {
        payload.extend_from_slice(&component.to_be_bytes());
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
    payload.extend_from_slice(&creature.combat_timer.to_be_bytes());
    payload.push(u8::from(creature.tapped_by.is_some()));
    payload.extend_from_slice(&creature.tapped_by.unwrap_or(0).to_be_bytes());
    payload.push(u8::from(creature.loot.is_some()));
    if let Some(rewards) = creature.loot {
        crate::loot::encode_rewards(payload, rewards);
    }
    let abilities = &creature.abilities;
    payload.extend_from_slice(&abilities.ability_timer.to_be_bytes());
    encode_cast(payload, abilities.cast);
    encode_auras(payload, &abilities.auras)
}

pub fn decode_canonical_snapshot(payload: &[u8]) -> Result<CanonicalZoneSnapshot, ProtocolError> {
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, CANONICAL_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let content_fingerprint = u64::from_be_bytes(take(payload, &mut offset)?);
    let rng_state = u64::from_be_bytes(take(payload, &mut offset)?);
    let loot_rng_state = u64::from_be_bytes(take(payload, &mut offset)?);
    let player_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_PLAYERS_PER_ZONE,
        "snapshot exceeds configured zone player capacity",
    )?;
    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        players.push(decode_player(payload, &mut offset)?);
    }
    let creature_count = decode_u16_count(
        payload,
        &mut offset,
        MAX_CREATURE_SPAWNS,
        "snapshot exceeds the creature spawn capacity",
    )?;
    let mut creatures = Vec::with_capacity(creature_count);
    for _ in 0..creature_count {
        creatures.push(decode_creature(payload, &mut offset)?);
    }
    let doors = decode_canonical_doors(payload, &mut offset)?;
    let (next_projectile, projectiles) = decode_projectiles(payload, &mut offset)?;
    let forced = decode_forced(payload, &mut offset)?;
    let (next_debris, debris) = decode_debris(payload, &mut offset)?;
    let pieces = decode_pieces(payload, &mut offset)?;
    let rewards = crate::rewards::decode_history(payload, &mut offset)?;
    ensure_fully_consumed(payload, offset)?;
    Ok(CanonicalZoneSnapshot {
        rewards,
        schema_version,
        zone_id,
        tick,
        content_revision,
        content_fingerprint,
        rng_state,
        loot_rng_state,
        players,
        creatures,
        doors,
        projectiles,
        next_projectile,
        forced,
        debris,
        next_debris,
        pieces,
    })
}

/// Destructible pieces (#305): count (`u8`), then per piece its ID (`u16`), health (`u32`),
/// last change tick (`u64`) and repaired regrowth ticks (`u32`), in content order.
fn encode_pieces(
    payload: &mut Vec<u8>,
    pieces: &[mmorpg_core::CanonicalPiece],
) -> Result<(), ProtocolError> {
    payload.push(
        u8::try_from(pieces.len())
            .ok()
            .filter(|&count| usize::from(count) <= mmorpg_core::MAX_PIECES)
            .ok_or_else(|| ProtocolError::new("too many structure pieces"))?,
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
    let count = usize::from(read_u8(payload, offset)?);
    if count > mmorpg_core::MAX_PIECES {
        return Err(ProtocolError::new("too many structure pieces"));
    }
    (0..count)
        .map(|_| {
            Ok(mmorpg_core::CanonicalPiece {
                piece: mmorpg_core::PieceId::new(u16::from_be_bytes(take(payload, offset)?)),
                health: u32::from_be_bytes(take(payload, offset)?),
                changed_tick: u64::from_be_bytes(take(payload, offset)?),
                repaired: u32::from_be_bytes(take(payload, offset)?),
            })
        })
        .collect()
}

fn decode_player(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalPlayerSnapshot, ProtocolError> {
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
    let last_sequence = u32::from_be_bytes(take(payload, offset)?);
    let spawn_slot = u16::from_be_bytes(take(payload, offset)?);
    let inventory_revision = u64::from_be_bytes(take(payload, offset)?);
    let inventory_changed_at = u64::from_be_bytes(take(payload, offset)?);
    let inventory = decode_inventory(payload, offset)?;
    let equipment = decode_equipment(payload, offset)?;
    let copper = u32::from_be_bytes(take(payload, offset)?);
    let level = read_u8(payload, offset)?;
    let experience = u32::from_be_bytes(take(payload, offset)?);
    let health = u32::from_be_bytes(take(payload, offset)?);
    let target = decode_entity_ref(payload, offset)?;
    let auto_attack = read_bool(payload, offset)?;
    let swing_timer = u16::from_be_bytes(take(payload, offset)?);
    let combat_timer = u16::from_be_bytes(take(payload, offset)?);
    let calm_ticks = u16::from_be_bytes(take(payload, offset)?);
    let error_cooldown = u16::from_be_bytes(take(payload, offset)?);
    let intent_count = decode_u8_count(
        payload,
        offset,
        MAX_PENDING_INTENTS,
        "player has too many pending intents",
    )?;
    let mut intents = Vec::with_capacity(intent_count);
    for _ in 0..intent_count {
        let code = read_u8(payload, offset)?;
        if code == LOOT_INTENT {
            intents.push(PlayerIntent::Loot(crate::loot::decode_claim(
                payload, offset,
            )?));
            continue;
        }
        if code == USE_ABILITY_INTENT {
            let ability = read_u8(payload, offset)?;
            intents.push(PlayerIntent::UseAbility {
                ability,
                target: decode_entity_ref(payload, offset)?,
            });
            continue;
        }
        if code == DIRECTED_STRIKE_INTENT {
            let [ability, high, low, side, height] = take(payload, offset)?;
            intents.push(PlayerIntent::DirectedStrike {
                ability,
                facing: u16::from_be_bytes([high, low]),
                aim: crate::wire::decode_aim([side, height])?,
            });
            continue;
        }
        if code == GUARD_INTENT {
            intents.push(PlayerIntent::Guard(crate::wire::decode_lane(read_u8(
                payload, offset,
            )?)?));
            continue;
        }
        if code == USE_DOOR_INTENT {
            let [door, open] = take(payload, offset)?;
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
        if code == REPAIR_INTENT {
            let piece = u16::from_be_bytes(take(payload, offset)?);
            intents.push(PlayerIntent::Repair(crate::wire::decode_piece(piece)));
            continue;
        }
        if code == CHOOSE_CLASS_INTENT {
            let [class, sex] = take(payload, offset)?;
            intents.push(PlayerIntent::ChooseClass { class, sex });
            continue;
        }
        if code == ACCEPT_QUEST_INTENT || code == COMPLETE_QUEST_INTENT {
            let npc = mmorpg_core::NpcId::new(u32::from_be_bytes(take(payload, offset)?));
            let quest = read_u8(payload, offset)?;
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
                quest: read_u8(payload, offset)?,
            });
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
            let [index, high, low] = take(payload, offset)?;
            let quantity = u16::from_be_bytes([high, low]);
            intents.push(if code == BUY_ITEM_INTENT {
                PlayerIntent::BuyItem {
                    npc,
                    offer: index,
                    quantity,
                }
            } else {
                PlayerIntent::SellItem {
                    npc,
                    bag_slot: index,
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
            (START_ATTACK_INTENT, None) => PlayerIntent::StartAttack,
            (STOP_ATTACK_INTENT, None) => PlayerIntent::StopAttack,
            (RELEASE_SPIRIT_INTENT, None) => PlayerIntent::ReleaseSpirit,
            (CANCEL_CAST_INTENT, None) => PlayerIntent::CancelCast,
            _ => return Err(ProtocolError::new("malformed pending intent")),
        });
    }
    let intents_dropped = read_bool(payload, offset)?;
    let event_count = decode_u8_count(
        payload,
        offset,
        MAX_EVENTS_PER_PLAYER,
        "player has too many events",
    )?;
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        events.push(decode_event(payload, offset)?);
    }
    let class_code = read_u8(payload, offset)?;
    let class = ClassChoice::from_appearance(u16::from(class_code));
    if class_code != 0 && class.is_none() {
        return Err(ProtocolError::new("unknown class choice"));
    }
    let resource = u16::from_be_bytes(take(payload, offset)?);
    let resource_ticks = u16::from_be_bytes(take(payload, offset)?);
    let mana_delay = u16::from_be_bytes(take(payload, offset)?);
    let global_cooldown = u16::from_be_bytes(take(payload, offset)?);
    let cooldown_count = decode_u8_count(
        payload,
        offset,
        MAX_COOLDOWNS,
        "player has too many cooldowns",
    )?;
    let mut cooldowns = Vec::with_capacity(cooldown_count);
    for _ in 0..cooldown_count {
        let ability = AbilityId::new(read_u8(payload, offset)?);
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
        cooldowns,
        cast: decode_cast(payload, offset)?,
        auras: decode_auras(payload, offset)?,
        guard: mmorpg_core::GuardState {
            raised: crate::wire::decode_lane(read_u8(payload, offset)?)?,
            broken: u16::from_be_bytes(take(payload, offset)?),
        },
    };
    let landing = mmorpg_core::LandingState {
        airborne: read_bool(payload, offset)?,
        recovery: u16::from_be_bytes(take(payload, offset)?),
        attacked_airborne: read_bool(payload, offset)?,
    };
    let strike_recovery = u16::from_be_bytes(take(payload, offset)?);
    let repairing = crate::wire::decode_piece(u16::from_be_bytes(take(payload, offset)?));
    let combo_target = decode_entity_ref(payload, offset)?;
    let combo_elapsed = u16::from_be_bytes(take(payload, offset)?);
    let combo = if let Some(target) = combo_target {
        Some(mmorpg_core::ComboOpening {
            target,
            elapsed: combo_elapsed,
        })
    } else if combo_elapsed != 0 {
        return Err(ProtocolError::new("absent combo has nonzero fields"));
    } else {
        None
    };
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
    let (quests, quests_changed_at) = crate::quest::decode_log(payload, offset)?;
    Ok(CanonicalPlayerSnapshot {
        quests,
        quests_changed_at,
        chat_ready_at,
        chat,
        player_id,
        position,
        velocity,
        facing,
        forward,
        strafe,
        jump_pending,
        last_sequence,
        spawn_slot,
        inventory,
        copper,
        inventory_revision,
        inventory_changed_at,
        equipment,
        combat: CanonicalPlayerCombat {
            level,
            experience,
            health,
            target,
            auto_attack,
            swing_timer,
            combat_timer,
            calm_ticks,
            error_cooldown,
            intents,
            intents_dropped,
            events,
            abilities,
            landing,
            combo,
            strike_recovery,
            repairing,
        },
    })
}

fn decode_creature(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalCreatureSnapshot, ProtocolError> {
    let creature_id = CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
    let generation = u64::from_be_bytes(take(payload, offset)?);
    if generation == 0 {
        return Err(ProtocolError::new("invalid creature life identity"));
    }
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
    let combat_timer = u16::from_be_bytes(take(payload, offset)?);
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
    let abilities = CanonicalCreatureAbilities {
        ability_timer: u16::from_be_bytes(take(payload, offset)?),
        cast: decode_cast(payload, offset)?,
        auras: decode_auras(payload, offset)?,
    };
    Ok(CanonicalCreatureSnapshot {
        generation,
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
        combat_timer,
        tapped_by,
        loot,
        abilities,
    })
}

/// A pending trade: code, NPC (u32), offer or bag slot (u8), quantity (u16).
fn encode_trade_intent(
    payload: &mut Vec<u8>,
    code: u8,
    npc: mmorpg_core::NpcId,
    index: u8,
    quantity: u16,
) {
    payload.push(code);
    payload.extend_from_slice(&npc.get().to_be_bytes());
    payload.push(index);
    payload.extend_from_slice(&quantity.to_be_bytes());
}

const TRACKING: u8 = 1;
const FREE: u8 = 2;
const STUCK: u8 = 3;

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
        payload.push(projectile.ability.get());
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
        let ability = mmorpg_core::AbilityId::new(crate::wire::read_u8(payload, offset)?);
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
                resting,
            },
            age,
        });
    }
    Ok((next, debris))
}

/// At most one forced-movement state per unit.
const MAX_FORCED: usize = MAX_PLAYERS_PER_ZONE + MAX_CREATURE_SPAWNS;

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
            Some(mmorpg_core::ForcedMove {
                velocity,
                remaining,
                charge,
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
mod tests {
    use super::*;
    use crate::decode_snapshot;
    use mmorpg_core::{
        ErrorCode, NpcId, SNAPSHOT_SCHEMA_VERSION, ZoneDefinition, ZoneEvent, ZoneId,
        ZoneSimulation,
    };

    fn snapshot() -> CanonicalZoneSnapshot {
        let wolf = EntityRef::Creature(CreatureId::new(4));
        CanonicalZoneSnapshot {
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
                    resting: false,
                },
                age: 9,
            }],
            next_debris: 8,
            forced: vec![
                (
                    EntityRef::Player(1),
                    mmorpg_core::ForcedState {
                        moving: Some(mmorpg_core::ForcedMove {
                            velocity: [0, 45],
                            remaining: 12,
                            charge: Some((wolf, 300)),
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
            ],
            loot_rng_state: 0,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            content_revision: 3,
            content_fingerprint: 0x0123_4567_89ab_cdef,
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
                    last_sequence: 81,
                    spawn_slot: 3,
                    inventory: {
                        let mut bag = mmorpg_core::Inventory::default();
                        bag.insert(mmorpg_core::ItemId::new(1), 27).unwrap();
                        bag.insert(mmorpg_core::ItemId::new(2), 2).unwrap();
                        bag
                    },
                    inventory_revision: 4,
                    inventory_changed_at: 88,
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
                        auto_attack: true,
                        swing_timer: 12,
                        combat_timer: 149,
                        calm_ticks: 0,
                        error_cooldown: 30,
                        intents: vec![
                            PlayerIntent::SelectTarget(Some(EntityRef::Npc(NpcId::new(2)))),
                            PlayerIntent::SelectTarget(None),
                            PlayerIntent::StartAttack,
                            PlayerIntent::StopAttack,
                            PlayerIntent::ReleaseSpirit,
                            PlayerIntent::MoveItem {
                                source: 1,
                                destination: 15,
                                quantity: 4,
                            },
                        ],
                        // The decoder checks structure; core checks that only a full queue drops.
                        intents_dropped: true,
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
                        combo: Some(mmorpg_core::ComboOpening {
                            target: EntityRef::Creature(CreatureId::new(1)),
                            elapsed: 3,
                        }),
                        strike_recovery: 4,
                        repairing: Some(mmorpg_core::PieceId::new(3)),
                    },
                    quests: mmorpg_core::QuestLog::default(),
                    quests_changed_at: 0,
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
                    last_sequence: 0,
                    spawn_slot: 0,
                    inventory: mmorpg_core::Inventory::default(),
                    inventory_revision: 1,
                    inventory_changed_at: 0,
                    equipment: mmorpg_core::Equipment::default(),
                    combat: CanonicalPlayerCombat::default(),
                    quests: mmorpg_core::QuestLog::default(),
                    quests_changed_at: 0,
                },
            ],
            creatures: vec![
                CanonicalCreatureSnapshot {
                    generation: 1,
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
                    combat_timer: 150,
                    tapped_by: Some(7),
                    abilities: CanonicalCreatureAbilities::default(),
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
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
                    combat_timer: 0,
                    tapped_by: Some(8),
                    abilities: CanonicalCreatureAbilities::default(),
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
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
                    combat_timer: 0,
                    tapped_by: None,
                    abilities: CanonicalCreatureAbilities::default(),
                },
                CanonicalCreatureSnapshot {
                    generation: 1,
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
                    combat_timer: 0,
                    tapped_by: None,
                    abilities: CanonicalCreatureAbilities::default(),
                },
            ],
            pieces: vec![mmorpg_core::CanonicalPiece {
                piece: mmorpg_core::PieceId::new(3),
                health: 40,
                changed_tick: 77,
                repaired: 8,
            }],
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
            .push(PlayerIntent::Loot(mmorpg_core::LootClaim {
                creature: CreatureId::new(5),
                died_at: 90,
            }));
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
        state.players[0].combat.intents = vec![
            PlayerIntent::UseAbility {
                ability: 9,
                target: Some(wolf),
            },
            PlayerIntent::UseAbility {
                ability: u8::MAX,
                target: None,
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
        ];
        state.players[0].combat.abilities = CanonicalPlayerAbilities {
            class: Some(ClassChoice {
                class: mmorpg_core::PlayerClass::Arcanist,
                sex: mmorpg_core::Sex::Male,
            }),
            resource: 132,
            resource_ticks: 29,
            mana_delay: 150,
            global_cooldown: 44,
            guard: mmorpg_core::GuardState {
                raised: Some(mmorpg_core::StrikeHeight::Low),
                broken: 7,
            },
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
        // occupy 7 + 7 + 6 + 6 + 3 + 2 + 2 bytes, then the dropped flag and two events.
        let intents = 16 + 32 + 2 + 158;
        let player_abilities = intents + 1 + (7 + 7 + 6 + 6 + 3 + 2 + 2) + 1 + 1 + 2 * EVENT_BYTES;
        assert_eq!(encoded[player_abilities], 6, "Arcanist, male");
        let cast = player_abilities + 1 + 8 + 1 + 2 * 3;
        let aura = cast + 17 + 1;
        for (offset, value, message) in [
            (player_abilities, 7, "unknown class choice"),
            // The directed strike's side code.
            (intents + 1 + 7 + 7 + 4, 3, "unknown strike aim"),
            (cast + 8, 2, "boolean field must be 0 or 1"),
            (aura + 1, 0, "an absent entity must have ID 0"),
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
        stray[cast + 2] = 1;
        assert_eq!(
            decode_canonical_snapshot(&stray).unwrap_err().to_string(),
            "malformed cast state"
        );
    }

    const EVENT_BYTES: usize = 14;

    #[test]
    fn canonical_snapshot_round_trip_preserves_continuation_state() {
        let snapshot = snapshot();
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        // A canonical snapshot is never a player projection.
        assert!(decode_snapshot(&encoded).is_err());
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_canonical_snapshot(&trailing).is_err());
        // Header 16, identity 24, then the first player record.
        let jump_flag = 16 + 32 + 2 + 32;
        assert_eq!(encoded[jump_flag], 1);
        for (offset, value, message) in [
            (jump_flag, 2, "jump flag must be 0 or 1"),
            (0, 4, "unsupported snapshot wire version"),
            (3, 4, "unsupported core snapshot schema version"),
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
        snapshot.players[0].combat.intents = vec![
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
        ];
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        let buy = [BUY_ITEM_INTENT, 1, 2, 3, 4, 6, 0x0a, 0x0b];
        let sell = [SELL_ITEM_INTENT, 0, 0, 0, 3, 15, 0xff, 0xff];
        let records: Vec<u8> = [buy, sell].concat();
        assert!(
            encoded
                .windows(records.len())
                .any(|window| window == records.as_slice()),
            "pending trades are 8-byte records"
        );
        for length in 0..encoded.len() {
            assert!(decode_canonical_snapshot(&encoded[..length]).is_err());
        }
    }

    #[test]
    fn quest_logs_and_pending_quest_intents_round_trip_with_exact_records() {
        let mut snapshot = snapshot();
        let npc = mmorpg_core::NpcId::new(0x0102_0304);
        snapshot.players[0].combat.intents = vec![
            PlayerIntent::AcceptQuest { npc, quest: 2 },
            PlayerIntent::CompleteQuest {
                npc: mmorpg_core::NpcId::new(1),
                quest: 9,
                choice: u8::MAX,
            },
            PlayerIntent::AbandonQuest { quest: 0 },
        ];
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
            completed: 0b11,
        };
        snapshot.players[0].quests_changed_at = 0x0a0b;
        let encoded = encode_canonical_snapshot(&snapshot).unwrap();
        assert_eq!(decode_canonical_snapshot(&encoded).unwrap(), snapshot);
        let intents = [
            &[ACCEPT_QUEST_INTENT, 1, 2, 3, 4, 2][..],
            &[COMPLETE_QUEST_INTENT, 0, 0, 0, 1, 9, 0xff],
            &[ABANDON_QUEST_INTENT, 0],
        ]
        .concat();
        let log = [
            &0x0a0b_u64.to_be_bytes()[..],
            &[0, 0, 0, 0b11, 2, 3, 1, 0, 0, 32, 0xff, 2, 7],
        ]
        .concat();
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
            .windows(13)
            .position(|window| window == [0, 0, 0, 0b11, 2, 3, 1, 0, 0, 32, 0xff, 2, 7])
            .unwrap();
        for (offset, value, message) in [
            (at + 4, 11, "quest log exceeds its capacity"),
            (at + 5, 0, "quest id is out of range"),
            (at + 9, 33, "quest id is out of range"),
            (at + 9, 3, "quest log entries are not ordered"),
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
        // Player 7's auto-attack flag follows its target reference.
        let player = 16 + 32 + 2;
        let auto_attack = player + 39 + 96 + 1 + 4 + 4 + 5;
        let intents = auto_attack + 1 + 8;
        let mut cases = vec![
            (auto_attack, 2, "boolean field must be 0 or 1"),
            (intents, 17, "player has too many pending intents"),
            (intents + 1, 22, "malformed pending intent"),
        ];
        // The third intent (StartAttack) carries no target.
        cases.push((intents + 1 + 2 * 6 + 1, 2, "malformed pending intent"));
        // The dropped-intent flag follows the six intents.
        cases.push((intents + 1 + 6 * 6, 2, "boolean field must be 0 or 1"));
        cases.push((
            intents + 1 + 5 * 6 + 5,
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
        let life = creature + 4 + 8 + 1 + 4 + 2 + 24;
        let ai = life + 9;
        for (offset, value, message) in [
            (life, 4, "malformed creature life cycle"),
            (life + 8, 1, "malformed creature life cycle"),
            (ai, 9, "malformed creature AI state"),
            (ai + 3, 2, "boolean field must be 0 or 1"),
            (ai + 4, 1, "malformed creature destination"),
            (ai + 12, 65, "creature threat table exceeds its capacity"),
            (ai + 13, 0, "an absent entity must have ID 0"),
            (ai + 12 + 1 + 9 + 4, 2, "boolean field must be 0 or 1"),
            (ai + 12 + 1 + 9 + 4, 0, "malformed creature tap"),
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
        // Player 7's equipment follows its 80-byte inventory block.
        let equipment = 16 + 32 + 2 + 39 + 80;
        assert_eq!(encoded[equipment..equipment + 2], [0, 3]);
        for (slot, item, message) in [
            (0, 11, "unknown item"),
            (1, 2, "item cannot be equipped there"),
            (2, 1, "item cannot be equipped there"),
        ] {
            let mut invalid = encoded.clone();
            invalid[equipment + 2 * slot + 1] = item;
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
            ZoneDefinition::new(9, [0, -1, 0], Vec::new()).unwrap(),
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
            state.players[0].combat.combo = Some(mmorpg_core::ComboOpening { target, elapsed: 3 });
            let decoded =
                decode_canonical_snapshot(&encode_canonical_snapshot(&state).unwrap()).unwrap();
            assert_eq!(
                decoded.players[0].combat.combo,
                state.players[0].combat.combo
            );
        }
    }
}
