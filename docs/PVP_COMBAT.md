# Authoritative opt-in arena combat (#339)

`mmorpg-core::ZoneSimulation` resolves player attacks, directed weapon sweeps, shields,
class abilities, projectiles, hostile auras, damage, death and forced movement in its
existing single authoritative tick. Ordinary content, including Greyhaven, defaults to
`PlayerCombatPolicy::Safe`; its players cannot damage, root, stun or knock one another
through these player-origin attacks. Exploding PvE barrels retain their existing hazard
behavior. No client chooses hit results.

## Dedicated arena admission

A host explicitly builds an isolated arena's immutable content before creating its zone:

```rust
use std::sync::Arc;
use mmorpg_core::{PlayerCombatPolicy, ZoneContent, ZoneId, ZoneSimulation};

fn arena(content: ZoneContent) -> Result<ZoneSimulation, mmorpg_core::ZoneError> {
    let content = Arc::new(content.with_player_combat(PlayerCombatPolicy::FreeForAll));
    ZoneSimulation::with_content(ZoneId::new(100), content)
}
```

Every distinct admitted player in `FreeForAll` is an eligible opponent. The host owns
explicit opt-in admission; spectators must not be admitted as active combatants.
`players_are_hostile(attacker, victim)` exposes eligibility, while player projections
mark living opponents hostile and attackable. Self-targeting remains invalid for
hostile abilities. Self heals/buffs keep their existing behavior. There is no runtime
world flag toggle, duel implementation or team policy in this slice.

Use the existing `add_player`, ordered `apply_command` and `advance_tick` APIs. Class,
ability unlocks, resources, cooldowns, range, visibility, physical cover, shields, status
and death remain the shared rules. Targeted player melee checks line of sight; directed
sweeps use the aimed height lane against a guard. Confirmed directed hits open the same
buffered Shield Bash combo, bound to the actual player or creature struck. Projectile
collision includes eligible opponents and retains physical cover and shield interception.
Area effects include eligible opponents, excluding the caster; hostile aura pulses use
the same damage resolver. No PvE XP/copper or creature loot is awarded for killing a player.

## Mode observations and continuation

A mode controller reads authoritative health/death from the zone's canonical snapshot;
`DamageTaken`, `DamageDealt`, `DamageObserved`, `Blocked` and `Died` retain their existing
entity references and player-killer identity. Match phases, storm, elimination, winner,
rewards and between-life loadout policy remain the consuming mode's responsibility.
Do not mirror player health or physics into a competing match simulation.

The content fingerprint binds the arena policy: restoring arena state with safe content
fails closed. Casts, poison auras, forced movement and player-target combo openings retain
complete continuation. Snapshot wire v30 / schema 29 encode combo targets as entity
references; commands stay at v13. Old snapshots/checkpoints require a compatible build
and fail closed in this one. The player projection payload layout is unchanged.

## Focused evidence and remaining delivery

`cargo test -p mmorpg-core --locked --test pvp` covers thirteen deterministic two-player
checks: explicit policy and safe refusal, projection eligibility, weapon hit/guard,
auto-attacks, a real Firebolt projectile/guard, area root isolation, death attribution,
physical wall coverage, and exact recovery of a cast, poison, directed-strike combo and
charge, plus valid continuation when an opponent retires during a cast/combo/charge. The charge check found and fixed shared tick ordering: knockback created during
forced-move processing now receives its first velocity before stopped-motion detection.
Existing PvE combat/ability/trace, forced-movement, combo and directed-strike checks pass.
Canonical codec checks preserve both target kinds. Rust-generated v30 projection fixtures
are decoded by the browser; v29 remains rejection evidence.

This is the reusable combat capability. Max-level temporary builds remain #340;
Battle Royale runtime composition remains battle-royale #13; client entry remains #341.
No playable Battle Royale match or large-player performance claim is made here.

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.
