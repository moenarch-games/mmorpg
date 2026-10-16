//! Canonical ability state of a unit: its auras, its cast or channel and a
//! player's cooldowns, with the pure bookkeeping rules they follow.
//!
//! - At most [`MAX_AURAS`] auras rest on a unit, in slot order.
//! - Re-applying the same ability from the same caster refreshes the aura
//!   in its slot.
//! - On a full unit, the aura with the least remaining time is replaced in
//!   its slot; ties replace the lower ability ID.
//! - Root and stun zero movement; snares scale it; haste scales the swing
//!   timer. Death clears every aura.

use crate::EntityRef;
use crate::ability::{AbilityId, AuraKind, AuraSpec, MAX_AURAS, ability_by_id};

/// One aura on a unit. `amount` is the total of a damage or heal over
/// time, the remaining shield of an absorb, the percentage of a snare or
/// haste, and 0 for roots and stuns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Aura {
    pub ability: AbilityId,
    pub caster: EntityRef,
    /// Ticks left; an aura reaching 0 is removed.
    pub remaining: u16,
    pub amount: u16,
}

impl Aura {
    /// The catalog aura of this aura's ability; recovery validates it exists.
    pub(crate) fn spec(&self) -> Option<AuraSpec> {
        ability_by_id(self.ability)?.aura()
    }

    pub(crate) fn kind(&self) -> Option<AuraKind> {
        self.spec().map(|spec| spec.kind)
    }

    /// Ticks since the aura was applied or refreshed.
    pub(crate) fn elapsed(&self) -> u16 {
        self.spec()
            .map_or(0, |spec| spec.duration.saturating_sub(self.remaining))
    }
}

/// A cast or channel in progress. `elapsed` counts resolved ticks and stays
/// below the ability's cast time; `point` is a channel's fixed target point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CastState {
    pub ability: AbilityId,
    pub elapsed: u16,
    pub target: Option<EntityRef>,
    pub point: Option<[i32; 2]>,
}

/// A running player cooldown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cooldown {
    pub ability: AbilityId,
    pub remaining: u16,
}

/// Adds `aura`, refreshing the same caster's aura of the same ability, and
/// returns the aura it evicted from a full unit, if any.
pub(crate) fn apply_aura(auras: &mut Vec<Aura>, aura: Aura) -> Option<Aura> {
    if let Some(existing) = auras
        .iter_mut()
        .find(|existing| existing.ability == aura.ability && existing.caster == aura.caster)
    {
        *existing = aura;
        return None;
    }
    if auras.len() < MAX_AURAS {
        auras.push(aura);
        return None;
    }
    let slot = auras
        .iter()
        .enumerate()
        .min_by_key(|(_, existing)| (existing.remaining, existing.ability))
        .map(|(slot, _)| slot)?;
    Some(std::mem::replace(&mut auras[slot], aura))
}

pub(crate) fn has_kind(auras: &[Aura], kind: AuraKind) -> bool {
    auras.iter().any(|aura| aura.kind() == Some(kind))
}

/// Movement speed in percent: 0 while rooted or stunned, otherwise reduced
/// by the strongest snare.
pub(crate) fn speed_percent(auras: &[Aura]) -> i32 {
    if has_kind(auras, AuraKind::Root) || has_kind(auras, AuraKind::Stun) {
        return 0;
    }
    let snare = auras
        .iter()
        .filter(|aura| aura.kind() == Some(AuraKind::Snare))
        .map(|aura| i32::from(aura.amount))
        .max()
        .unwrap_or(0);
    (100 - snare).max(0)
}

/// Scales a horizontal velocity by `percent`, rounding toward zero.
pub(crate) fn scale_velocity(velocity: [i32; 2], percent: i32) -> [i32; 2] {
    velocity.map(|component| {
        i32::try_from(i64::from(component) * i64::from(percent) / 100).unwrap_or(0)
    })
}

/// A swing interval shortened by the strongest haste: `ticks × 100 / (100 + haste)`.
pub(crate) fn hasted_swing(ticks: u16, auras: &[Aura]) -> u16 {
    let haste = auras
        .iter()
        .filter(|aura| aura.kind() == Some(AuraKind::Haste))
        .map(|aura| u32::from(aura.amount))
        .max()
        .unwrap_or(0);
    u16::try_from(u32::from(ticks) * 100 / (100 + haste)).unwrap_or(ticks)
}

/// The amount of the `pulse`-th (1-based) of `pulses` pulses of `total`, so
/// the pulses sum to exactly `total`.
pub(crate) fn pulse_amount(total: u16, pulse: u16, pulses: u16) -> u16 {
    if pulses == 0 {
        return 0;
    }
    let share = |index: u16| u32::from(total) * u32::from(index) / u32::from(pulses);
    u16::try_from(share(pulse) - share(pulse - 1)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::ids;

    fn aura(ability: AbilityId, caster: u32, remaining: u16) -> Aura {
        Aura {
            ability,
            caster: EntityRef::Player(caster),
            remaining,
            amount: 0,
        }
    }

    #[test]
    fn auras_refresh_per_caster_and_replace_the_shortest_when_full() {
        let mut auras = Vec::new();
        assert_eq!(
            apply_aura(&mut auras, aura(ids::SERPENT_STING, 1, 100)),
            None
        );
        assert_eq!(
            apply_aura(&mut auras, aura(ids::SERPENT_STING, 1, 450)),
            None
        );
        assert_eq!(
            auras,
            [aura(ids::SERPENT_STING, 1, 450)],
            "refreshed in place"
        );
        assert_eq!(
            apply_aura(&mut auras, aura(ids::SERPENT_STING, 2, 300)),
            None
        );
        for caster in 3..=8 {
            apply_aura(&mut auras, aura(ids::CONCUSSIVE_SHOT, caster, 50));
        }
        assert_eq!(auras.len(), MAX_AURAS);
        // Ties on remaining time replace the lower ability ID: all six snares
        // tie, so the earliest slot of the lowest ability goes.
        let evicted = apply_aura(&mut auras, aura(ids::FROST_NOVA, 9, 180));
        assert_eq!(evicted, Some(aura(ids::CONCUSSIVE_SHOT, 3, 50)));
        assert_eq!(auras[2], aura(ids::FROST_NOVA, 9, 180));
        let evicted = apply_aura(&mut auras, aura(ids::SHIELD_BASH, 10, 60));
        assert_eq!(evicted, Some(aura(ids::CONCUSSIVE_SHOT, 4, 50)));
        let mut tied = vec![aura(ids::FROST_NOVA, 1, 5), aura(ids::SHIELD_BASH, 2, 5)];
        tied.extend((3..=8).map(|caster| aura(ids::SERPENT_STING, caster, 9)));
        assert_eq!(
            apply_aura(&mut tied, aura(ids::RAPID_FIRE, 9, 300)),
            Some(aura(ids::SHIELD_BASH, 2, 5)),
            "the lower ability ID loses a tie"
        );
    }

    #[test]
    fn movement_swing_and_pulse_arithmetic() {
        let snare = Aura {
            amount: 50,
            ..aura(ids::CONCUSSIVE_SHOT, 1, 10)
        };
        assert_eq!(speed_percent(&[]), 100);
        assert_eq!(speed_percent(&[snare]), 50);
        assert_eq!(speed_percent(&[snare, aura(ids::FROST_NOVA, 1, 10)]), 0);
        assert_eq!(speed_percent(&[aura(ids::SHIELD_BASH, 1, 10)]), 0);
        assert_eq!(scale_velocity([19, -7], 50), [9, -3]);
        let haste = Aura {
            amount: 40,
            ..aura(ids::RAPID_FIRE, 1, 300)
        };
        assert_eq!(hasted_swing(60, &[]), 60);
        assert_eq!(hasted_swing(60, &[haste]), 42);
        let pulses: Vec<_> = (1..=5).map(|pulse| pulse_amount(34, pulse, 5)).collect();
        assert_eq!(pulses, [6, 7, 7, 7, 7]);
        assert_eq!(pulses.iter().sum::<u16>(), 34);
    }
}
