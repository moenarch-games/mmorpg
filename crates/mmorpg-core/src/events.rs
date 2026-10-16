//! Player-visible gameplay feedback.
//!
//! Each player owns a bounded event queue. A tick clears every queue before
//! it resolves anything, then appends the events it produces, so the events
//! of tick N stay in zone state (and in canonical snapshots) until tick N + 1
//! starts, and a recovered zone projects exactly the same events. Events are
//! cosmetic: losing a projection may lose them, and durable facts such as
//! health live in state that every projection repeats.

use crate::EntityRef;
use crate::ability::AbilityId;

/// At most this many events per player and tick.
pub const MAX_EVENTS_PER_PLAYER: usize = 16;

/// Why a well-formed intent was not carried out. Gameplay refusals are
/// outcomes, never session errors.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ErrorCode {
    /// Attacking without a selected target.
    NoTarget,
    /// The target is beyond melee reach when a swing is due.
    OutOfRange,
    /// The target is dead.
    TargetDead,
    /// The target is a player or an NPC.
    NotAttackable,
    /// Dead players can only release their spirit.
    YouAreDead,
    /// Releasing the spirit while alive.
    NotDead,
    /// The selected unit does not exist or is not visible.
    InvalidTarget,
    /// More discrete intents arrived between two ticks than a player may
    /// queue; the excess was dropped.
    TooManyIntents,
    /// Invalid bag slot, quantity, source or partial swap.
    InvalidInventoryMove,
    /// The complete requested quantity cannot fit the destination stack.
    InventoryFull,
    /// The corpse death is stale, absent or expired.
    InvalidLoot,
    /// The viewer did not tap this death.
    NotLootOwner,
    /// No rewards remain.
    EmptyLoot,
    /// Crediting rewards would overflow the copper balance.
    MoneyOverflow,
    /// Abilities need a chosen class.
    NoClass,
    /// The class has not learned this ability (or it is not a class ability).
    NotLearned,
    /// The global cooldown or the ability's cooldown is running.
    NotReady,
    /// The resource does not cover the ability's cost.
    NotEnoughResource,
    /// Stunned units cannot act.
    Stunned,
    /// A cast or channel is already in progress.
    AlreadyCasting,
    /// The class choice is unknown or the class was already chosen.
    InvalidClass,
    /// The bag item has no equipment slot.
    NotEquippable,
    /// The NPC is not a vendor, or the vendor has no such offer.
    InvalidVendor,
    /// The copper balance does not cover the purchase.
    NotEnoughMoney,
    /// The speaker spoke within the last second.
    ChatThrottled,
    /// The quest is unknown, not offered or ended by this NPC, not
    /// available (already active, turned in or locked by its
    /// prerequisite), not in the log, or the reward choice is invalid.
    InvalidQuest,
    /// The quest log already holds ten quests.
    QuestLogFull,
    /// Not every objective of the quest is complete.
    QuestIncomplete,
}

/// One feedback event addressed to a player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneEvent {
    /// The player's attack hit `target`.
    DamageDealt {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
        critical: bool,
    },
    /// `source` hit the player.
    DamageTaken {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
        critical: bool,
    },
    /// An attack between `source` and `target` missed; one of them is the player.
    Miss {
        source: EntityRef,
        target: EntityRef,
    },
    /// `entity` died, killed by `killer` when known.
    Died {
        entity: EntityRef,
        killer: Option<EntityRef>,
    },
    /// `target` is evading and ignored the player's attack.
    Evade {
        source: EntityRef,
        target: EntityRef,
    },
    /// An intent was refused; `target` is the unit it concerned, if any.
    Error {
        code: ErrorCode,
        target: Option<EntityRef>,
    },
    /// `source` began casting or channelling `ability` for `ticks` ticks.
    CastStarted {
        source: EntityRef,
        target: Option<EntityRef>,
        ability: AbilityId,
        ticks: u16,
    },
    /// `source` resolved `ability`, at `target` unless it is self-centred.
    AbilityUsed {
        source: EntityRef,
        target: Option<EntityRef>,
        ability: AbilityId,
    },
    /// `ability` of `source` healed `target` by `amount` effective health.
    Healed {
        source: EntityRef,
        target: EntityRef,
        ability: AbilityId,
        amount: u16,
    },
    /// `source` put the aura of `ability` on `target` for `ticks` ticks.
    AuraApplied {
        source: EntityRef,
        target: EntityRef,
        ability: AbilityId,
        ticks: u16,
    },
    /// The aura of `ability` that `source` put on `target` ended.
    AuraRemoved {
        source: EntityRef,
        target: EntityRef,
        ability: AbilityId,
    },
    /// `target`'s cast or channel of `ability` stopped early, by `source`
    /// when another unit interrupted it.
    Interrupted {
        source: Option<EntityRef>,
        target: EntityRef,
        ability: AbilityId,
    },
    /// A shield on `target` absorbed `amount` of `source`'s damage.
    Absorbed {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
    },
    /// Objective `objective` (its index) of `quest` reached `count`.
    QuestProgress {
        quest: crate::QuestId,
        objective: u8,
        count: u8,
    },
    /// The player turned `quest` in.
    QuestCompleted { quest: crate::QuestId },
}

impl ZoneEvent {
    /// Overflow keeps higher priorities: deaths, then damage taken, errors
    /// and turn-ins, damage dealt and quest progress, ability feedback, and
    /// finally misses and evades.
    const fn priority(self) -> u8 {
        match self {
            Self::Died { .. } => 5,
            Self::DamageTaken { .. } => 4,
            Self::Error { .. } | Self::QuestCompleted { .. } => 3,
            Self::DamageDealt { .. } | Self::QuestProgress { .. } => 2,
            Self::CastStarted { .. }
            | Self::AbilityUsed { .. }
            | Self::Healed { .. }
            | Self::AuraApplied { .. }
            | Self::AuraRemoved { .. }
            | Self::Interrupted { .. }
            | Self::Absorbed { .. } => 1,
            Self::Miss { .. } | Self::Evade { .. } => 0,
        }
    }
}

/// Appends `event` to a bounded queue. When the queue is full, the event
/// replaces the latest queued event of the lowest priority if that priority
/// is strictly lower; otherwise the new event is dropped. Both choices depend
/// only on queue contents, so overflow is deterministic.
pub(crate) fn push_event(queue: &mut Vec<ZoneEvent>, event: ZoneEvent) {
    if queue.len() < MAX_EVENTS_PER_PLAYER {
        queue.push(event);
        return;
    }
    let lowest = queue
        .iter()
        .enumerate()
        .min_by(|(left_index, left), (right_index, right)| {
            // Lowest priority first; among equals, the latest one.
            left.priority()
                .cmp(&right.priority())
                .then(right_index.cmp(left_index))
        })
        .map(|(index, queued)| (index, queued.priority()));
    if let Some((index, priority)) = lowest
        && event.priority() > priority
    {
        queue.remove(index);
        queue.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CreatureId;

    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(4));
    const PLAYER: EntityRef = EntityRef::Player(1);

    fn miss(index: u16) -> ZoneEvent {
        if index.is_multiple_of(2) {
            ZoneEvent::Miss {
                source: PLAYER,
                target: WOLF,
            }
        } else {
            ZoneEvent::Evade {
                source: PLAYER,
                target: WOLF,
            }
        }
    }

    #[test]
    fn overflow_drops_the_latest_lowest_priority_event() {
        let mut queue = Vec::new();
        for index in 0..16 {
            push_event(&mut queue, miss(index));
        }
        let died = ZoneEvent::Died {
            entity: WOLF,
            killer: Some(PLAYER),
        };
        push_event(&mut queue, died);
        assert_eq!(queue.len(), MAX_EVENTS_PER_PLAYER);
        assert_eq!(queue[..15], (0..15).map(miss).collect::<Vec<_>>()[..]);
        assert_eq!(queue[15], died);

        // An event no more important than every queued one is dropped.
        let before = queue.clone();
        push_event(&mut queue, miss(0));
        assert_eq!(queue, before);

        let dealt = ZoneEvent::DamageDealt {
            source: PLAYER,
            target: WOLF,
            amount: 4,
            critical: false,
        };
        push_event(&mut queue, dealt);
        assert_eq!(queue[13], miss(13), "miss 14 made room");
        assert_eq!(queue[14..], [died, dealt]);
    }
}
