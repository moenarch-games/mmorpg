//! The unit records of a player projection (#1088): each layout is a [`record!`] and each game
//! type's [`Codec`] encodes and decodes it side by side.

use mmorpg_core::{AbilityId, CastView, EntityFlags, EntityKind, EntitySnapshot, ability_by_id};

use crate::ProtocolError;
use crate::codec::{Codec, Field, record};
use crate::wire::{decode_entity_kind, entity_kind_code, flag_byte, read_flags};

record! {
    /// A cast: ability ID (`u32`, 0 for none), flags (bit 0 channel), elapsed and total ticks.
    pub(crate) struct CastRecord {
        ability: u32,
        flags: u8,
        elapsed: u16,
        total: u16,
    }
}

/// A cast must name a casting catalog ability with its exact cast time and channel flag, and be
/// in progress; no cast is all zeros.
impl Codec for Option<CastView> {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        self.map_or_else(CastRecord::default, |cast| CastRecord {
            ability: cast.ability.get(),
            flags: u8::from(cast.channel),
            elapsed: cast.elapsed,
            total: cast.total,
        })
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let CastRecord {
            ability,
            flags,
            elapsed,
            total,
        } = CastRecord::read(payload, offset)?;
        let [channel] = read_flags(flags)?;
        if ability == 0 {
            if channel || elapsed != 0 || total != 0 {
                return Err(ProtocolError::new("cast state is inconsistent"));
            }
            return Ok(None);
        }
        let ability = AbilityId::new(ability);
        let valid = ability_by_id(ability).is_some_and(|catalog| {
            catalog.cast.ticks() == total && catalog.cast.is_channel() == channel && elapsed < total
        });
        if !valid {
            return Err(ProtocolError::new("cast state is inconsistent"));
        }
        Ok(Some(CastView {
            ability,
            elapsed,
            total,
            channel,
        }))
    }
}

record! {
    /// A projectile (#286): `u16` ID, kind, `3 × i32` position, `u16` yaw, `i16` pitch and state
    /// (0 flying, 1 stuck, 2 bursting, #290).
    pub(crate) struct ProjectileRecord {
        id: u16,
        kind: u8,
        position: [i32; 3],
        yaw: u16,
        pitch: i16,
        state: u8,
    }
}

impl Codec for mmorpg_core::ProjectileView {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        ProjectileRecord {
            id: self.id,
            kind: self.kind.code(),
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
            state: if self.burst { 2 } else { u8::from(self.stuck) },
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let record = ProjectileRecord::read(payload, offset)?;
        let (stuck, burst) = match record.state {
            0 => (false, false),
            1 => (true, false),
            2 => (false, true),
            _ => return Err(ProtocolError::new("unknown projectile state")),
        };
        Ok(Self {
            id: record.id,
            kind: mmorpg_core::ProjectileKind::from_code(record.kind)
                .ok_or_else(|| ProtocolError::new("unknown projectile kind"))?,
            position: record.position,
            yaw: record.yaw,
            pitch: record.pitch,
            stuck,
            burst,
        })
    }
}

record! {
    /// A piece of debris (#288): `u16` ID, prop kind, piece index, `3 × i32` position,
    /// `4 × i16` rotation quaternion and age.
    pub(crate) struct DebrisRecord {
        id: u16,
        kind: u8,
        piece: u8,
        position: [i32; 3],
        rotation: [i16; 4],
        age: u8,
    }
}

impl Codec for mmorpg_core::DebrisView {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        DebrisRecord {
            id: self.id,
            kind: self.kind.code(),
            piece: self.piece,
            position: self.position,
            rotation: self.rotation,
            age: self.age,
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let record = DebrisRecord::read(payload, offset)?;
        let kind = mmorpg_core::PropKind::from_code(record.kind)
            .ok_or_else(|| ProtocolError::new("unknown debris kind"))?;
        if usize::from(record.piece) >= mmorpg_core::debris_pieces(kind).len() {
            return Err(ProtocolError::new("unknown debris piece"));
        }
        Ok(Self {
            id: record.id,
            kind,
            piece: record.piece,
            position: record.position,
            rotation: record.rotation,
            age: record.age,
        })
    }
}

record! {
    /// A set-down load (#1077): its kind's code, with bit 7 set when the viewer set it down,
    /// `3 × i32` base and `u16` facing.
    pub(crate) struct LoadRecord {
        kind: u8,
        position: [i32; 3],
        facing: u16,
    }
}

/// The load record's kind byte: bit 7 says the viewer set it down (#1077).
const OWN_LOAD_BIT: u8 = 0x80;

impl Codec for mmorpg_core::LoadView {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        LoadRecord {
            kind: self.kind.code() | if self.own { OWN_LOAD_BIT } else { 0 },
            position: self.at,
            facing: self.facing,
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let record = LoadRecord::read(payload, offset)?;
        Ok(Self {
            kind: mmorpg_core::LoadKind::from_code(record.kind & !OWN_LOAD_BIT)
                .ok_or_else(|| ProtocolError::new("unknown load kind"))?,
            own: record.kind & OWN_LOAD_BIT != 0,
            at: record.position,
            facing: record.facing,
        })
    }
}

record! {
    /// A visible unit: kind, ID, `u32` appearance, `3 × i32` position, `3 × i8` velocity, `u16`
    /// facing, level, health percent, flags and the player's weapon (#1088).
    ///
    /// Only creatures are tapped, evade or are lootable, so on a player record those three flag
    /// bits carry its [`mmorpg_core::TraversalView`] code instead (#490, #516, #500): tapped is
    /// bit 0, evading bit 1 and lootable bit 2. On an NPC record they carry its posture code
    /// instead (#619, #991), and on a player record the health byte's high bit says it sits
    /// (#619).
    pub(crate) struct EntityRecord {
        kind: u8,
        id: u32,
        appearance: u32,
        position: [i32; 3],
        velocity: [i8; 3],
        facing: u16,
        level: u8,
        health: u8,
        flags: u8,
        weapon: u8,
    }
}

/// The health byte's high bit on a player record: the player sits (#619).
const SITTING_BIT: u8 = 0x80;
/// The appearance word's high bit on a player record: the weapon is drawn (#802).
const DRAWN_BIT: u32 = 0x8000_0000;
/// The appearance word's bits 24–26 on a player record: 1 + its great weapon's code, 0 for
/// none (#1073).
const GREAT_SHIFT: u32 = 24;
const GREAT_MASK: u32 = 0x0700_0000;
/// The appearance word's bits 27–30 on a player record: 1 + its named weapon's code, 0 for none
/// (#918).
const NAMED_SHIFT: u32 = 27;
const NAMED_MASK: u32 = 0x7800_0000;
/// The appearance word's bit 23 on a player record: the player is in wolf form (#957).
const SHIFTED_BIT: u32 = 0x0080_0000;
/// The appearance bits a player record lends to its weapon and form.
const PLAYER_ONLY_BITS: u32 = DRAWN_BIT | GREAT_MASK | NAMED_MASK | SHIFTED_BIT;
/// The weapon byte's low nibble on a player record: 1 + its weapon family's code, 0 for none
/// (#1088); the high nibble: 1 + the code of the load it carries, 0 for none (#1077).
const WEAPON_MASK: u8 = 0x0F;
const LOAD_SHIFT: u8 = 4;
/// Where the weapon byte sits in an entity record: last.
#[cfg(test)]
const WEAPON_OFFSET: usize = EntityRecord::BYTES - 1;

/// `1 + code` of an optional code, 0 for none.
fn optional_code(code: Option<u8>) -> u32 {
    code.map_or(0, |code| u32::from(code) + 1)
}

/// The value `1 + code` names, `None` for 0.
fn from_optional_code<T>(
    code: u32,
    from_code: impl FnOnce(u8) -> Option<T>,
    message: &str,
) -> Result<Option<T>, ProtocolError> {
    match code {
        0 => Ok(None),
        code => u8::try_from(code - 1)
            .ok()
            .and_then(from_code)
            .map(Some)
            .ok_or_else(|| ProtocolError::new(message)),
    }
}

impl Codec for EntitySnapshot {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        let entity = self;
        let flags = entity.flags;
        let player = entity.kind == EntityKind::Player;
        if (player && entity.appearance & PLAYER_ONLY_BITS != 0)
            || (!player
                && (flags.weapon_drawn
                    || flags.weapon.is_some()
                    || flags.named_weapon.is_some()
                    || flags.great_weapon.is_some()
                    || flags.load.is_some()
                    || flags.shapeshifted))
        {
            return Err(ProtocolError::new("invalid drawn weapon appearance"));
        }
        // The player-only high bits are free in the appearance word. Threat remains its own
        // flag; NPC and creature appearance IDs are untouched.
        let appearance = entity.appearance
            | optional_code(flags.great_weapon.map(mmorpg_core::GreatWeapon::code)) << GREAT_SHIFT
            | optional_code(flags.named_weapon.map(mmorpg_core::NamedWeapon::code)) << NAMED_SHIFT
            | if flags.shapeshifted { SHIFTED_BIT } else { 0 }
            | if flags.weapon_drawn { DRAWN_BIT } else { 0 };
        let weapon = u8::try_from(optional_code(
            flags.weapon.map(mmorpg_core::WeaponFamily::code),
        ))
        .ok()
        .filter(|weapon| *weapon <= WEAPON_MASK)
        .ok_or_else(|| ProtocolError::new("invalid weapon family"))?;
        // A carried load shares the weapon byte (#1077), so a projection does not grow.
        let load = u8::try_from(optional_code(flags.load.map(mmorpg_core::LoadKind::code)))
            .ok()
            .filter(|load| *load <= WEAPON_MASK)
            .ok_or_else(|| ProtocolError::new("invalid load"))?;
        let weapon = weapon | load << LOAD_SHIFT;
        if entity.health_percent > 100 {
            return Err(ProtocolError::new("health percent exceeds 100"));
        }
        let posture = flags.posture;
        match entity.kind {
            EntityKind::Player
                if !matches!(
                    posture,
                    mmorpg_core::PostureView::Free | mmorpg_core::PostureView::Sitting
                ) =>
            {
                return Err(ProtocolError::new("a player is never in bed"));
            }
            EntityKind::Player | EntityKind::Npc => {}
            _ if posture != mmorpg_core::PostureView::Free => {
                return Err(ProtocolError::new("only players and npcs sit"));
            }
            _ => {}
        }
        let sitting_player = player && posture == mmorpg_core::PostureView::Sitting;
        let (tapped, evading, lootable) = if entity.kind == EntityKind::Npc {
            if flags.tapped_by_other || flags.evading || flags.lootable {
                return Err(ProtocolError::new(
                    "an npc is never tapped, evading or lootable",
                ));
            }
            let code = posture.code();
            (code & 1 != 0, code & 2 != 0, code & 4 != 0)
        } else if player {
            if flags.tapped_by_other || flags.evading || flags.lootable {
                return Err(ProtocolError::new(
                    "a player is never tapped, evading or lootable",
                ));
            }
            let code = flags.traversal.code();
            (code & 1 != 0, code & 2 != 0, code & 4 != 0)
        } else {
            if flags.traversal != mmorpg_core::TraversalView::Free {
                return Err(ProtocolError::new("only a player climbs, mantles or rolls"));
            }
            (flags.tapped_by_other, flags.evading, flags.lootable)
        };
        EntityRecord {
            kind: entity_kind_code(entity.kind),
            id: entity.id,
            appearance,
            position: entity.position,
            velocity: entity.velocity,
            facing: entity.facing,
            level: entity.level,
            // The percent needs seven bits; the eighth says a player sits (#619).
            health: entity.health_percent | if sitting_player { SITTING_BIT } else { 0 },
            flags: flag_byte(&[
                flags.dead,
                flags.in_combat,
                flags.hostile,
                flags.attackable,
                tapped,
                evading,
                flags.targets_viewer,
                lootable,
            ]),
            weapon,
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let record = EntityRecord::read(payload, offset)?;
        let kind = decode_entity_kind(record.kind)?;
        let player = kind == EntityKind::Player;
        let encoded_appearance = record.appearance;
        if !player && record.weapon != 0 {
            return Err(ProtocolError::new("only a player record carries a weapon"));
        }
        let (weapon_drawn, shapeshifted, weapon, named_weapon, great_weapon, appearance) = if player
        {
            (
                encoded_appearance & DRAWN_BIT != 0,
                encoded_appearance & SHIFTED_BIT != 0,
                from_optional_code(
                    u32::from(record.weapon & WEAPON_MASK),
                    mmorpg_core::WeaponFamily::from_code,
                    "invalid weapon family",
                )?,
                from_optional_code(
                    (encoded_appearance & NAMED_MASK) >> NAMED_SHIFT,
                    mmorpg_core::NamedWeapon::from_code,
                    "invalid named weapon",
                )?,
                from_optional_code(
                    (encoded_appearance & GREAT_MASK) >> GREAT_SHIFT,
                    mmorpg_core::GreatWeapon::from_code,
                    "invalid great weapon",
                )?,
                encoded_appearance & !PLAYER_ONLY_BITS,
            )
        } else {
            (false, false, None, None, None, encoded_appearance)
        };
        let load = if player {
            from_optional_code(
                u32::from(record.weapon >> LOAD_SHIFT),
                mmorpg_core::LoadKind::from_code,
                "invalid load",
            )?
        } else {
            None
        };
        let sitting_player = record.health & SITTING_BIT != 0;
        let health_percent = record.health & !SITTING_BIT;
        if sitting_player && !player {
            return Err(ProtocolError::new(
                "only a player record carries the sit bit",
            ));
        }
        if health_percent > 100 {
            return Err(ProtocolError::new("health percent exceeds 100"));
        }
        let [
            dead,
            in_combat,
            hostile,
            attackable,
            tapped_by_other,
            evading,
            targets_viewer,
            lootable,
        ] = read_flags(record.flags)?;
        // An NPC is never tapped, evading or lootable: those bits carry its posture code (#619,
        // #991).
        let npc_posture = (kind == EntityKind::Npc).then(|| {
            mmorpg_core::PostureView::from_code(
                u8::from(tapped_by_other) | u8::from(evading) << 1 | u8::from(lootable) << 2,
            )
        });
        let posture = match npc_posture {
            Some(Some(posture)) => posture,
            Some(None) => return Err(ProtocolError::new("npc posture code is out of range")),
            None if sitting_player => mmorpg_core::PostureView::Sitting,
            None => mmorpg_core::PostureView::Free,
        };
        if dead && posture != mmorpg_core::PostureView::Free {
            return Err(ProtocolError::new("a dead unit does not sit"));
        }
        let (tapped_by_other, evading, lootable) = if kind == EntityKind::Npc {
            (false, false, false)
        } else {
            (tapped_by_other, evading, lootable)
        };
        // On a player record the tapped, evading and lootable bits carry its traversal code
        // (#490, #516, #500).
        let (tapped_by_other, evading, lootable, traversal) = if player {
            let code = u8::from(tapped_by_other) | u8::from(evading) << 1 | u8::from(lootable) << 2;
            let traversal = mmorpg_core::TraversalView::from_code(code)
                .ok_or_else(|| ProtocolError::new("player traversal code is out of range"))?;
            if dead && traversal != mmorpg_core::TraversalView::Free {
                return Err(ProtocolError::new(
                    "a dead player cannot climb, mantle or roll",
                ));
            }
            (false, false, false, traversal)
        } else {
            if lootable
                && (kind != EntityKind::Creature
                    || !dead
                    || health_percent != 0
                    || attackable
                    || tapped_by_other)
            {
                return Err(ProtocolError::new("lootable entity flags are inconsistent"));
            }
            (
                tapped_by_other,
                evading,
                lootable,
                mmorpg_core::TraversalView::Free,
            )
        };
        Ok(Self {
            kind,
            id: record.id,
            appearance,
            position: record.position,
            velocity: record.velocity,
            facing: record.facing,
            level: record.level,
            health_percent,
            flags: EntityFlags {
                dead,
                in_combat,
                weapon_drawn,
                weapon,
                named_weapon,
                great_weapon,
                shapeshifted,
                load,
                hostile,
                attackable,
                tapped_by_other,
                evading,
                targets_viewer,
                lootable,
                traversal,
                posture,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use mmorpg_core::{
        AbilityId, CastView, DebrisView, EntityFlags, EntityKind, EntitySnapshot, PostureView,
        ProjectileView, TraversalView, ability_by_id,
    };

    use super::{
        CastRecord, DebrisRecord, EntityRecord, LoadRecord, PLAYER_ONLY_BITS, ProjectileRecord,
        WEAPON_OFFSET,
    };
    use crate::codec::Codec;
    use crate::codec::generate::{CASES, Gen, round_trip};

    /// Every value a `from_code` names.
    pub(crate) fn all<T>(from_code: impl Fn(u8) -> Option<T>) -> Vec<T> {
        (0..=u8::MAX).filter_map(from_code).collect()
    }

    fn maybe<T: Copy>(rng: &mut Gen, choices: &[T]) -> Option<T> {
        rng.bool().then(|| rng.pick(choices))
    }

    #[test]
    fn casts_round_trip() {
        let casting = (1..=u32::from(u8::MAX))
            .map(AbilityId::new)
            .filter(|&ability| ability_by_id(ability).is_some_and(|spec| spec.cast.ticks() > 0))
            .collect::<Vec<_>>();
        assert!(!casting.is_empty());
        let mut rng = Gen::new(1);
        for _ in 0..CASES {
            let cast = maybe(&mut rng, &casting).map(|ability| {
                let spec = ability_by_id(ability).unwrap();
                let total = spec.cast.ticks();
                CastView {
                    ability,
                    elapsed: u16::try_from(rng.below(u64::from(total))).unwrap(),
                    total,
                    channel: spec.cast.is_channel(),
                }
            });
            round_trip(&cast, Some(CastRecord::BYTES));
        }
    }

    #[test]
    fn projectiles_round_trip() {
        let kinds = all(mmorpg_core::ProjectileKind::from_code);
        let mut rng = Gen::new(2);
        for _ in 0..CASES {
            let state = rng.below(3);
            let projectile = ProjectileView {
                id: rng.int(),
                kind: rng.pick(&kinds),
                position: [rng.int(), rng.int(), rng.int()],
                yaw: rng.int(),
                pitch: rng.int(),
                stuck: state == 1,
                burst: state == 2,
            };
            round_trip(&projectile, Some(ProjectileRecord::BYTES));
        }
    }

    #[test]
    fn debris_round_trips() {
        let kinds = all(mmorpg_core::PropKind::from_code)
            .into_iter()
            .filter(|&kind| !mmorpg_core::debris_pieces(kind).is_empty())
            .collect::<Vec<_>>();
        let mut rng = Gen::new(3);
        for _ in 0..CASES {
            let kind = rng.pick(&kinds);
            let pieces = mmorpg_core::debris_pieces(kind).len() as u64;
            let piece = DebrisView {
                id: rng.int(),
                kind,
                piece: u8::try_from(rng.below(pieces)).unwrap(),
                position: [rng.int(), rng.int(), rng.int()],
                rotation: [rng.int(), rng.int(), rng.int(), rng.int()],
                age: rng.int(),
            };
            round_trip(&piece, Some(DebrisRecord::BYTES));
        }
    }

    #[test]
    fn set_down_loads_round_trip() {
        let kinds = all(mmorpg_core::LoadKind::from_code);
        let mut rng = Gen::new(7);
        for _ in 0..CASES {
            let load = mmorpg_core::LoadView {
                kind: rng.pick(&kinds),
                own: rng.bool(),
                at: [rng.int(), rng.int(), rng.int()],
                facing: rng.int(),
            };
            round_trip(&load, Some(LoadRecord::BYTES));
        }
    }

    /// Any entity a projection can carry: each kind with only the flags its record has room for.
    pub(crate) fn entity(rng: &mut Gen) -> EntitySnapshot {
        let kind = rng.pick(&[
            EntityKind::Player,
            EntityKind::Creature,
            EntityKind::Npc,
            EntityKind::Minion,
        ]);
        let dead = rng.bool();
        let health_percent = if dead {
            0
        } else {
            u8::try_from(rng.below(101)).unwrap()
        };
        let mut flags = EntityFlags {
            dead,
            in_combat: rng.bool(),
            hostile: rng.bool(),
            attackable: rng.bool(),
            targets_viewer: rng.bool(),
            ..EntityFlags::default()
        };
        let mut appearance = rng.int::<u32>();
        match kind {
            EntityKind::Player => {
                appearance &= !PLAYER_ONLY_BITS;
                flags.weapon_drawn = rng.bool();
                flags.shapeshifted = rng.bool();
                flags.weapon = maybe(rng, &all(mmorpg_core::WeaponFamily::from_code));
                flags.named_weapon = maybe(rng, &all(mmorpg_core::NamedWeapon::from_code));
                flags.great_weapon = maybe(rng, &all(mmorpg_core::GreatWeapon::from_code));
                if !dead {
                    flags.traversal = rng.pick(&all(TraversalView::from_code));
                    flags.posture = rng.pick(&[PostureView::Free, PostureView::Sitting]);
                }
                flags.load = maybe(rng, &all(mmorpg_core::LoadKind::from_code));
            }
            EntityKind::Npc => {
                if !dead {
                    flags.posture = rng.pick(&all(PostureView::from_code));
                }
            }
            EntityKind::Creature | EntityKind::Minion => {
                flags.tapped_by_other = rng.bool();
                flags.evading = rng.bool();
                flags.lootable = kind == EntityKind::Creature
                    && dead
                    && !flags.attackable
                    && !flags.tapped_by_other
                    && rng.bool();
            }
        }
        EntitySnapshot {
            kind,
            id: rng.int(),
            appearance,
            position: [rng.int(), rng.int(), rng.int()],
            velocity: [rng.int(), rng.int(), rng.int()],
            facing: rng.int(),
            level: rng.int(),
            health_percent,
            flags,
        }
    }

    #[test]
    fn entities_round_trip() {
        let mut rng = Gen::new(4);
        for _ in 0..CASES {
            round_trip(&entity(&mut rng), Some(EntityRecord::BYTES));
        }
    }

    /// The weapon family travels in its own byte (#1088), so the appearance word keeps its bits
    /// whatever family a player holds, and only a player record carries one.
    #[test]
    fn the_player_weapon_has_its_own_byte() {
        let mut rng = Gen::new(5);
        let mut player = entity(&mut rng);
        player.kind = EntityKind::Player;
        player.appearance = 0x0078_0003;
        player.flags = EntityFlags {
            weapon: Some(mmorpg_core::WeaponFamily::TwoHanded),
            ..EntityFlags::default()
        };
        let mut bytes = Vec::new();
        player.encode(&mut bytes).unwrap();
        assert_eq!(bytes[5..9], [0, 0x78, 0, 3]);
        assert_eq!(
            bytes[WEAPON_OFFSET],
            mmorpg_core::WeaponFamily::TwoHanded.code() + 1
        );
        assert_eq!(EntitySnapshot::decode(&bytes, &mut 0).unwrap(), player);
        bytes[WEAPON_OFFSET] = u8::MAX;
        assert_eq!(
            EntitySnapshot::decode(&bytes, &mut 0)
                .unwrap_err()
                .to_string(),
            "invalid weapon family"
        );
        let mut creature = player;
        creature.kind = EntityKind::Creature;
        creature.flags = EntityFlags::default();
        let mut bytes = Vec::new();
        creature.encode(&mut bytes).unwrap();
        assert_eq!(bytes[WEAPON_OFFSET], 0);
        bytes[WEAPON_OFFSET] = 1;
        assert_eq!(
            EntitySnapshot::decode(&bytes, &mut 0)
                .unwrap_err()
                .to_string(),
            "only a player record carries a weapon"
        );
    }

    /// A carried load (#1077) rides the weapon byte's high nibble beside the weapon family, so
    /// the record keeps its size; an unknown load fails closed.
    #[test]
    fn a_carried_load_shares_the_weapon_byte() {
        let mut rng = Gen::new(6);
        let mut player = entity(&mut rng);
        player.kind = EntityKind::Player;
        player.appearance = 3;
        player.flags = EntityFlags {
            weapon: Some(mmorpg_core::WeaponFamily::Sword),
            load: Some(mmorpg_core::LoadKind::WaterJug),
            ..EntityFlags::default()
        };
        let mut bytes = Vec::new();
        player.encode(&mut bytes).unwrap();
        assert_eq!(bytes.len(), EntityRecord::BYTES);
        assert_eq!(
            bytes[WEAPON_OFFSET],
            (mmorpg_core::LoadKind::WaterJug.code() + 1) << 4
                | (mmorpg_core::WeaponFamily::Sword.code() + 1)
        );
        assert_eq!(EntitySnapshot::decode(&bytes, &mut 0).unwrap(), player);
        bytes[WEAPON_OFFSET] = 0xF0;
        assert_eq!(
            EntitySnapshot::decode(&bytes, &mut 0)
                .unwrap_err()
                .to_string(),
            "invalid load"
        );
        let mut creature = player;
        creature.kind = EntityKind::Creature;
        creature.flags = EntityFlags {
            load: Some(mmorpg_core::LoadKind::Sack),
            ..EntityFlags::default()
        };
        assert!(creature.encode(&mut Vec::new()).is_err());
    }
}
