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
    encode_u8_count, encode_u16_count, ensure_fully_consumed, read_bool, read_u8, read_vector,
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
            PlayerIntent::CancelCast => (CANCEL_CAST_INTENT, None),
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
    ensure_fully_consumed(payload, offset)?;
    Ok(CanonicalZoneSnapshot {
        schema_version,
        zone_id,
        tick,
        content_revision,
        content_fingerprint,
        rng_state,
        loot_rng_state,
        players,
        creatures,
    })
}

fn decode_player(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalPlayerSnapshot, ProtocolError> {
    let player_id = u32::from_be_bytes(take(payload, offset)?);
    let position = read_vector(payload, offset)?;
    let velocity = read_vector(payload, offset)?;
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
        },
    })
}

fn decode_creature(
    payload: &[u8],
    offset: &mut usize,
) -> Result<CanonicalCreatureSnapshot, ProtocolError> {
    let creature_id = CreatureId::new(u32::from_be_bytes(take(payload, offset)?));
    let level = read_u8(payload, offset)?;
    let health = u32::from_be_bytes(take(payload, offset)?);
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let position = read_vector(payload, offset)?;
    let velocity = read_vector(payload, offset)?;
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
            loot_rng_state: 0,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            content_revision: 3,
            content_fingerprint: 0x0123_4567_89ab_cdef,
            rng_state: u64::MAX,
            players: vec![
                CanonicalPlayerSnapshot {
                    chat_ready_at: 0,
                    chat: Vec::new(),
                    copper: 0,
                    player_id: 7,
                    position: [10, 20, -30],
                    velocity: [1, -2, 70_000],
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
                    },
                    quests: mmorpg_core::QuestLog::default(),
                    quests_changed_at: 0,
                },
                CanonicalPlayerSnapshot {
                    chat_ready_at: 0,
                    chat: Vec::new(),
                    copper: 0,
                    player_id: 8,
                    position: [0; 3],
                    velocity: [0; 3],
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
                    loot: None,
                    creature_id: CreatureId::new(4),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [100, 45, 200],
                    velocity: [-19, -1, 0],
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
                    loot: None,
                    creature_id: CreatureId::new(5),
                    level: 1,
                    health: 0,
                    facing: 3,
                    position: [300, 45, 200],
                    velocity: [0; 3],
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
                    loot: None,
                    creature_id: CreatureId::new(6),
                    level: 1,
                    health: 30,
                    facing: 3,
                    position: [500, 45, 200],
                    velocity: [8, 0, 0],
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
                    loot: None,
                    creature_id: CreatureId::new(7),
                    level: 1,
                    health: 12,
                    facing: 3,
                    position: [700, 45, 200],
                    velocity: [0, 0, 24],
                    life: CreatureLife::Alive,
                    ai: CreatureAi::Evading { ticks: 44 },
                    threat: Vec::new(),
                    swing_timer: 0,
                    combat_timer: 0,
                    tapped_by: None,
                    abilities: CanonicalCreatureAbilities::default(),
                },
            ],
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
        // The intent count follows the player's timers; the six intents
        // occupy 7 + 7 + 6 + 3 + 2 + 2 bytes, then the dropped flag and two events.
        let intents = 16 + 32 + 2 + 158;
        let player_abilities = intents + 1 + (7 + 7 + 6 + 3 + 2 + 2) + 1 + 1 + 2 * EVENT_BYTES;
        assert_eq!(encoded[player_abilities], 6, "Arcanist, male");
        let cast = player_abilities + 1 + 8 + 1 + 2 * 3;
        let aura = cast + 17 + 1;
        for (offset, value, message) in [
            (player_abilities, 7, "unknown class choice"),
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
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "unexpected snapshot scope"
        );
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
            (intents + 1, 18, "malformed pending intent"),
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
        let life = creature + 4 + 1 + 4 + 2 + 24;
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
}
