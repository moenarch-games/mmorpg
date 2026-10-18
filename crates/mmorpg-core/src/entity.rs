//! Unit identity: which kind of unit, which one, and which physics body.
//!
//! [`EntityRef`] orders players before creatures before NPCs, then by ID.
//! Every ordered pass over units (combat resolution, threat ties, projection
//! ties) uses this order. Each kind owns a disjoint physics body ID
//! namespace above the static collider IDs, which zone content keeps below
//! [`PLAYER_BODY_BASE`].

use crate::physics::BodyId;

use crate::PlayerId;

/// A creature spawn and the creature it hosts. A respawn keeps the ID.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CreatureId(u32);

impl CreatureId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A non-player character placed by zone content.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NpcId(u32);

impl NpcId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A creature template of zone content; clients name creatures by it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CreatureTemplateId(u16);

impl CreatureTemplateId {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Kind of a unit, in [`EntityRef`] order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityKind {
    Player,
    Creature,
    Npc,
}

/// Any unit in a zone: `kind` then `id` order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityRef {
    Player(PlayerId),
    Creature(CreatureId),
    Npc(NpcId),
}

impl EntityRef {
    #[must_use]
    pub const fn new(kind: EntityKind, id: u32) -> Self {
        match kind {
            EntityKind::Player => Self::Player(id),
            EntityKind::Creature => Self::Creature(CreatureId::new(id)),
            EntityKind::Npc => Self::Npc(NpcId::new(id)),
        }
    }

    #[must_use]
    pub const fn kind(self) -> EntityKind {
        match self {
            Self::Player(_) => EntityKind::Player,
            Self::Creature(_) => EntityKind::Creature,
            Self::Npc(_) => EntityKind::Npc,
        }
    }

    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::Player(id) => id,
            Self::Creature(id) => id.get(),
            Self::Npc(id) => id.get(),
        }
    }
}

/// Static collider IDs lie below this; zone content validates it.
pub(crate) const PLAYER_BODY_BASE: u64 = 1_000_000;
/// Six fixed world-limit bodies start above every player body ID.
pub(crate) const WORLD_LIMIT_BODY_BASE: u64 = PLAYER_BODY_BASE + (1 << 32);
/// Creature bodies start above the world-limit range.
const CREATURE_BODY_BASE: u64 = WORLD_LIMIT_BODY_BASE + (1 << 32);
/// NPC bodies start above every creature body ID.
const NPC_BODY_BASE: u64 = CREATURE_BODY_BASE + (1 << 32);

// Each namespace spans a full `u32` of IDs and the last one ends inside `u64`.
const _: () = assert!(WORLD_LIMIT_BODY_BASE - PLAYER_BODY_BASE > u32::MAX as u64);
const _: () = assert!(CREATURE_BODY_BASE - WORLD_LIMIT_BODY_BASE > u32::MAX as u64);
const _: () = assert!(NPC_BODY_BASE - CREATURE_BODY_BASE > u32::MAX as u64);
const _: () = assert!(NPC_BODY_BASE.checked_add(u32::MAX as u64).is_some());

/// Whether `body` belongs to a player, creature or NPC (not content or a world limit).
pub(crate) const fn is_unit_body(body: BodyId) -> bool {
    (body.0 >= PLAYER_BODY_BASE && body.0 < WORLD_LIMIT_BODY_BASE) || body.0 >= CREATURE_BODY_BASE
}

/// The unit owning `body`, if it is a unit body.
pub(crate) const fn entity_of(body: BodyId) -> Option<EntityRef> {
    let id = body.0;
    #[allow(clippy::cast_possible_truncation)]
    if id >= NPC_BODY_BASE {
        Some(EntityRef::Npc(NpcId::new((id - NPC_BODY_BASE) as u32)))
    } else if id >= CREATURE_BODY_BASE {
        Some(EntityRef::Creature(CreatureId::new(
            (id - CREATURE_BODY_BASE) as u32,
        )))
    } else if id >= PLAYER_BODY_BASE && id < WORLD_LIMIT_BODY_BASE {
        Some(EntityRef::Player((id - PLAYER_BODY_BASE) as u32))
    } else {
        None
    }
}

/// The physics body of a unit. Namespaces never overlap across kinds.
pub(crate) const fn body_id(entity: EntityRef) -> BodyId {
    match entity {
        EntityRef::Player(id) => BodyId(PLAYER_BODY_BASE + id as u64),
        EntityRef::Creature(id) => BodyId(CREATURE_BODY_BASE + id.get() as u64),
        EntityRef::Npc(id) => BodyId(NPC_BODY_BASE + id.get() as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_order_is_kind_then_id() {
        let mut entities = vec![
            EntityRef::Npc(NpcId::new(0)),
            EntityRef::Creature(CreatureId::new(9)),
            EntityRef::Player(u32::MAX),
            EntityRef::Creature(CreatureId::new(2)),
            EntityRef::Player(3),
        ];
        entities.sort();
        assert_eq!(
            entities,
            [
                EntityRef::Player(3),
                EntityRef::Player(u32::MAX),
                EntityRef::Creature(CreatureId::new(2)),
                EntityRef::Creature(CreatureId::new(9)),
                EntityRef::Npc(NpcId::new(0)),
            ]
        );
        for entity in entities {
            assert_eq!(EntityRef::new(entity.kind(), entity.id()), entity);
        }
    }

    #[test]
    fn body_namespaces_are_disjoint_per_kind() {
        let ranges = [
            (0, PLAYER_BODY_BASE - 1),
            (
                body_id(EntityRef::Player(0)).0,
                body_id(EntityRef::Player(u32::MAX)).0,
            ),
            (WORLD_LIMIT_BODY_BASE, WORLD_LIMIT_BODY_BASE + 5),
            (
                body_id(EntityRef::Creature(CreatureId::new(0))).0,
                body_id(EntityRef::Creature(CreatureId::new(u32::MAX))).0,
            ),
            (
                body_id(EntityRef::Npc(NpcId::new(0))).0,
                body_id(EntityRef::Npc(NpcId::new(u32::MAX))).0,
            ),
        ];
        for pair in ranges.windows(2) {
            assert!(pair[0].1 < pair[1].0, "{pair:?}");
        }
    }
}
