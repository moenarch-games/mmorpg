//! Corpse payoffs (#935): a creature that dies while a Necromancer's Corruption rests on it
//! leaves that Necromancer one fresh corpse to spend.
//!
//! - **Marking** (at the death): every living corruptor who knows Raise Dead or Corpse Burst
//!   gets a corpse mark, an [`AuraKind::Corpse`] aura of Raise Dead whose caster is the corpse.
//!   It lasts [`crate::CORPSE_WINDOW_TICKS`]; a newer corpse replaces an older mark, so a
//!   Necromancer holds at most one.
//! - **Spending**: Raise Dead raises one timed helper on the minion system; Corpse Burst bursts
//!   the corpse where it lies. Either needs the mark and the corpse within
//!   [`CORPSE_SPEND_REACH_UNITS`]; Raise Dead is refused while [`crate::RAISE_DEAD_LIMIT`]
//!   helpers stand, and the corpse stays. Spending removes every mark naming that corpse, so no
//!   one spends it twice.
//! - The corpse itself stays: its life, tap and loot are untouched, so its rewards are claimed
//!   once as before.

use std::collections::BTreeSet;

use crate::ability::{AbilityEffect, CORPSE_WINDOW_TICKS, ids, learned};
use crate::creature::Life;
use crate::{
    Ability, Aura, AuraKind, CreatureId, EntityRef, ErrorCode, MAX_MINIONS, PlayerId,
    RAISE_DEAD_LIMIT, ZoneError, ZoneEvent, ZoneSimulation,
};

/// A Necromancer spends a corpse within this distance (30 m).
pub const CORPSE_SPEND_REACH_UNITS: i32 = 3_000;

impl ZoneSimulation {
    /// The living Necromancers whose Corruption rests on `creature_id` and who know a corpse
    /// payoff; read before its death clears its auras.
    pub(crate) fn corruptors(&self, creature_id: CreatureId) -> BTreeSet<PlayerId> {
        let Some(creature) = self.creatures.get(&creature_id) else {
            return BTreeSet::new();
        };
        creature
            .auras
            .iter()
            .filter(|aura| aura.ability == ids::CORRUPTION)
            .filter_map(|aura| match aura.caster {
                EntityRef::Player(player_id) => Some(player_id),
                _ => None,
            })
            .filter(|player_id| {
                self.players.get(player_id).is_some_and(|player| {
                    player.is_alive()
                        && player.class.is_some_and(|choice| {
                            [ids::RAISE_DEAD, ids::CORPSE_BURST]
                                .into_iter()
                                .any(|id| learned(choice.class, player.level, id).is_some())
                        })
                })
            })
            .collect()
    }

    /// Gives each of `corruptors` the fresh corpse of `creature_id`, replacing an older mark.
    pub(crate) fn mark_corpse(&mut self, creature_id: CreatureId, corruptors: BTreeSet<PlayerId>) {
        for player_id in corruptors {
            self.remove_corpse_marks(|_| true, Some(player_id));
            self.put_aura(
                EntityRef::Player(player_id),
                Aura {
                    ability: ids::RAISE_DEAD,
                    caster: EntityRef::Creature(creature_id),
                    remaining: CORPSE_WINDOW_TICKS,
                    amount: 0,
                },
            );
        }
    }

    /// The corpse `player_id` may spend with `ability`, or why not.
    pub(crate) fn corpse_to_spend(
        &self,
        player_id: PlayerId,
        ability: &Ability,
    ) -> Result<Result<EntityRef, ErrorCode>, ZoneError> {
        let marked = self.players.get(&player_id).and_then(|player| {
            player.auras.iter().find_map(|aura| match aura.caster {
                EntityRef::Creature(id) if aura.kind() == Some(AuraKind::Corpse) => Some(id),
                _ => None,
            })
        });
        let Some((corpse, position)) = marked.and_then(|id| match self.creatures.get(&id)?.life {
            Life::Corpse { position, .. } => Some((id, position)),
            Life::Alive | Life::Despawned { .. } => None,
        }) else {
            return Ok(Err(ErrorCode::NoTarget));
        };
        let from = self.player_position(player_id)?;
        if !crate::reach::within_sphere(
            [from.x, from.y, from.z],
            position,
            CORPSE_SPEND_REACH_UNITS,
        ) {
            return Ok(Err(ErrorCode::OutOfRange));
        }
        if matches!(ability.effect, AbilityEffect::RaiseDead { .. })
            && (self.minions_by(player_id, ability.id).len() >= RAISE_DEAD_LIMIT
                || self.minions.len() >= MAX_MINIONS)
        {
            return Ok(Err(ErrorCode::NotReady));
        }
        Ok(Ok(EntityRef::Creature(corpse)))
    }

    /// Spends `corpse` and returns where it lies: every mark naming it goes. `None` when it
    /// is no corpse any more.
    pub(crate) fn spend_corpse(&mut self, corpse: CreatureId) -> Option<[i32; 3]> {
        let Life::Corpse { position, .. } = self.creatures.get(&corpse)?.life else {
            return None;
        };
        let entity = EntityRef::Creature(corpse);
        self.remove_corpse_marks(|aura| aura.caster == entity, None);
        Some(position)
    }

    /// Removes the corpse marks `matches` selects, of `only` or of every player, reporting each.
    fn remove_corpse_marks(&mut self, matches: impl Fn(&Aura) -> bool, only: Option<PlayerId>) {
        let mut removed = Vec::new();
        for (&player_id, player) in &mut self.players {
            if only.is_some_and(|only| only != player_id) {
                continue;
            }
            player.auras.retain(|aura| {
                let spent = aura.kind() == Some(AuraKind::Corpse) && matches(aura);
                if spent {
                    removed.push((player_id, *aura));
                }
                !spent
            });
        }
        for (player_id, aura) in removed {
            let target = EntityRef::Player(player_id);
            self.notify_units(
                [Some(aura.caster), Some(target)],
                ZoneEvent::AuraRemoved {
                    source: aura.caster,
                    target,
                    ability: aura.ability,
                },
            );
        }
    }
}
