//! Sheets (#544): the parts of a player projection that rarely change, sent only until the
//! client acknowledges them (docs/PROTOCOL.md).
//!
//! A sheet's revision is a hash of its encoded bytes, so equal contents always share a
//! revision, on any host. A projection carries a sheet only while the client's acknowledged
//! revision differs from the current one; the client echoes the revisions it holds in its
//! command datagrams ([`encode_acknowledged_command`]).

use mmorpg_core::{
    AbilityId, AuraKind, AuraView, Cooldown, EntityRef, Equipment, Inventory, LootView, MAX_AURAS,
    MAX_COOLDOWNS, QuestSheet, ZoneSnapshot, ability_by_id,
};

use crate::ProtocolError;
use crate::inventory::{
    EQUIPMENT_RECORD_BYTES, INVENTORY_RECORD_BYTES, decode_equipment, decode_inventory,
    encode_equipment, encode_inventory,
};
use crate::wire::{
    ENTITY_REF_BYTES, decode_entity_ref, decode_u8_count, encode_entity_ref, encode_u8_count,
    ensure_fully_consumed, read_bool, read_u8, take,
};

/// The sheets in wire order; a projection's sheet mask has bit `index` for each one it carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Sheet {
    /// Inventory revision, bag, equipment and its stat totals.
    Inventory,
    /// Quest log, turned-in quests and NPC markers.
    Quests,
    /// The selected corpse's loot, or none.
    Loot,
    /// The viewer's cooldowns and auras and its target's auras.
    Abilities,
}

impl Sheet {
    pub(crate) const ALL: [Self; SHEET_COUNT] =
        [Self::Inventory, Self::Quests, Self::Loot, Self::Abilities];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

pub(crate) const SHEET_COUNT: usize = 4;

/// The four `u32` revisions a client echoes.
pub const SHEET_REVISIONS_BYTES: usize = SHEET_COUNT * 4;

/// Inventory revision (`u64`), bag, equipment and four `u16` stat totals.
const MAX_INVENTORY_SHEET_BYTES: usize =
    8 + INVENTORY_RECORD_BYTES + EQUIPMENT_RECORD_BYTES + 4 * 2;
/// Presence byte, then the claim and rewards.
const MAX_LOOT_SHEET_BYTES: usize = 1 + crate::loot::MAX_LOOT_SHEET_BYTES;
/// Ability ID and the `u16` tick it ends.
const COOLDOWN_RECORD_BYTES: usize = 1 + 2;
/// Ability ID, aura kind, the `u16` tick it ends and amount.
const AURA_RECORD_BYTES: usize = 1 + 1 + 2 + 2;
/// The target the target auras belong to, then the cooldown, aura and target aura lists.
const MAX_ABILITY_SHEET_BYTES: usize = ENTITY_REF_BYTES
    + 1
    + MAX_COOLDOWNS * COOLDOWN_RECORD_BYTES
    + 2 * (1 + MAX_AURAS * AURA_RECORD_BYTES);
/// Every sheet at its largest.
pub(crate) const MAX_SHEETS_BYTES: usize = MAX_INVENTORY_SHEET_BYTES
    + crate::quest::MAX_QUEST_SHEET_BYTES
    + MAX_LOOT_SHEET_BYTES
    + MAX_ABILITY_SHEET_BYTES;

/// The sheet revisions a client holds, `0` for a sheet it has none of. A host sends a sheet
/// only while the client's revision differs from the current one.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SheetRevisions {
    pub inventory: u32,
    pub quests: u32,
    pub loot: u32,
    pub abilities: u32,
}

impl SheetRevisions {
    pub(crate) const fn get(self, sheet: Sheet) -> u32 {
        match sheet {
            Sheet::Inventory => self.inventory,
            Sheet::Quests => self.quests,
            Sheet::Loot => self.loot,
            Sheet::Abilities => self.abilities,
        }
    }

    pub(crate) fn set(&mut self, sheet: Sheet, revision: u32) {
        match sheet {
            Sheet::Inventory => self.inventory = revision,
            Sheet::Quests => self.quests = revision,
            Sheet::Loot => self.loot = revision,
            Sheet::Abilities => self.abilities = revision,
        }
    }

    pub(crate) fn encode(self, payload: &mut Vec<u8>) {
        for sheet in Sheet::ALL {
            payload.extend_from_slice(&self.get(sheet).to_be_bytes());
        }
    }

    pub(crate) fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let mut revisions = Self::default();
        for sheet in Sheet::ALL {
            revisions.set(sheet, u32::from_be_bytes(take(payload, offset)?));
        }
        Ok(revisions)
    }
}

/// FNV-1a over a sheet's bytes; `0` stays reserved for "none held".
pub(crate) fn revision(bytes: &[u8]) -> u32 {
    let hash = bytes.iter().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    });
    hash.max(1)
}

/// Each sheet's bytes at `snapshot.tick`; the bag and quest sheets are absent when the
/// projection has none to send.
pub(crate) fn encode_sheets(
    snapshot: &ZoneSnapshot,
) -> Result<[Option<Vec<u8>>; SHEET_COUNT], ProtocolError> {
    let inventory = match (&snapshot.inventory, &snapshot.equipment) {
        (Some(inventory), Some(equipment)) => Some(encode_inventory_sheet(
            snapshot.inventory_revision,
            inventory,
            equipment,
        )),
        (None, None) => None,
        _ => {
            return Err(ProtocolError::new(
                "the bag sheet needs both the bag and the equipment",
            ));
        }
    };
    let quests = match &snapshot.quests {
        Some(quests) => {
            let mut bytes = Vec::new();
            crate::quest::encode_sheet(&mut bytes, quests)?;
            Some(bytes)
        }
        None => None,
    };
    let mut loot = vec![u8::from(snapshot.loot.is_some())];
    if let Some(view) = snapshot.loot {
        crate::loot::encode_claim(&mut loot, view.claim);
        crate::loot::encode_rewards(&mut loot, view.rewards);
    }
    let mut abilities = Vec::new();
    encode_entity_ref(&mut abilities, snapshot.viewer.target);
    abilities.push(encode_u8_count(
        snapshot.cooldowns.len(),
        MAX_COOLDOWNS,
        "player projection has too many cooldowns",
    )?);
    let now = tick_bits(snapshot.tick);
    for cooldown in &snapshot.cooldowns {
        abilities.push(cooldown.ability.get());
        abilities.extend_from_slice(&now.wrapping_add(cooldown.remaining).to_be_bytes());
    }
    encode_auras(&mut abilities, now, &snapshot.auras)?;
    encode_auras(&mut abilities, now, &snapshot.target_detail.auras)?;
    Ok([inventory, quests, Some(loot), Some(abilities)])
}

fn encode_inventory_sheet(revision: u64, inventory: &Inventory, equipment: &Equipment) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MAX_INVENTORY_SHEET_BYTES);
    bytes.extend_from_slice(&revision.to_be_bytes());
    encode_inventory(&mut bytes, inventory);
    encode_equipment(&mut bytes, equipment);
    let totals = equipment.totals();
    for total in [
        totals.stamina,
        totals.strength,
        totals.agility,
        totals.intellect,
    ] {
        bytes.extend_from_slice(&total.to_be_bytes());
    }
    bytes
}

/// Timers travel as the low 16 bits of the tick they end, so a sheet stays unchanged while
/// they run down; every timer is shorter than 2^16 ticks.
const fn tick_bits(tick: u64) -> u16 {
    let bytes = tick.to_be_bytes();
    u16::from_be_bytes([bytes[6], bytes[7]])
}

fn encode_auras(payload: &mut Vec<u8>, now: u16, auras: &[AuraView]) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        auras.len(),
        MAX_AURAS,
        "player projection has too many auras",
    )?);
    for aura in auras {
        payload.extend_from_slice(&[aura.ability.get(), aura.kind.code()]);
        payload.extend_from_slice(&now.wrapping_add(aura.remaining).to_be_bytes());
        payload.extend_from_slice(&aura.amount.to_be_bytes());
    }
    Ok(())
}

/// Reads one sheet's bytes from a projection, validating them on the way.
pub(crate) fn read_sheet<'a>(
    sheet: Sheet,
    payload: &'a [u8],
    offset: &mut usize,
    tick: u64,
) -> Result<&'a [u8], ProtocolError> {
    let start = *offset;
    match sheet {
        Sheet::Inventory => {
            read_inventory_sheet(payload, offset)?;
        }
        Sheet::Quests => {
            crate::quest::decode_sheet(payload, offset)?;
        }
        Sheet::Loot => {
            read_loot_sheet(payload, offset)?;
        }
        Sheet::Abilities => {
            read_ability_sheet(payload, offset, tick, tick)?;
        }
    }
    Ok(&payload[start..*offset])
}

/// The bag sheet: inventory revision, bag and equipment.
pub(crate) fn decode_inventory_sheet(
    bytes: &[u8],
) -> Result<(u64, Inventory, Equipment), ProtocolError> {
    let mut offset = 0;
    let sheet = read_inventory_sheet(bytes, &mut offset)?;
    ensure_fully_consumed(bytes, offset)?;
    Ok(sheet)
}

fn read_inventory_sheet(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(u64, Inventory, Equipment), ProtocolError> {
    let revision = u64::from_be_bytes(take(payload, offset)?);
    if revision == 0 {
        return Err(ProtocolError::new("inventory revision must be nonzero"));
    }
    let inventory = decode_inventory(payload, offset)?;
    let equipment = decode_equipment(payload, offset)?;
    let totals = equipment.totals();
    let mut wire = [0_u16; 4];
    for total in &mut wire {
        *total = u16::from_be_bytes(take(payload, offset)?);
    }
    if wire
        != [
            totals.stamina,
            totals.strength,
            totals.agility,
            totals.intellect,
        ]
    {
        return Err(ProtocolError::new("stat totals do not match the equipment"));
    }
    Ok((revision, inventory, equipment))
}

pub(crate) fn decode_quest_sheet(bytes: &[u8]) -> Result<QuestSheet, ProtocolError> {
    let mut offset = 0;
    let sheet = crate::quest::decode_sheet(bytes, &mut offset)?;
    ensure_fully_consumed(bytes, offset)?;
    Ok(sheet)
}

pub(crate) fn decode_loot_sheet(bytes: &[u8]) -> Result<Option<LootView>, ProtocolError> {
    let mut offset = 0;
    let sheet = read_loot_sheet(bytes, &mut offset)?;
    ensure_fully_consumed(bytes, offset)?;
    Ok(sheet)
}

fn read_loot_sheet(payload: &[u8], offset: &mut usize) -> Result<Option<LootView>, ProtocolError> {
    if !read_bool(payload, offset)? {
        return Ok(None);
    }
    Ok(Some(LootView {
        claim: crate::loot::decode_claim(payload, offset)?,
        rewards: crate::loot::decode_rewards(payload, offset)?,
    }))
}

/// The ability sheet as of `tick`: timers received at `received` keep running down, and the
/// ones that ended are gone.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Abilities {
    /// The target the target auras belong to.
    pub(crate) target: Option<EntityRef>,
    pub(crate) cooldowns: Vec<Cooldown>,
    pub(crate) auras: Vec<AuraView>,
    pub(crate) target_auras: Vec<AuraView>,
}

pub(crate) fn decode_ability_sheet(
    bytes: &[u8],
    received: u64,
    tick: u64,
) -> Result<Abilities, ProtocolError> {
    let mut offset = 0;
    let sheet = read_ability_sheet(bytes, &mut offset, received, tick)?;
    ensure_fully_consumed(bytes, offset)?;
    Ok(sheet)
}

/// Ticks a timer ending at `end` still runs at `tick`, given it ran `0 < left <= limit` ticks
/// when received; `None` once it ended.
fn time_left(
    end: u16,
    received: u64,
    tick: u64,
    limit: u16,
    error: &'static str,
) -> Result<Option<u16>, ProtocolError> {
    let left = end.wrapping_sub(tick_bits(received));
    if left == 0 || left > limit {
        return Err(ProtocolError::new(error));
    }
    let elapsed = tick.saturating_sub(received);
    Ok(u64::from(left)
        .checked_sub(elapsed)
        .filter(|left| *left > 0)
        .and_then(|left| u16::try_from(left).ok()))
}

fn read_ability_sheet(
    payload: &[u8],
    offset: &mut usize,
    received: u64,
    tick: u64,
) -> Result<Abilities, ProtocolError> {
    let target = decode_entity_ref(payload, offset)?;
    let count = decode_u8_count(
        payload,
        offset,
        MAX_COOLDOWNS,
        "snapshot has too many cooldowns",
    )?;
    let mut cooldowns = Vec::with_capacity(count);
    let mut last: Option<AbilityId> = None;
    for _ in 0..count {
        let ability = AbilityId::new(read_u8(payload, offset)?);
        let end = u16::from_be_bytes(take(payload, offset)?);
        let catalog = ability_by_id(ability)
            .filter(|_| last.is_none_or(|last| last < ability))
            .ok_or_else(|| ProtocolError::new("cooldown record is inconsistent"))?;
        last = Some(ability);
        if let Some(remaining) = time_left(
            end,
            received,
            tick,
            catalog.cooldown,
            "cooldown record is inconsistent",
        )? {
            cooldowns.push(Cooldown { ability, remaining });
        }
    }
    let auras = read_auras(payload, offset, received, tick)?;
    let target_auras = read_auras(payload, offset, received, tick)?;
    if target.is_none() && !target_auras.is_empty() {
        return Err(ProtocolError::new("target detail needs a target"));
    }
    Ok(Abilities {
        target,
        cooldowns,
        auras,
        target_auras,
    })
}

/// Auras name catalog abilities with their aura kind and time left.
fn read_auras(
    payload: &[u8],
    offset: &mut usize,
    received: u64,
    tick: u64,
) -> Result<Vec<AuraView>, ProtocolError> {
    let count = decode_u8_count(payload, offset, MAX_AURAS, "snapshot has too many auras")?;
    let mut auras = Vec::with_capacity(count);
    for _ in 0..count {
        let ability = AbilityId::new(read_u8(payload, offset)?);
        let kind = AuraKind::from_code(read_u8(payload, offset)?);
        let end = u16::from_be_bytes(take(payload, offset)?);
        let amount = u16::from_be_bytes(take(payload, offset)?);
        let spec = ability_by_id(ability).and_then(|catalog| catalog.aura());
        let (Some(spec), Some(kind)) = (spec, kind) else {
            return Err(ProtocolError::new("aura record is inconsistent"));
        };
        if spec.kind != kind {
            return Err(ProtocolError::new("aura record is inconsistent"));
        }
        if let Some(remaining) = time_left(
            end,
            received,
            tick,
            spec.duration,
            "aura record is inconsistent",
        )? {
            auras.push(AuraView {
                ability,
                kind,
                remaining,
                amount,
            });
        }
    }
    Ok(auras)
}

/// `command` (plain or observed bytes from [`crate::encode_command`] or
/// [`crate::encode_observed_command`]) with the sheet revisions the client holds: the protocol
/// byte, the acknowledgement tag, four `u32` revisions, then the command's tag and fields.
#[must_use]
pub fn encode_acknowledged_command(revisions: SheetRevisions, command: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + SHEET_REVISIONS_BYTES + command.len());
    payload.extend_from_slice(&[
        crate::command::PROTOCOL_BYTE,
        crate::command::ACKNOWLEDGED_TAG,
    ]);
    revisions.encode(&mut payload);
    payload.extend_from_slice(command.get(1..).unwrap_or_default());
    payload
}
