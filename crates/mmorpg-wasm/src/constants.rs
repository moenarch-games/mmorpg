//! Rule facts the browser shows or uses as hints (#1159): reaches, the Pages day length
//! and what each ability's definition states, read from the core so a core change cannot leave
//! the interface stating a stale number. The zone still checks every intent; nothing here decides.
use mmorpg_core as core;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

/// The core's reaches in zone units (100 per metre) and the Pages day; `tickHz` is exported beside.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct ZoneConstants {
    /// Looting a corpse or skinning it.
    pub loot_reach_units: i32,
    /// Trading with a vendor, XZ.
    pub vendor_reach_units: i32,
    /// Talking to a quest giver or reporting to a raid lookout, XZ.
    pub quest_reach_units: i32,
    /// Mining an ore vein, XZ.
    pub gather_reach_units: i32,
    /// Tending or mending a raid aftermath aid task, XZ.
    pub aid_reach_units: i32,
    /// Reading a raid's tracks, XZ.
    pub tracks_reach_units: i32,
    /// Bracing a barn wall before a raid, XZ.
    pub brace_reach_units: i32,
    /// One cosmetic Pages day in real seconds (`mmorpg_presentation::clock::PAGES_DAY_SECONDS`).
    pub day_seconds: f64,
}

#[must_use]
pub fn zone_constants() -> ZoneConstants {
    ZoneConstants {
        loot_reach_units: core::LOOT_REACH_UNITS,
        vendor_reach_units: core::VENDOR_REACH_UNITS,
        quest_reach_units: core::QUEST_REACH_UNITS,
        gather_reach_units: core::GATHER_REACH_UNITS,
        aid_reach_units: core::raid::AID_REACH_UNITS,
        tracks_reach_units: core::raid::TRACKS_REACH_UNITS,
        brace_reach_units: core::raid::BRACE_REACH_UNITS,
        day_seconds: mmorpg_presentation::clock::PAGES_DAY_SECONDS,
    }
}

#[wasm_bindgen(js_name = zoneConstants)]
pub fn zone_constants_js() -> Result<tsify::Ts<ZoneConstants>, JsError> {
    Ok(zone_constants().into_ts()?)
}

/// What an ability's rules state, for checking its prose description against (#1159): the
/// distances, durations, percentages and small counts its definition uses, and the outcomes a
/// reader is told about. Each list holds every value a description may truthfully name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct AbilityFacts {
    pub id: u32,
    /// Range, radii, lengths and distances in metres.
    pub metres: Vec<f64>,
    /// Cast or channel time, cooldown, aura and summon durations and fuses in seconds.
    pub seconds: Vec<f64>,
    /// Heal, slow, haste, speed and reckoning shares.
    pub percents: Vec<u16>,
    /// Extra targets, jumps, bounces, standing helpers and whole weapon-damage multiples.
    pub counts: Vec<u32>,
    /// It knocks or hurls what it hits away.
    pub knockback: bool,
    /// Walls stop it: a shot, a beam, a charge, a pull, a leap's flight or a blink.
    pub walls_stop: bool,
    /// A raised shield turned toward it stops it.
    pub shields_stop: bool,
    /// It lands only on what its centre can see, so walls shield what lies behind them.
    pub needs_sight: bool,
    /// A target that was just moved by force is immune to it.
    pub forced_move_immunity: bool,
}

fn metres(units: i32) -> f64 {
    f64::from(units) / 100.0
}

fn seconds(ticks: u16) -> f64 {
    f64::from(ticks) / f64::from(core::TICK_HZ)
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The facts of one catalog ability.
#[must_use]
pub fn ability_facts(ability: &core::Ability) -> AbilityFacts {
    use core::AbilityEffect as Effect;
    let mut facts = AbilityFacts {
        id: ability.id.get(),
        metres: ability.range.reach().map(metres).into_iter().collect(),
        seconds: Vec::new(),
        percents: Vec::new(),
        counts: Vec::new(),
        knockback: false,
        walls_stop: ability.projectile.is_some(),
        shields_stop: ability.projectile.is_some(),
        needs_sight: false,
        forced_move_immunity: false,
    };
    for ticks in [ability.cast.ticks(), ability.cooldown]
        .into_iter()
        .chain(ability.aura().map(|aura| aura.duration))
    {
        if ticks > 0 {
            facts.seconds.push(seconds(ticks));
        }
    }
    match ability.effect {
        Effect::WeaponStrike {
            numerator,
            denominator,
            ..
        } => {
            if denominator != 0 && numerator % denominator == 0 {
                facts.counts.push(u32::from(numerator / denominator));
            }
        }
        Effect::Bash {
            lockout_ticks,
            knockback,
            ..
        } => {
            facts.seconds.push(seconds(lockout_ticks));
            facts.knockback = knockback > 0;
        }
        Effect::HealOverTime { percent, .. }
        | Effect::Snare { percent, .. }
        | Effect::Haste { percent, .. }
        | Effect::Reckoning { percent, .. }
        | Effect::Shapeshift { speed: percent, .. } => facts.percents.push(percent),
        Effect::Bandage {
            percent,
            below_percent,
        } => facts.percents.extend([percent, below_percent]),
        Effect::Cleave { extra, radius } => {
            facts.counts.push(u32::from(extra));
            facts.metres.push(metres(radius));
        }
        Effect::DamageOverTime { .. }
        | Effect::Bolt { .. }
        | Effect::Absorb { .. }
        | Effect::Drain { .. }
        | Effect::Companion { .. }
        | Effect::SignalCall => {}
        Effect::Nova {
            radius,
            unbreakable_ticks,
            ..
        } => {
            facts.metres.push(metres(radius));
            facts.seconds.push(seconds(unbreakable_ticks));
        }
        Effect::Root {
            unbreakable_ticks, ..
        } => facts.seconds.push(seconds(unbreakable_ticks)),
        Effect::Blizzard { radius, .. } => facts.metres.push(metres(radius)),
        Effect::Blast {
            radius, knockback, ..
        }
        | Effect::Stomp {
            radius, knockback, ..
        }
        | Effect::Smash {
            range: radius,
            knockback,
            ..
        } => {
            facts.metres.push(metres(radius));
            facts.knockback = knockback > 0;
            facts.needs_sight = true;
        }
        Effect::Charge { knockback } => {
            facts.knockback = knockback > 0;
            facts.walls_stop = true;
        }
        Effect::Cone { range, percent, .. } => {
            facts.metres.push(metres(range));
            facts.percents.push(percent);
            facts.needs_sight = true;
        }
        Effect::Beam { length, .. } => {
            facts.metres.push(metres(length));
            facts.walls_stop = true;
            facts.shields_stop = true;
        }
        Effect::Chain { jumps, radius, .. } => {
            facts.counts.push(u32::from(jumps));
            facts.metres.push(metres(radius));
            facts.needs_sight = true;
        }
        Effect::Splash { radius, .. } => {
            facts.metres.push(metres(radius));
            facts.needs_sight = true;
        }
        Effect::Summon { duration, .. } => {
            facts.seconds.push(seconds(duration));
            facts.counts.push(count(core::MAX_MINIONS_PER_SUMMONER));
        }
        Effect::SummonTerrain { .. } => facts.metres.push(metres(core::SUMMON_DISTANCE_UNITS)),
        Effect::Leap {
            distance,
            radius,
            knockback,
            ..
        } => {
            facts.metres.extend([metres(distance), metres(radius)]);
            facts.knockback = knockback > 0;
            facts.walls_stop = true;
            facts.needs_sight = true;
        }
        Effect::Hook { .. } => {
            facts.walls_stop = true;
            facts.forced_move_immunity = true;
        }
        Effect::Blink { distance } => {
            facts.metres.push(metres(distance));
            facts.walls_stop = true;
        }
        Effect::RaiseDead { duration, .. } => {
            facts.seconds.push(seconds(duration));
            facts.metres.push(metres(core::CORPSE_SPEND_REACH_UNITS));
            facts.counts.push(count(core::RAISE_DEAD_LIMIT));
        }
        Effect::CorpseBurst { radius, .. } => {
            facts
                .metres
                .extend([metres(core::CORPSE_SPEND_REACH_UNITS), metres(radius)]);
            facts.needs_sight = true;
        }
    }
    match ability.shot() {
        core::Shot::Pierce => facts.counts.push(count(core::PIERCE_HITS)),
        core::Shot::Ricochet => facts.counts.push(u32::from(core::RICOCHET_BOUNCES)),
        core::Shot::Bounce => facts.seconds.push(seconds(core::BOMB_FUSE_TICKS)),
        core::Shot::Plain | core::Shot::Shatter => {}
    }
    facts
}

/// The facts of every catalog ability, in ID order.
#[must_use]
pub fn all_ability_facts() -> Vec<AbilityFacts> {
    core::ABILITY_CATALOG.iter().map(ability_facts).collect()
}

#[wasm_bindgen(js_name = abilityFacts, unchecked_return_type = "AbilityFacts[]")]
pub fn ability_facts_js() -> Result<wasm_bindgen::JsValue, JsError> {
    crate::client_model::to_js(&all_ability_facts())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_the_cores() {
        let constants = zone_constants();
        assert_eq!(constants.vendor_reach_units, 500);
        assert_eq!(constants.aid_reach_units, core::raid::AID_REACH_UNITS);
    }

    #[test]
    fn every_ability_has_facts_in_id_order() {
        let facts = all_ability_facts();
        assert_eq!(facts.len(), core::ABILITY_CATALOG.len());
        assert!(facts.windows(2).all(|pair| pair[0].id < pair[1].id));
        let hook = facts.iter().find(|facts| facts.id == 52).expect("hook");
        assert!(hook.walls_stop && hook.forced_move_immunity);
    }
}
