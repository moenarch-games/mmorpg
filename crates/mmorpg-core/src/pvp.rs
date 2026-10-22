//! Mode-neutral opt-in player combat. Arena hosts own admission; core owns outcomes.
use crate::{EntityRef, PlayerCombatPolicy, PlayerId, ZoneError, ZoneSimulation};

impl ZoneSimulation {
    /// Whether two distinct currently admitted players belong to an explicitly hostile zone.
    /// Self-effects remain governed by their abilities. This does not create a duel.
    #[must_use]
    pub fn players_are_hostile(&self, attacker: PlayerId, victim: PlayerId) -> bool {
        attacker != victim
            && self.content.player_combat_policy() == PlayerCombatPolicy::FreeForAll
            && self.players.contains_key(&attacker)
            && self.players.contains_key(&victim)
    }

    pub(crate) fn attackable_by_player(&self, attacker: PlayerId, target: EntityRef) -> bool {
        // No combat into, out of or between submerged units (#274).
        if self.combat_blocked(EntityRef::Player(attacker), target) {
            return false;
        }
        match target {
            EntityRef::Creature(id) => self.creatures.get(&id).is_some_and(|unit| unit.is_alive()),
            EntityRef::Player(id) => {
                self.players_are_hostile(attacker, id)
                    && self.players.get(&id).is_some_and(|unit| unit.is_alive())
            }
            EntityRef::Npc(_) => false,
        }
    }

    /// Weapon impacts use the same shield geometry/momentum and stance as creature blows.
    pub(crate) fn player_melee_hits(
        &mut self,
        attacker: PlayerId,
        target: EntityRef,
        amount: u32,
        critical: bool,
        now: u64,
    ) -> Result<(), ZoneError> {
        self.player_melee_hits_at(attacker, target, amount, critical, now, None)
    }

    pub(crate) fn player_melee_hits_at(
        &mut self,
        attacker: PlayerId,
        target: EntityRef,
        amount: u32,
        critical: bool,
        now: u64,
        strike_lane: Option<crate::StrikeHeight>,
    ) -> Result<(), ZoneError> {
        let amount = if let EntityRef::Player(victim) = target {
            if !self.players_are_hostile(attacker, victim)
                || self.block_player_melee(
                    EntityRef::Player(attacker),
                    victim,
                    u16::try_from(amount).unwrap_or(u16::MAX),
                    strike_lane,
                )?
            {
                return Ok(());
            }
            self.stance_exposure(victim, amount)?
        } else {
            amount
        };
        self.player_damages_unit(attacker, target, amount, critical, now)
    }

    /// The same player-origin hit resolver for creatures and eligible arena opponents.
    pub(crate) fn player_damages_unit(
        &mut self,
        attacker: PlayerId,
        target: EntityRef,
        amount: u32,
        critical: bool,
        now: u64,
    ) -> Result<(), ZoneError> {
        match target {
            EntityRef::Creature(creature) => {
                self.player_damages_creature(attacker, creature, amount, critical, now)
            }
            EntityRef::Player(victim) if self.players_are_hostile(attacker, victim) => {
                self.update_player(attacker, |player| {
                    player.combat_timer = crate::unit::COMBAT_LINGER_TICKS
                });
                self.damage_player(EntityRef::Player(attacker), victim, amount, critical)
            }
            _ => Ok(()),
        }
    }
}
