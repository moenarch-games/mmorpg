//! Zone command datagrams: the protocol byte, a tag and fixed per-tag fields
//! (docs/PROTOCOL.md). Decoding is strict.

use mmorpg_core::{NpcId, ZoneCommand};

use crate::ProtocolError;
use crate::wire::{decode_entity_ref, encode_entity_ref};

/// Every command leads with the low byte of [`crate::PROTOCOL_ID`].
const PROTOCOL_BYTE: u8 = crate::PROTOCOL_ID.to_be_bytes()[3];

const MOVE_TAG: u8 = 1;
const JUMP_TAG: u8 = 2;
const SELECT_TARGET_TAG: u8 = 3;
/// A weapon swing toward a facing (#517); tag 5 (the retired stop-attack) is unknown.
const SWING_TAG: u8 = 4;
const RELEASE_SPIRIT_TAG: u8 = 6;
const MOVE_ITEM_TAG: u8 = 7;
const LOOT_TAG: u8 = 8;
const USE_ABILITY_TAG: u8 = 9;
const CANCEL_CAST_TAG: u8 = 10;
const CHOOSE_CLASS_TAG: u8 = 11;
const EQUIP_ITEM_TAG: u8 = 12;
const UNEQUIP_ITEM_TAG: u8 = 13;
const BUY_ITEM_TAG: u8 = 14;
const SELL_ITEM_TAG: u8 = 15;
const CHAT_TAG: u8 = 16;
const EMOTE_TAG: u8 = 17;
const ACCEPT_QUEST_TAG: u8 = 18;
const COMPLETE_QUEST_TAG: u8 = 19;
const ABANDON_QUEST_TAG: u8 = 20;
const DIRECTED_STRIKE_TAG: u8 = 21;
const GUARD_TAG: u8 = 22;
const USE_DOOR_TAG: u8 = 23;
const REPAIR_TAG: u8 = 24;
const RISE_TAG: u8 = 25;
const COMMAND_MINIONS_TAG: u8 = 26;
/// Wraps another command with the zone tick its client had observed (#415).
const OBSERVED_TAG: u8 = 27;
/// Releases a held ability input (#414).
const RELEASE_ABILITY_TAG: u8 = 28;
/// A dodge roll (#516).
const ROLL_TAG: u8 = 29;
/// Version, tag, ability (u8), retired (0 key up, 1 retired).
const RELEASE_ABILITY_COMMAND_BYTES: usize = 4;
/// Version, tag, observed tick (u64), before the inner command's tag.
const OBSERVED_PREFIX_BYTES: usize = 10;
/// Version, tag, minion order (9 bytes, #295).
const COMMAND_MINIONS_COMMAND_BYTES: usize = 2 + crate::wire::MINION_ORDER_BYTES;
/// Version, tag, rise (i8: -1, 0 or 1).
const RISE_COMMAND_BYTES: usize = 3;
/// Version, tag, piece reference (u16: 0 none, else the piece ID plus one).
const REPAIR_COMMAND_BYTES: usize = 4;
/// Version, tag, lane code (0 lowered, 1 + the height code raised).
const GUARD_COMMAND_BYTES: usize = 3;
const USE_DOOR_COMMAND_BYTES: usize = 4;
/// Version, tag, ability (u8), facing (u16), side and height codes (u8 each).
const DIRECTED_STRIKE_COMMAND_BYTES: usize = 7;
/// Version, tag, emote ID (u8).
const EMOTE_COMMAND_BYTES: usize = 3;
/// Version, tag, NPC (u32), offer or bag slot (u8), quantity (u16).
const TRADE_COMMAND_BYTES: usize = 9;
const USE_ABILITY_COMMAND_BYTES: usize = 8;
const CHOOSE_CLASS_COMMAND_BYTES: usize = 4;
const MOVE_COMMAND_BYTES: usize = 6;
const SELECT_TARGET_COMMAND_BYTES: usize = 7;
const BARE_COMMAND_BYTES: usize = 2;

#[must_use]
pub fn encode_command(command: ZoneCommand) -> Vec<u8> {
    match command {
        ZoneCommand::Move {
            forward,
            strafe,
            facing,
        } => {
            let mut payload = Vec::with_capacity(MOVE_COMMAND_BYTES);
            payload.extend_from_slice(&[PROTOCOL_BYTE, MOVE_TAG]);
            payload.extend_from_slice(&forward.to_be_bytes());
            payload.extend_from_slice(&strafe.to_be_bytes());
            payload.extend_from_slice(&facing.to_be_bytes());
            payload
        }
        ZoneCommand::Loot(claim) => {
            let mut payload = vec![PROTOCOL_BYTE, LOOT_TAG];
            crate::loot::encode_claim(&mut payload, claim);
            payload
        }
        ZoneCommand::Jump => vec![PROTOCOL_BYTE, JUMP_TAG],
        ZoneCommand::Roll => vec![PROTOCOL_BYTE, ROLL_TAG],
        ZoneCommand::Rise(rise) => vec![PROTOCOL_BYTE, RISE_TAG, rise.to_be_bytes()[0]],
        ZoneCommand::UseAbility { ability, target } => {
            let mut payload = Vec::with_capacity(USE_ABILITY_COMMAND_BYTES);
            payload.extend_from_slice(&[PROTOCOL_BYTE, USE_ABILITY_TAG, ability]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::ReleaseAbility { ability, retired } => vec![
            PROTOCOL_BYTE,
            RELEASE_ABILITY_TAG,
            ability,
            u8::from(retired),
        ],
        ZoneCommand::DirectedStrike {
            ability,
            facing,
            aim,
        } => {
            let mut payload = Vec::with_capacity(DIRECTED_STRIKE_COMMAND_BYTES);
            payload.extend_from_slice(&[PROTOCOL_BYTE, DIRECTED_STRIKE_TAG, ability]);
            payload.extend_from_slice(&facing.to_be_bytes());
            payload.extend_from_slice(&crate::wire::encode_aim(aim));
            payload
        }
        ZoneCommand::Guard { lane } => {
            vec![PROTOCOL_BYTE, GUARD_TAG, crate::wire::encode_lane(lane)]
        }
        ZoneCommand::UseDoor { door, open } => {
            vec![PROTOCOL_BYTE, USE_DOOR_TAG, door.get(), u8::from(open)]
        }
        ZoneCommand::Repair { piece } => {
            let [high, low] = crate::wire::encode_piece(piece).to_be_bytes();
            vec![PROTOCOL_BYTE, REPAIR_TAG, high, low]
        }
        ZoneCommand::CancelCast => vec![PROTOCOL_BYTE, CANCEL_CAST_TAG],
        ZoneCommand::CommandMinions(order) => {
            let mut payload = Vec::with_capacity(COMMAND_MINIONS_COMMAND_BYTES);
            payload.extend_from_slice(&[PROTOCOL_BYTE, COMMAND_MINIONS_TAG]);
            crate::wire::encode_minion_order(&mut payload, order);
            payload
        }
        ZoneCommand::ChooseClass { class, sex } => {
            vec![PROTOCOL_BYTE, CHOOSE_CLASS_TAG, class, sex]
        }
        ZoneCommand::EquipItem { bag_slot } => {
            vec![PROTOCOL_BYTE, EQUIP_ITEM_TAG, bag_slot]
        }
        ZoneCommand::UnequipItem { equipment_slot } => {
            vec![PROTOCOL_BYTE, UNEQUIP_ITEM_TAG, equipment_slot]
        }
        ZoneCommand::SelectTarget(target) => {
            let mut payload = Vec::with_capacity(SELECT_TARGET_COMMAND_BYTES);
            payload.extend_from_slice(&[PROTOCOL_BYTE, SELECT_TARGET_TAG]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::Swing { facing } => {
            let [high, low] = facing.to_be_bytes();
            vec![PROTOCOL_BYTE, SWING_TAG, high, low]
        }
        ZoneCommand::ReleaseSpirit => vec![PROTOCOL_BYTE, RELEASE_SPIRIT_TAG],
        ZoneCommand::BuyItem {
            npc,
            offer,
            quantity,
        } => encode_trade(BUY_ITEM_TAG, npc, offer, quantity),
        ZoneCommand::SellItem {
            npc,
            bag_slot,
            quantity,
        } => encode_trade(SELL_ITEM_TAG, npc, bag_slot, quantity),
        ZoneCommand::Emote(emote) => vec![PROTOCOL_BYTE, EMOTE_TAG, emote.code()],
        ZoneCommand::AcceptQuest { npc, quest } => {
            let mut payload = vec![PROTOCOL_BYTE, ACCEPT_QUEST_TAG];
            payload.extend_from_slice(&npc.get().to_be_bytes());
            payload.push(quest);
            payload
        }
        ZoneCommand::CompleteQuest { npc, quest, choice } => {
            let mut payload = vec![PROTOCOL_BYTE, COMPLETE_QUEST_TAG];
            payload.extend_from_slice(&npc.get().to_be_bytes());
            payload.extend_from_slice(&[quest, choice]);
            payload
        }
        ZoneCommand::AbandonQuest { quest } => {
            vec![PROTOCOL_BYTE, ABANDON_QUEST_TAG, quest]
        }
        ZoneCommand::Chat { channel, text } => {
            let mut payload = vec![PROTOCOL_BYTE, CHAT_TAG];
            crate::chat::encode_text(&mut payload, channel, &text);
            payload
        }
        ZoneCommand::MoveItem {
            source,
            destination,
            quantity,
        } => {
            let [high, low] = quantity.to_be_bytes();
            vec![PROTOCOL_BYTE, MOVE_ITEM_TAG, source, destination, high, low]
        }
    }
}

fn encode_trade(tag: u8, npc: NpcId, index: u8, quantity: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(TRADE_COMMAND_BYTES);
    payload.extend_from_slice(&[PROTOCOL_BYTE, tag]);
    payload.extend_from_slice(&npc.get().to_be_bytes());
    payload.push(index);
    payload.extend_from_slice(&quantity.to_be_bytes());
    payload
}

/// The NPC, offer or bag slot, and quantity of a 9-byte trade command.
fn decode_trade(payload: &[u8], name: &str) -> Result<(NpcId, u8, u16), ProtocolError> {
    let [_, _, n0, n1, n2, n3, index, q0, q1] = payload else {
        return Err(ProtocolError::new(format!(
            "{name} command payload must be exactly 9 bytes"
        )));
    };
    Ok((
        NpcId::new(u32::from_be_bytes([*n0, *n1, *n2, *n3])),
        *index,
        u16::from_be_bytes([*q0, *q1]),
    ))
}

/// `command` sent after the client had observed the zone's completed tick
/// `observed` (#415, docs/PROTOCOL.md): version, the observed tag, the tick
/// (u64), then the inner command's tag and fields.
#[must_use]
pub fn encode_observed_command(command: ZoneCommand, observed: u64) -> Vec<u8> {
    let inner = encode_command(command);
    let mut payload = Vec::with_capacity(OBSERVED_PREFIX_BYTES + inner.len() - 1);
    payload.extend_from_slice(&[PROTOCOL_BYTE, OBSERVED_TAG]);
    payload.extend_from_slice(&observed.to_be_bytes());
    payload.extend_from_slice(&inner[1..]);
    payload
}

/// Decodes a plain or an observed command: the command and, for an observed
/// one, the zone tick its client had observed. An observed command cannot
/// wrap another.
pub fn decode_observed_command(
    payload: &[u8],
) -> Result<(ZoneCommand, Option<u64>), ProtocolError> {
    let [version, tag, rest @ ..] = payload else {
        return Err(ProtocolError::new("command payload is truncated"));
    };
    if *tag != OBSERVED_TAG {
        return decode_command(payload).map(|command| (command, None));
    }
    let Some((tick, inner)) = rest.split_first_chunk::<8>() else {
        return Err(ProtocolError::new("observed command payload is truncated"));
    };
    if inner.first() == Some(&OBSERVED_TAG) {
        return Err(ProtocolError::new("an observed command cannot nest"));
    }
    let mut plain = Vec::with_capacity(1 + inner.len());
    plain.push(*version);
    plain.extend_from_slice(inner);
    Ok((decode_command(&plain)?, Some(u64::from_be_bytes(*tick))))
}

pub fn decode_command(payload: &[u8]) -> Result<ZoneCommand, ProtocolError> {
    let [version, tag, ..] = payload else {
        return Err(ProtocolError::new("command payload is truncated"));
    };
    if *version != PROTOCOL_BYTE {
        return Err(ProtocolError::new("unsupported command protocol id"));
    }
    let bare = |command| {
        if payload.len() == BARE_COMMAND_BYTES {
            Ok(command)
        } else {
            Err(ProtocolError::new(
                "command payload must be exactly 2 bytes",
            ))
        }
    };
    match *tag {
        LOOT_TAG => {
            if payload.len() != 14 {
                return Err(ProtocolError::new(
                    "loot command payload must be exactly 14 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES;
            Ok(ZoneCommand::Loot(crate::loot::decode_claim(
                payload,
                &mut offset,
            )?))
        }
        MOVE_ITEM_TAG => {
            let [_, _, source, destination, high, low] = payload else {
                return Err(ProtocolError::new(
                    "move item command payload must be exactly 6 bytes",
                ));
            };
            Ok(ZoneCommand::MoveItem {
                source: *source,
                destination: *destination,
                quantity: u16::from_be_bytes([*high, *low]),
            })
        }
        MOVE_TAG => {
            let [_, _, forward, strafe, facing_high, facing_low] = payload else {
                return Err(ProtocolError::new(
                    "move command payload must be exactly 6 bytes",
                ));
            };
            let forward = i8::from_be_bytes([*forward]);
            let strafe = i8::from_be_bytes([*strafe]);
            if !(-1..=1).contains(&forward) || !(-1..=1).contains(&strafe) {
                return Err(ProtocolError::new(
                    "movement components must be between -1 and 1",
                ));
            }
            Ok(ZoneCommand::Move {
                forward,
                strafe,
                facing: u16::from_be_bytes([*facing_high, *facing_low]),
            })
        }
        SELECT_TARGET_TAG => {
            if payload.len() != SELECT_TARGET_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "select target command payload must be exactly 7 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES;
            Ok(ZoneCommand::SelectTarget(decode_entity_ref(
                payload,
                &mut offset,
            )?))
        }
        USE_ABILITY_TAG => {
            if payload.len() != USE_ABILITY_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "use ability command payload must be exactly 8 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES + 1;
            Ok(ZoneCommand::UseAbility {
                ability: payload[BARE_COMMAND_BYTES],
                target: decode_entity_ref(payload, &mut offset)?,
            })
        }
        RELEASE_ABILITY_TAG => {
            let [_, _, ability, retired] = payload else {
                return Err(ProtocolError::new(
                    "release ability command payload must be exactly 4 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), RELEASE_ABILITY_COMMAND_BYTES);
            let retired = match retired {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::new("release retired flag must be 0 or 1")),
            };
            Ok(ZoneCommand::ReleaseAbility {
                ability: *ability,
                retired,
            })
        }
        DIRECTED_STRIKE_TAG => {
            let [_, _, ability, high, low, side, height] = payload else {
                return Err(ProtocolError::new(
                    "directed strike command payload must be exactly 7 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), DIRECTED_STRIKE_COMMAND_BYTES);
            Ok(ZoneCommand::DirectedStrike {
                ability: *ability,
                facing: u16::from_be_bytes([*high, *low]),
                aim: crate::wire::decode_aim([*side, *height])?,
            })
        }
        RISE_TAG => {
            let [_, _, rise] = payload else {
                return Err(ProtocolError::new(
                    "rise command payload must be exactly 3 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), RISE_COMMAND_BYTES);
            let rise = i8::from_be_bytes([*rise]);
            if !(-1..=1).contains(&rise) {
                return Err(ProtocolError::new("rise must be between -1 and 1"));
            }
            Ok(ZoneCommand::Rise(rise))
        }
        SWING_TAG => {
            let [_, _, high, low] = payload else {
                return Err(ProtocolError::new(
                    "swing command payload must be exactly 4 bytes",
                ));
            };
            Ok(ZoneCommand::Swing {
                facing: u16::from_be_bytes([*high, *low]),
            })
        }
        GUARD_TAG => {
            let [_, _, lane] = payload else {
                return Err(ProtocolError::new(
                    "guard command payload must be exactly 3 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), GUARD_COMMAND_BYTES);
            Ok(ZoneCommand::Guard {
                lane: crate::wire::decode_lane(*lane)?,
            })
        }
        USE_DOOR_TAG => {
            let [_, _, door, open] = payload else {
                return Err(ProtocolError::new(
                    "use door command payload must be exactly 4 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), USE_DOOR_COMMAND_BYTES);
            let open = match open {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::new("door state must be 0 or 1")),
            };
            Ok(ZoneCommand::UseDoor {
                door: mmorpg_core::DoorId::new(*door),
                open,
            })
        }
        COMMAND_MINIONS_TAG => {
            if payload.len() != COMMAND_MINIONS_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "command minions payload must be exactly 11 bytes",
                ));
            }
            let mut offset = 2;
            Ok(ZoneCommand::CommandMinions(
                crate::wire::decode_minion_order(payload, &mut offset)?,
            ))
        }
        REPAIR_TAG => {
            let [_, _, high, low] = payload else {
                return Err(ProtocolError::new(
                    "repair command payload must be exactly 4 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), REPAIR_COMMAND_BYTES);
            Ok(ZoneCommand::Repair {
                piece: crate::wire::decode_piece(u16::from_be_bytes([*high, *low])),
            })
        }
        CHOOSE_CLASS_TAG => {
            let [_, _, class, sex] = payload else {
                return Err(ProtocolError::new(
                    "choose class command payload must be exactly 4 bytes",
                ));
            };
            debug_assert_eq!(payload.len(), CHOOSE_CLASS_COMMAND_BYTES);
            Ok(ZoneCommand::ChooseClass {
                class: *class,
                sex: *sex,
            })
        }
        EQUIP_ITEM_TAG => {
            let [_, _, bag_slot] = payload else {
                return Err(ProtocolError::new(
                    "equip item command payload must be exactly 3 bytes",
                ));
            };
            Ok(ZoneCommand::EquipItem {
                bag_slot: *bag_slot,
            })
        }
        UNEQUIP_ITEM_TAG => {
            let [_, _, equipment_slot] = payload else {
                return Err(ProtocolError::new(
                    "unequip item command payload must be exactly 3 bytes",
                ));
            };
            Ok(ZoneCommand::UnequipItem {
                equipment_slot: *equipment_slot,
            })
        }
        BUY_ITEM_TAG => {
            let (npc, offer, quantity) = decode_trade(payload, "buy item")?;
            Ok(ZoneCommand::BuyItem {
                npc,
                offer,
                quantity,
            })
        }
        SELL_ITEM_TAG => {
            let (npc, bag_slot, quantity) = decode_trade(payload, "sell item")?;
            Ok(ZoneCommand::SellItem {
                npc,
                bag_slot,
                quantity,
            })
        }
        CHAT_TAG => {
            let mut offset = BARE_COMMAND_BYTES;
            let (channel, text) = crate::chat::decode_text(payload, &mut offset)?;
            if offset != payload.len() {
                return Err(ProtocolError::new(
                    "chat command payload has trailing bytes",
                ));
            }
            Ok(ZoneCommand::Chat { channel, text })
        }
        EMOTE_TAG => {
            if payload.len() != EMOTE_COMMAND_BYTES {
                return Err(ProtocolError::new(
                    "emote command payload must be exactly 3 bytes",
                ));
            }
            let mut offset = BARE_COMMAND_BYTES;
            Ok(ZoneCommand::Emote(crate::chat::decode_emote(
                payload,
                &mut offset,
            )?))
        }
        ACCEPT_QUEST_TAG => {
            let [_, _, n0, n1, n2, n3, quest] = payload else {
                return Err(ProtocolError::new(
                    "accept quest command payload must be exactly 7 bytes",
                ));
            };
            Ok(ZoneCommand::AcceptQuest {
                npc: NpcId::new(u32::from_be_bytes([*n0, *n1, *n2, *n3])),
                quest: *quest,
            })
        }
        COMPLETE_QUEST_TAG => {
            let [_, _, n0, n1, n2, n3, quest, choice] = payload else {
                return Err(ProtocolError::new(
                    "complete quest command payload must be exactly 8 bytes",
                ));
            };
            Ok(ZoneCommand::CompleteQuest {
                npc: NpcId::new(u32::from_be_bytes([*n0, *n1, *n2, *n3])),
                quest: *quest,
                choice: *choice,
            })
        }
        ABANDON_QUEST_TAG => {
            let [_, _, quest] = payload else {
                return Err(ProtocolError::new(
                    "abandon quest command payload must be exactly 3 bytes",
                ));
            };
            Ok(ZoneCommand::AbandonQuest { quest: *quest })
        }
        CANCEL_CAST_TAG => bare(ZoneCommand::CancelCast),
        JUMP_TAG => bare(ZoneCommand::Jump),
        ROLL_TAG => bare(ZoneCommand::Roll),
        RELEASE_SPIRIT_TAG => bare(ZoneCommand::ReleaseSpirit),
        _ => Err(ProtocolError::new("unknown command tag")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{
        CreatureId, EntityRef, MinionOrder, NpcId, StrikeAim, StrikeHeight, StrikeSide,
    };

    #[test]
    fn commands_have_exact_layouts() {
        let movement = ZoneCommand::Move {
            forward: -1,
            strafe: 1,
            facing: 0xabcd,
        };
        assert_eq!(
            encode_command(movement),
            [PROTOCOL_BYTE, 1, 0xff, 1, 0xab, 0xcd]
        );
        assert_eq!(encode_command(ZoneCommand::Jump), [PROTOCOL_BYTE, 2]);
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
                CreatureId::new(0x0102_0304)
            )))),
            [PROTOCOL_BYTE, 3, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(None)),
            [PROTOCOL_BYTE, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            encode_command(ZoneCommand::Swing { facing: 0x1234 }),
            [PROTOCOL_BYTE, 4, 0x12, 0x34]
        );
        assert_eq!(
            encode_command(ZoneCommand::ReleaseSpirit),
            [PROTOCOL_BYTE, 6]
        );
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Creature(CreatureId::new(0x0102_0304))),
            }),
            [PROTOCOL_BYTE, 9, 9, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 3,
                target: None,
            }),
            [PROTOCOL_BYTE, 9, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::CancelCast), [PROTOCOL_BYTE, 10]);
        assert_eq!(
            encode_command(ZoneCommand::DirectedStrike {
                ability: 1,
                facing: 0xabcd,
                aim: StrikeAim::new(StrikeSide::Right, StrikeHeight::Low),
            }),
            [PROTOCOL_BYTE, 21, 1, 0xab, 0xcd, 2, 0]
        );
        assert_eq!(
            encode_command(ZoneCommand::Guard {
                lane: Some(StrikeHeight::High)
            }),
            [PROTOCOL_BYTE, 22, 3]
        );
        assert_eq!(
            encode_command(ZoneCommand::Guard { lane: None }),
            [PROTOCOL_BYTE, 22, 0]
        );
        assert_eq!(
            encode_command(ZoneCommand::ChooseClass { class: 2, sex: 1 }),
            [PROTOCOL_BYTE, 11, 2, 1]
        );
        assert_eq!(
            encode_command(ZoneCommand::EquipItem { bag_slot: 3 }),
            [PROTOCOL_BYTE, 12, 3]
        );
        assert_eq!(
            encode_command(ZoneCommand::UnequipItem { equipment_slot: 5 }),
            [PROTOCOL_BYTE, 13, 5]
        );
        assert_eq!(
            encode_command(ZoneCommand::BuyItem {
                npc: NpcId::new(0x0102_0304),
                offer: 6,
                quantity: 0x0a0b,
            }),
            [PROTOCOL_BYTE, 14, 1, 2, 3, 4, 6, 0x0a, 0x0b]
        );
        assert_eq!(
            encode_command(ZoneCommand::SellItem {
                npc: NpcId::new(3),
                bag_slot: 15,
                quantity: 2,
            }),
            [PROTOCOL_BYTE, 15, 0, 0, 0, 3, 15, 0, 2]
        );
        for command in [
            movement,
            ZoneCommand::Jump,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: u16::MAX,
            },
            ZoneCommand::SelectTarget(Some(EntityRef::Player(u32::MAX))),
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(9)))),
            ZoneCommand::SelectTarget(None),
            ZoneCommand::Swing { facing: u16::MAX },
            ZoneCommand::ReleaseSpirit,
            ZoneCommand::CancelCast,
            // Abilities without a directed mode are tick-time refusals.
            ZoneCommand::DirectedStrike {
                ability: u8::MAX,
                facing: u16::MAX,
                aim: StrikeAim::new(StrikeSide::Left, StrikeHeight::High),
            },
            ZoneCommand::UseAbility {
                ability: u8::MAX,
                target: Some(EntityRef::Npc(NpcId::new(u32::MAX))),
            },
            // Out-of-range class values are gameplay refusals, not wire errors.
            ZoneCommand::ChooseClass {
                class: u8::MAX,
                sex: u8::MAX,
            },
            // Out-of-range slots are tick-time refusals, not wire errors.
            ZoneCommand::EquipItem { bag_slot: u8::MAX },
            ZoneCommand::UnequipItem {
                equipment_slot: u8::MAX,
            },
            // Unknown quests, givers and choices are tick-time refusals.
            ZoneCommand::AcceptQuest {
                npc: NpcId::new(u32::MAX),
                quest: 0,
            },
            ZoneCommand::CompleteQuest {
                npc: NpcId::new(0),
                quest: u8::MAX,
                choice: u8::MAX,
            },
            ZoneCommand::AbandonQuest { quest: u8::MAX },
            // Unknown vendors, offers, slots and quantities are tick-time refusals.
            ZoneCommand::BuyItem {
                npc: NpcId::new(u32::MAX),
                offer: u8::MAX,
                quantity: u16::MAX,
            },
            ZoneCommand::SellItem {
                npc: NpcId::new(0),
                bag_slot: u8::MAX,
                quantity: 0,
            },
        ] {
            assert_eq!(decode_command(&encode_command(command)).unwrap(), command);
        }
    }

    /// The shared Rust/browser command fixture: one command per line as
    /// `<hex> <name> [fields]`.
    fn command_fixture() -> String {
        let commands = [
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 0,
            },
            ZoneCommand::Move {
                forward: 1,
                strafe: 1,
                facing: 16_384,
            },
            ZoneCommand::Move {
                forward: -1,
                strafe: -1,
                facing: 32_768,
            },
            ZoneCommand::Move {
                forward: 0,
                strafe: 1,
                facing: 0xabcd,
            },
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: u16::MAX,
            },
            ZoneCommand::Jump,
            ZoneCommand::Roll,
            ZoneCommand::Rise(1),
            ZoneCommand::Rise(0),
            ZoneCommand::Rise(-1),
            ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(108)))),
            ZoneCommand::SelectTarget(Some(EntityRef::Player(4_000_000_000))),
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(5)))),
            ZoneCommand::SelectTarget(None),
            ZoneCommand::Swing { facing: u16::MAX },
            ZoneCommand::ReleaseSpirit,
            ZoneCommand::MoveItem {
                source: 0,
                destination: 15,
                quantity: 2,
            },
            ZoneCommand::MoveItem {
                source: 15,
                destination: 0,
                quantity: u16::MAX,
            },
            ZoneCommand::MoveItem {
                source: u8::MAX,
                destination: 0,
                quantity: 0,
            },
            ZoneCommand::Loot(mmorpg_core::LootClaim {
                creature: CreatureId::new(1),
                died_at: 99,
            }),
            ZoneCommand::Loot(mmorpg_core::LootClaim {
                creature: CreatureId::new(u32::MAX),
                died_at: u64::MAX,
            }),
            ZoneCommand::UseAbility {
                ability: 1,
                target: None,
            },
            ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Creature(CreatureId::new(108))),
            },
            ZoneCommand::UseAbility {
                ability: u8::MAX,
                target: Some(EntityRef::Player(4_000_000_000)),
            },
            ZoneCommand::CancelCast,
            ZoneCommand::DirectedStrike {
                ability: 1,
                facing: 0,
                aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
            },
            ZoneCommand::DirectedStrike {
                ability: 1,
                facing: 0xabcd,
                aim: StrikeAim::new(StrikeSide::Left, StrikeHeight::High),
            },
            ZoneCommand::DirectedStrike {
                ability: u8::MAX,
                facing: u16::MAX,
                aim: StrikeAim::new(StrikeSide::Right, StrikeHeight::Low),
            },
            ZoneCommand::Guard {
                lane: Some(StrikeHeight::Low),
            },
            ZoneCommand::Guard {
                lane: Some(StrikeHeight::Middle),
            },
            ZoneCommand::Guard {
                lane: Some(StrikeHeight::High),
            },
            ZoneCommand::Guard { lane: None },
            ZoneCommand::ChooseClass { class: 0, sex: 1 },
            ZoneCommand::ChooseClass { class: 2, sex: 0 },
            ZoneCommand::ChooseClass {
                class: u8::MAX,
                sex: 7,
            },
            ZoneCommand::EquipItem { bag_slot: 0 },
            ZoneCommand::EquipItem { bag_slot: 15 },
            ZoneCommand::EquipItem { bag_slot: u8::MAX },
            ZoneCommand::UnequipItem { equipment_slot: 0 },
            ZoneCommand::UnequipItem { equipment_slot: 5 },
            ZoneCommand::UnequipItem {
                equipment_slot: u8::MAX,
            },
            ZoneCommand::BuyItem {
                npc: NpcId::new(3),
                offer: 0,
                quantity: 1,
            },
            ZoneCommand::BuyItem {
                npc: NpcId::new(u32::MAX),
                offer: u8::MAX,
                quantity: u16::MAX,
            },
            ZoneCommand::SellItem {
                npc: NpcId::new(3),
                bag_slot: 0,
                quantity: 3,
            },
            ZoneCommand::SellItem {
                npc: NpcId::new(0),
                bag_slot: u8::MAX,
                quantity: 0,
            },
            ZoneCommand::Chat {
                channel: mmorpg_core::ChatChannel::Say,
                text: mmorpg_core::ChatText::new("Hail, Greyhaven!").unwrap(),
            },
            ZoneCommand::Chat {
                channel: mmorpg_core::ChatChannel::Yell,
                text: mmorpg_core::ChatText::new("Grüße!").unwrap(),
            },
            ZoneCommand::Emote(mmorpg_core::Emote::Wave),
            ZoneCommand::Emote(mmorpg_core::Emote::Point),
            ZoneCommand::AcceptQuest {
                npc: NpcId::new(1),
                quest: 1,
            },
            ZoneCommand::AcceptQuest {
                npc: NpcId::new(u32::MAX),
                quest: u8::MAX,
            },
            ZoneCommand::CompleteQuest {
                npc: NpcId::new(2),
                quest: 2,
                choice: 1,
            },
            ZoneCommand::CompleteQuest {
                npc: NpcId::new(0),
                quest: 0,
                choice: u8::MAX,
            },
            ZoneCommand::AbandonQuest { quest: 3 },
            ZoneCommand::UseDoor {
                door: mmorpg_core::DoorId::new(1),
                open: true,
            },
            ZoneCommand::UseDoor {
                door: mmorpg_core::DoorId::new(u8::MAX),
                open: false,
            },
            ZoneCommand::Repair {
                piece: Some(mmorpg_core::PieceId::new(3)),
            },
            ZoneCommand::Repair { piece: None },
            ZoneCommand::CommandMinions(MinionOrder::Follow),
            ZoneCommand::CommandMinions(MinionOrder::Attack(EntityRef::Creature(CreatureId::new(
                108,
            )))),
            ZoneCommand::CommandMinions(MinionOrder::Defend([-1_234, 56_789])),
            ZoneCommand::CommandMinions(MinionOrder::Retreat),
            ZoneCommand::ReleaseAbility {
                ability: 4,
                retired: false,
            },
            ZoneCommand::ReleaseAbility {
                ability: u8::MAX,
                retired: true,
            },
        ];
        // Observed commands (#415): the inner command after `observed <tick>`.
        let observed = [
            (
                ZoneCommand::UseAbility {
                    ability: 3,
                    target: Some(EntityRef::Creature(CreatureId::new(108))),
                },
                1_234,
            ),
            (
                ZoneCommand::DirectedStrike {
                    ability: 1,
                    facing: 16_384,
                    aim: StrikeAim::new(StrikeSide::Left, StrikeHeight::High),
                },
                u64::MAX,
            ),
            (ZoneCommand::Jump, 0),
        ];
        let mut fixture =
            String::from("# Command golden fixture, verified by mmorpg-protocol and web tests.\n");
        for (command, observed) in commands
            .into_iter()
            .map(|command| (command, None))
            .chain(observed.map(|(command, tick)| (command, Some(tick))))
        {
            let bytes = match observed {
                Some(tick) => encode_observed_command(command, tick),
                None => encode_command(command),
            };
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let prefix = observed.map_or_else(String::new, |tick| format!("observed {tick} "));
            let fields = match command {
                ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                } => format!("move {forward} {strafe} {facing}"),
                ZoneCommand::Jump => "jump".to_owned(),
                ZoneCommand::Roll => "roll".to_owned(),
                ZoneCommand::Rise(rise) => format!("rise {rise}"),
                ZoneCommand::Loot(claim) => {
                    format!("loot {} {}", claim.creature.get(), claim.died_at)
                }
                ZoneCommand::SelectTarget(target) => {
                    let (kind, id) = entity_fields(target);
                    format!("select_target {kind} {id}")
                }
                ZoneCommand::UseAbility { ability, target } => {
                    let (kind, id) = entity_fields(target);
                    format!("use_ability {ability} {kind} {id}")
                }
                ZoneCommand::ReleaseAbility { ability, retired } => {
                    format!("release_ability {ability} {}", u8::from(retired))
                }
                ZoneCommand::CancelCast => "cancel_cast".to_owned(),
                ZoneCommand::DirectedStrike {
                    ability,
                    facing,
                    aim,
                } => format!(
                    "directed_strike {ability} {facing} {} {}",
                    aim.side.code(),
                    aim.height.code()
                ),
                ZoneCommand::Guard { lane } => {
                    format!("guard {}", crate::wire::encode_lane(lane))
                }
                ZoneCommand::UseDoor { door, open } => {
                    format!("use_door {} {}", door.get(), u8::from(open))
                }
                ZoneCommand::Repair { piece } => {
                    format!("repair {}", crate::wire::encode_piece(piece))
                }
                ZoneCommand::CommandMinions(order) => match order {
                    MinionOrder::Follow => "command_minions follow".to_owned(),
                    MinionOrder::Attack(target) => {
                        let (kind, id) = entity_fields(Some(target));
                        format!("command_minions attack {kind} {id}")
                    }
                    MinionOrder::Defend([x, z]) => format!("command_minions defend {x} {z}"),
                    MinionOrder::Retreat => "command_minions retreat".to_owned(),
                },
                ZoneCommand::ChooseClass { class, sex } => format!("choose_class {class} {sex}"),
                ZoneCommand::EquipItem { bag_slot } => format!("equip_item {bag_slot}"),
                ZoneCommand::UnequipItem { equipment_slot } => {
                    format!("unequip_item {equipment_slot}")
                }
                ZoneCommand::BuyItem {
                    npc,
                    offer,
                    quantity,
                } => format!("buy_item {} {offer} {quantity}", npc.get()),
                ZoneCommand::SellItem {
                    npc,
                    bag_slot,
                    quantity,
                } => format!("sell_item {} {bag_slot} {quantity}", npc.get()),
                ZoneCommand::Chat { channel, text } => {
                    let channel = match channel {
                        mmorpg_core::ChatChannel::Say => "say",
                        mmorpg_core::ChatChannel::Yell => "yell",
                    };
                    // The text is hex so the line stays whitespace-separated.
                    let text = text
                        .as_str()
                        .bytes()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    format!("chat {channel} {text}")
                }
                ZoneCommand::Emote(emote) => format!("emote {}", emote.code()),
                ZoneCommand::AcceptQuest { npc, quest } => {
                    format!("accept_quest {} {quest}", npc.get())
                }
                ZoneCommand::CompleteQuest { npc, quest, choice } => {
                    format!("complete_quest {} {quest} {choice}", npc.get())
                }
                ZoneCommand::AbandonQuest { quest } => format!("abandon_quest {quest}"),
                ZoneCommand::Swing { facing } => format!("swing {facing}"),
                ZoneCommand::ReleaseSpirit => "release_spirit".to_owned(),
                ZoneCommand::MoveItem {
                    source,
                    destination,
                    quantity,
                } => format!("move_item {source} {destination} {quantity}"),
            };
            fixture.push_str(&format!("{hex} {prefix}{fields}\n"));
        }
        fixture
    }

    fn entity_fields(target: Option<EntityRef>) -> (u8, u32) {
        target.map_or((0, 0), |target| {
            let kind = match target.kind() {
                mmorpg_core::EntityKind::Player => 1,
                mmorpg_core::EntityKind::Creature => 2,
                mmorpg_core::EntityKind::Npc => 3,
                mmorpg_core::EntityKind::Minion => 4,
            };
            (kind, target.id())
        })
    }

    #[test]
    fn commands_match_the_shared_golden_fixture() {
        let checked_in = mmorpg_core::golden::golden(
            mmorpg_core::golden::workspace_root().join("fixtures/protocol/commands.hex"),
            &command_fixture(),
        );
        assert_eq!(checked_in, command_fixture());
        for line in checked_in.lines().filter(|line| !line.starts_with('#')) {
            let hex = line.split_whitespace().next().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let encoded = match decode_observed_command(&bytes).unwrap() {
                (command, Some(tick)) => encode_observed_command(command, tick),
                (command, None) => encode_command(command),
            };
            assert_eq!(encoded, bytes);
        }
    }

    #[test]
    fn observed_commands_wrap_exactly_one_plain_command() {
        let observed = encode_observed_command(ZoneCommand::Swing { facing: 9 }, 77);
        assert!(
            decode_command(&observed).is_err(),
            "plain decoding is strict"
        );
        assert_eq!(
            decode_observed_command(&observed).unwrap(),
            (ZoneCommand::Swing { facing: 9 }, Some(77))
        );
        assert_eq!(
            decode_observed_command(&encode_command(ZoneCommand::Jump)).unwrap(),
            (ZoneCommand::Jump, None)
        );
        let mut nested = observed[..10].to_vec();
        nested.extend_from_slice(&observed[1..]);
        assert!(decode_observed_command(&nested).is_err());
        assert!(decode_observed_command(&observed[..9]).is_err());
        assert!(decode_observed_command(&observed[..10]).is_err());
    }

    #[test]
    fn commands_of_any_other_protocol_are_rejected() {
        for command in [
            ZoneCommand::Jump,
            ZoneCommand::Rise(1),
            ZoneCommand::Swing { facing: u16::MAX },
        ] {
            let mut other = encode_command(command);
            for byte in [0, PROTOCOL_BYTE ^ 1, PROTOCOL_BYTE.wrapping_add(1), u8::MAX] {
                if byte == PROTOCOL_BYTE {
                    continue;
                }
                other[0] = byte;
                assert!(decode_command(&other).is_err());
            }
        }
    }

    #[test]
    fn command_decoding_is_strict() {
        let movement = encode_command(ZoneCommand::Move {
            forward: 1,
            strafe: -1,
            facing: 7,
        });
        let select = encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
            CreatureId::new(7),
        ))));
        let loot = encode_command(ZoneCommand::Loot(mmorpg_core::LootClaim {
            creature: CreatureId::new(1),
            died_at: 99,
        }));
        let ability = encode_command(ZoneCommand::UseAbility {
            ability: 5,
            target: Some(EntityRef::Creature(CreatureId::new(7))),
        });
        let directed = encode_command(ZoneCommand::DirectedStrike {
            ability: 1,
            facing: 7,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Low),
        });
        let guard = encode_command(ZoneCommand::Guard {
            lane: Some(StrikeHeight::Low),
        });
        let class = encode_command(ZoneCommand::ChooseClass { class: 1, sex: 0 });
        let equip = encode_command(ZoneCommand::EquipItem { bag_slot: 1 });
        let unequip = encode_command(ZoneCommand::UnequipItem { equipment_slot: 1 });
        let buy = encode_command(ZoneCommand::BuyItem {
            npc: NpcId::new(3),
            offer: 1,
            quantity: 1,
        });
        let sell = encode_command(ZoneCommand::SellItem {
            npc: NpcId::new(3),
            bag_slot: 1,
            quantity: 1,
        });
        let chat = encode_command(ZoneCommand::Chat {
            channel: mmorpg_core::ChatChannel::Say,
            text: mmorpg_core::ChatText::new("hi").unwrap(),
        });
        assert_eq!(chat, [PROTOCOL_BYTE, 16, 0, 2, b'h', b'i']);
        let accept = encode_command(ZoneCommand::AcceptQuest {
            npc: NpcId::new(1),
            quest: 2,
        });
        assert_eq!(accept, [PROTOCOL_BYTE, 18, 0, 0, 0, 1, 2]);
        let complete = encode_command(ZoneCommand::CompleteQuest {
            npc: NpcId::new(0x0102_0304),
            quest: 9,
            choice: 2,
        });
        assert_eq!(complete, [PROTOCOL_BYTE, 19, 1, 2, 3, 4, 9, 2]);
        let abandon = encode_command(ZoneCommand::AbandonQuest { quest: 7 });
        assert_eq!(abandon, [PROTOCOL_BYTE, 20, 7]);
        let emote = encode_command(ZoneCommand::Emote(mmorpg_core::Emote::Bow));
        assert_eq!(emote, [PROTOCOL_BYTE, 17, 2]);
        for invalid in [
            vec![PROTOCOL_BYTE, 16, 2, 2, b'h', b'i'],
            vec![PROTOCOL_BYTE, 16, 0, 0],
            vec![PROTOCOL_BYTE, 16, 0, 2, 0xff, 0xfe],
            vec![PROTOCOL_BYTE, 16, 0, 2, b'\n', b'i'],
            vec![PROTOCOL_BYTE, 16, 0, 2, b' ', b' '],
            vec![PROTOCOL_BYTE, 16, 0, 81]
                .into_iter()
                .chain([b'a'; 81])
                .collect(),
            // Unknown strike sides and heights are malformed.
            vec![PROTOCOL_BYTE, 21, 1, 0, 0, 3, 0],
            vec![PROTOCOL_BYTE, 21, 1, 0, 0, 0, 3],
            vec![PROTOCOL_BYTE, 21, 1, 0, 0, u8::MAX, u8::MAX],
            // Unknown guard lanes are malformed.
            vec![PROTOCOL_BYTE, 22, 4],
            vec![PROTOCOL_BYTE, 22, u8::MAX],
            vec![PROTOCOL_BYTE, 25, 2],
            vec![PROTOCOL_BYTE, 25, 0xfe],
            // Unknown emote IDs are malformed.
            vec![PROTOCOL_BYTE, 17, 0],
            vec![PROTOCOL_BYTE, 17, 6],
            vec![PROTOCOL_BYTE, 17, u8::MAX],
        ] {
            assert!(decode_command(&invalid).is_err(), "{invalid:?}");
        }
        for encoded in [
            &movement, &select, &loot, &ability, &directed, &guard, &class, &equip, &unequip, &buy,
            &sell, &chat, &emote, &accept, &complete, &abandon,
        ] {
            for length in 0..encoded.len() {
                assert!(decode_command(&encoded[..length]).is_err(), "{length}");
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(decode_command(&trailing).is_err());
        }
        // A swing (#517) carries exactly its facing; the retired stop-attack tag is unknown.
        for length in 0..4 {
            assert!(decode_command(&[PROTOCOL_BYTE, 4, 0, 0][..length]).is_err());
        }
        assert!(decode_command(&[PROTOCOL_BYTE, 4, 0, 0, 0]).is_err());
        assert_eq!(
            decode_command(&[PROTOCOL_BYTE, 5]).unwrap_err().to_string(),
            "unknown command tag"
        );
        for tag in [2, 6, 10] {
            assert_eq!(
                decode_command(&[PROTOCOL_BYTE, tag, 0])
                    .unwrap_err()
                    .to_string(),
                "command payload must be exactly 2 bytes",
                "tag {tag} has no body"
            );
        }
        // Minion orders (#295): exact length, known order codes, zero padding, a real target.
        let attack = encode_command(ZoneCommand::CommandMinions(MinionOrder::Attack(
            EntityRef::Creature(CreatureId::new(7)),
        )));
        for length in 0..attack.len() {
            assert!(decode_command(&attack[..length]).is_err(), "{length}");
        }
        for (offset, value) in [(2, 0), (2, 5), (3, 0), (8, 1), (10, 1)] {
            let mut invalid = attack.clone();
            invalid[offset] = value;
            assert!(decode_command(&invalid).is_err(), "byte {offset} = {value}");
        }
        let mut follow = encode_command(ZoneCommand::CommandMinions(MinionOrder::Follow));
        follow[5] = 1;
        assert!(decode_command(&follow).is_err());
        // A held ability's release (#414): exact length and a 0/1 retired flag.
        let release = encode_command(ZoneCommand::ReleaseAbility {
            ability: 2,
            retired: true,
        });
        for length in 0..release.len() {
            assert!(decode_command(&release[..length]).is_err(), "{length}");
        }
        let mut trailing = release.clone();
        trailing.push(0);
        assert!(decode_command(&trailing).is_err());
        let mut flag = release;
        flag[3] = 2;
        assert!(decode_command(&flag).is_err());
        for unknown in [0, 27, u8::MAX] {
            assert_eq!(
                decode_command(&[PROTOCOL_BYTE, unknown])
                    .unwrap_err()
                    .to_string(),
                "unknown command tag"
            );
        }
        for version in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
            let mut other = movement.clone();
            other[0] = version;
            assert_eq!(
                decode_command(&other).unwrap_err().to_string(),
                "unsupported command protocol id"
            );
        }
        assert!(
            decode_command(&[1, 1, 0, 1]).is_err(),
            "legacy v1 movement is rejected"
        );
        for (forward, strafe) in [(2, 0), (0, -2), (-128, 0), (0, 127)] {
            let mut invalid = movement.clone();
            invalid[2] = i8::to_be_bytes(forward)[0];
            invalid[3] = i8::to_be_bytes(strafe)[0];
            assert_eq!(
                decode_command(&invalid).unwrap_err().to_string(),
                "movement components must be between -1 and 1"
            );
        }
        assert_eq!(
            decode_command(&[PROTOCOL_BYTE, 3, 5, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[PROTOCOL_BYTE, 9, 1, 5, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[PROTOCOL_BYTE, 3, 0, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "an absent entity must have ID 0"
        );
    }
}
