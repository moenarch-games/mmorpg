//! Command wire version 4 added ability use (tag 9), cast cancellation
//! (tag 10) and the class choice (tag 11); version 5 added equipping
//! (tag 12) and unequipping (tag 13); version 6 adds vendor purchases
//! (tag 14) and sales (tag 15); version 7 added chat (tag 16); version 8
//! added emotes (tag 17); version 9 adds accepting (tag 18), turning in
//! (tag 19) and abandoning (tag 20) quests.

use mmorpg_core::{NpcId, ZoneCommand};

use crate::ProtocolError;
use crate::wire::{decode_entity_ref, encode_entity_ref};

pub const COMMAND_WIRE_VERSION: u8 = 9;

const MOVE_TAG: u8 = 1;
const JUMP_TAG: u8 = 2;
const SELECT_TARGET_TAG: u8 = 3;
const START_ATTACK_TAG: u8 = 4;
const STOP_ATTACK_TAG: u8 = 5;
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
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, MOVE_TAG]);
            payload.extend_from_slice(&forward.to_be_bytes());
            payload.extend_from_slice(&strafe.to_be_bytes());
            payload.extend_from_slice(&facing.to_be_bytes());
            payload
        }
        ZoneCommand::Loot(claim) => {
            let mut payload = vec![COMMAND_WIRE_VERSION, LOOT_TAG];
            crate::loot::encode_claim(&mut payload, claim);
            payload
        }
        ZoneCommand::Jump => vec![COMMAND_WIRE_VERSION, JUMP_TAG],
        ZoneCommand::UseAbility { ability, target } => {
            let mut payload = Vec::with_capacity(USE_ABILITY_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, USE_ABILITY_TAG, ability]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::CancelCast => vec![COMMAND_WIRE_VERSION, CANCEL_CAST_TAG],
        ZoneCommand::ChooseClass { class, sex } => {
            vec![COMMAND_WIRE_VERSION, CHOOSE_CLASS_TAG, class, sex]
        }
        ZoneCommand::EquipItem { bag_slot } => {
            vec![COMMAND_WIRE_VERSION, EQUIP_ITEM_TAG, bag_slot]
        }
        ZoneCommand::UnequipItem { equipment_slot } => {
            vec![COMMAND_WIRE_VERSION, UNEQUIP_ITEM_TAG, equipment_slot]
        }
        ZoneCommand::SelectTarget(target) => {
            let mut payload = Vec::with_capacity(SELECT_TARGET_COMMAND_BYTES);
            payload.extend_from_slice(&[COMMAND_WIRE_VERSION, SELECT_TARGET_TAG]);
            encode_entity_ref(&mut payload, target);
            payload
        }
        ZoneCommand::StartAttack => vec![COMMAND_WIRE_VERSION, START_ATTACK_TAG],
        ZoneCommand::StopAttack => vec![COMMAND_WIRE_VERSION, STOP_ATTACK_TAG],
        ZoneCommand::ReleaseSpirit => vec![COMMAND_WIRE_VERSION, RELEASE_SPIRIT_TAG],
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
        ZoneCommand::Emote(emote) => vec![COMMAND_WIRE_VERSION, EMOTE_TAG, emote.code()],
        ZoneCommand::AcceptQuest { npc, quest } => {
            let mut payload = vec![COMMAND_WIRE_VERSION, ACCEPT_QUEST_TAG];
            payload.extend_from_slice(&npc.get().to_be_bytes());
            payload.push(quest);
            payload
        }
        ZoneCommand::CompleteQuest { npc, quest, choice } => {
            let mut payload = vec![COMMAND_WIRE_VERSION, COMPLETE_QUEST_TAG];
            payload.extend_from_slice(&npc.get().to_be_bytes());
            payload.extend_from_slice(&[quest, choice]);
            payload
        }
        ZoneCommand::AbandonQuest { quest } => {
            vec![COMMAND_WIRE_VERSION, ABANDON_QUEST_TAG, quest]
        }
        ZoneCommand::Chat { channel, text } => {
            let mut payload = vec![COMMAND_WIRE_VERSION, CHAT_TAG];
            crate::chat::encode_text(&mut payload, channel, &text);
            payload
        }
        ZoneCommand::MoveItem {
            source,
            destination,
            quantity,
        } => {
            let [high, low] = quantity.to_be_bytes();
            vec![
                COMMAND_WIRE_VERSION,
                MOVE_ITEM_TAG,
                source,
                destination,
                high,
                low,
            ]
        }
    }
}

fn encode_trade(tag: u8, npc: NpcId, index: u8, quantity: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(TRADE_COMMAND_BYTES);
    payload.extend_from_slice(&[COMMAND_WIRE_VERSION, tag]);
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

pub fn decode_command(payload: &[u8]) -> Result<ZoneCommand, ProtocolError> {
    let [version, tag, ..] = payload else {
        return Err(ProtocolError::new("command payload is truncated"));
    };
    if *version != COMMAND_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported command wire version"));
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
        START_ATTACK_TAG => bare(ZoneCommand::StartAttack),
        STOP_ATTACK_TAG => bare(ZoneCommand::StopAttack),
        RELEASE_SPIRIT_TAG => bare(ZoneCommand::ReleaseSpirit),
        _ => Err(ProtocolError::new("unknown command tag")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, EntityRef, NpcId};

    #[test]
    fn commands_have_exact_versioned_layouts() {
        let movement = ZoneCommand::Move {
            forward: -1,
            strafe: 1,
            facing: 0xabcd,
        };
        assert_eq!(encode_command(movement), [9, 1, 0xff, 1, 0xab, 0xcd]);
        assert_eq!(encode_command(ZoneCommand::Jump), [9, 2]);
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
                CreatureId::new(0x0102_0304)
            )))),
            [9, 3, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::SelectTarget(None)),
            [9, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::StartAttack), [9, 4]);
        assert_eq!(encode_command(ZoneCommand::StopAttack), [9, 5]);
        assert_eq!(encode_command(ZoneCommand::ReleaseSpirit), [9, 6]);
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Creature(CreatureId::new(0x0102_0304))),
            }),
            [9, 9, 9, 2, 1, 2, 3, 4]
        );
        assert_eq!(
            encode_command(ZoneCommand::UseAbility {
                ability: 3,
                target: None,
            }),
            [9, 9, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(encode_command(ZoneCommand::CancelCast), [9, 10]);
        assert_eq!(
            encode_command(ZoneCommand::ChooseClass { class: 2, sex: 1 }),
            [9, 11, 2, 1]
        );
        assert_eq!(
            encode_command(ZoneCommand::EquipItem { bag_slot: 3 }),
            [9, 12, 3]
        );
        assert_eq!(
            encode_command(ZoneCommand::UnequipItem { equipment_slot: 5 }),
            [9, 13, 5]
        );
        assert_eq!(
            encode_command(ZoneCommand::BuyItem {
                npc: NpcId::new(0x0102_0304),
                offer: 6,
                quantity: 0x0a0b,
            }),
            [9, 14, 1, 2, 3, 4, 6, 0x0a, 0x0b]
        );
        assert_eq!(
            encode_command(ZoneCommand::SellItem {
                npc: NpcId::new(3),
                bag_slot: 15,
                quantity: 2,
            }),
            [9, 15, 0, 0, 0, 3, 15, 0, 2]
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
            ZoneCommand::StartAttack,
            ZoneCommand::StopAttack,
            ZoneCommand::ReleaseSpirit,
            ZoneCommand::CancelCast,
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
            ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(108)))),
            ZoneCommand::SelectTarget(Some(EntityRef::Player(4_000_000_000))),
            ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(5)))),
            ZoneCommand::SelectTarget(None),
            ZoneCommand::StartAttack,
            ZoneCommand::StopAttack,
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
        ];
        let mut fixture = String::from(
            "# Command wire v9 golden fixture, verified by mmorpg-protocol and web tests.\n",
        );
        for command in commands {
            let hex = encode_command(command)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let fields = match command {
                ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                } => format!("move {forward} {strafe} {facing}"),
                ZoneCommand::Jump => "jump".to_owned(),
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
                ZoneCommand::CancelCast => "cancel_cast".to_owned(),
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
                ZoneCommand::StartAttack => "start_attack".to_owned(),
                ZoneCommand::StopAttack => "stop_attack".to_owned(),
                ZoneCommand::ReleaseSpirit => "release_spirit".to_owned(),
                ZoneCommand::MoveItem {
                    source,
                    destination,
                    quantity,
                } => format!("move_item {source} {destination} {quantity}"),
            };
            fixture.push_str(&format!("{hex} {fields}\n"));
        }
        fixture
    }

    fn entity_fields(target: Option<EntityRef>) -> (u8, u32) {
        target.map_or((0, 0), |target| {
            let kind = match target.kind() {
                mmorpg_core::EntityKind::Player => 1,
                mmorpg_core::EntityKind::Creature => 2,
                mmorpg_core::EntityKind::Npc => 3,
            };
            (kind, target.id())
        })
    }

    #[test]
    fn commands_match_the_shared_golden_fixture() {
        let checked_in = include_str!("../../../fixtures/protocol/commands-v9.hex");
        if std::env::var_os("MMORPG_PRINT_FIXTURE").is_some() {
            print!("{}", command_fixture());
        }
        assert_eq!(checked_in, command_fixture());
        for line in checked_in.lines().filter(|line| !line.starts_with('#')) {
            let hex = line.split_whitespace().next().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(encode_command(decode_command(&bytes).unwrap()), bytes);
        }
    }

    #[test]
    fn actual_legacy_v2_to_v8_command_fixtures_are_rejected() {
        for line in include_str!("../../../fixtures/protocol/commands-v2.hex")
            .lines()
            .chain(include_str!("../../../fixtures/protocol/commands-v3.hex").lines())
            .chain(include_str!("../../../fixtures/protocol/commands-v4.hex").lines())
            .chain(include_str!("../../../fixtures/protocol/commands-v5.hex").lines())
            .chain(include_str!("../../../fixtures/protocol/commands-v6.hex").lines())
            .chain(include_str!("../../../fixtures/protocol/commands-v7.hex").lines())
            .chain(include_str!("../../../fixtures/protocol/commands-v8.hex").lines())
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let hex = line.split_whitespace().next().unwrap();
            let old = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect::<Vec<_>>();
            assert!(decode_command(&old).is_err());
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
        assert_eq!(chat, [9, 16, 0, 2, b'h', b'i']);
        let accept = encode_command(ZoneCommand::AcceptQuest {
            npc: NpcId::new(1),
            quest: 2,
        });
        assert_eq!(accept, [9, 18, 0, 0, 0, 1, 2]);
        let complete = encode_command(ZoneCommand::CompleteQuest {
            npc: NpcId::new(0x0102_0304),
            quest: 9,
            choice: 2,
        });
        assert_eq!(complete, [9, 19, 1, 2, 3, 4, 9, 2]);
        let abandon = encode_command(ZoneCommand::AbandonQuest { quest: 7 });
        assert_eq!(abandon, [9, 20, 7]);
        let emote = encode_command(ZoneCommand::Emote(mmorpg_core::Emote::Bow));
        assert_eq!(emote, [9, 17, 2]);
        for invalid in [
            vec![9, 16, 2, 2, b'h', b'i'],
            vec![9, 16, 0, 0],
            vec![9, 16, 0, 2, 0xff, 0xfe],
            vec![9, 16, 0, 2, b'\n', b'i'],
            vec![9, 16, 0, 2, b' ', b' '],
            vec![9, 16, 0, 81].into_iter().chain([b'a'; 81]).collect(),
            // Unknown emote IDs are malformed.
            vec![9, 17, 0],
            vec![9, 17, 6],
            vec![9, 17, u8::MAX],
        ] {
            assert!(decode_command(&invalid).is_err(), "{invalid:?}");
        }
        for encoded in [
            &movement, &select, &loot, &ability, &class, &equip, &unequip, &buy, &sell, &chat,
            &emote, &accept, &complete, &abandon,
        ] {
            for length in 0..encoded.len() {
                assert!(decode_command(&encoded[..length]).is_err(), "{length}");
            }
            let mut trailing = encoded.clone();
            trailing.push(0);
            assert!(decode_command(&trailing).is_err());
        }
        for tag in [2, 4, 5, 6, 10] {
            assert_eq!(
                decode_command(&[9, tag, 0]).unwrap_err().to_string(),
                "command payload must be exactly 2 bytes",
                "tag {tag} has no body"
            );
        }
        for unknown in [0, 21, u8::MAX] {
            assert_eq!(
                decode_command(&[9, unknown]).unwrap_err().to_string(),
                "unknown command tag"
            );
        }
        for version in [1, 2, 3, 4, 5, 6, 7, 8, 10] {
            let mut other = movement.clone();
            other[0] = version;
            assert_eq!(
                decode_command(&other).unwrap_err().to_string(),
                "unsupported command wire version"
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
            decode_command(&[9, 3, 4, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[9, 9, 1, 4, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "unknown entity kind"
        );
        assert_eq!(
            decode_command(&[9, 3, 0, 0, 0, 0, 1])
                .unwrap_err()
                .to_string(),
            "an absent entity must have ID 0"
        );
    }
}
