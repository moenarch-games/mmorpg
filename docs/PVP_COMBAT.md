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

## Timed swings and clashes (#679, arena first)

An arena opts in with `ZoneContent::with_timed_combat(TimedCombat::ARENA)`
(`crates/mmorpg-core/src/swing.rs`). The content revision binds the rules, so restoring
arena state into content without them fails closed. Greyhaven does not opt in: its
directed strikes still resolve in the tick their command arrives.

- **Begin on the wire.** The ordinary `DirectedStrike` command begins the swing: validation,
  Rage, cooldowns, the global cooldown and the guard release apply at once, and observers get
  `SwingBegan` with the aim and the phase lengths, so opponents see the wind-up.
- **Timed hitbox.** After the startup (0.3 s) the blade sweeps the strike's usual arc, reach and
  height lane over the active ticks (0.15 s), one contiguous slice per tick, travelling toward
  the aimed side (a centre strike right to left). The nearest eligible unit a tick's slice covers
  takes the ordinary weapon strike, physical cover still blocking; one swing strikes at most one
  unit and reports a whiff on its last active tick. Then the action graph's `DirectedSlash`
  recovery runs. The action graph reports the swing as `DirectedSlash` in its startup and
  active phases; a guard raised meanwhile waits like during the recovery.
- **Clashes**, in the zone's present tick and stable ID order, before any slice strikes:
  two hostile active swings whose whole arcs cover each other deal nothing and both recover;
  an active swing destroys a clashable hostile projectile passing through its arc and swings
  on; two clashable projectiles of opposing owners that pass within their clash radii (arrows
  15 cm, Firebolts 25 cm) both vanish when both attacks' content permits the interaction.
  `ProjectileInterception` declares ordinary melee, arrow and Firebolt permissions separately
  for plain arrows and Firebolts. Arena defaults permit swords to parry plain arrows and
  arrows to intercept arrows; Firebolts intercept other Firebolts but cannot be parried by an
  ordinary sword or intercepted by an arrow. Free and targeted/homing flight use the same
  permissions and the actual current tracking segment. Bombs, flasks, piercing, ricochet and
  other unsupported shots stay out; no new shield or magical-defense ability is introduced.
  `Clashed` names both sides. A stun or death ends a running swing.
- **Recovery.** The running swing is canonical player state, so a zone restored mid-swing
  continues it exactly (`cargo test -p mmorpg-core --test timed_swing`).

Health alone decides fights: no clash moves anyone (Atlas 5.2.i679-2). The phase lengths, clash
switches and radii are starting values to tune in play, as is the 2 s basic swing timer.
The browser's practice arena and hosted Battle Royale matches run these rules:
`battle_royale_core::arena::content()` opts in (battle-royale #50). Greyhaven is unchanged
until the arena playtest settles them.

## Arena netcode trial (#680, Atlas 5.9.i680)

Three options a tester compares in an arena match; nothing is assumed to win and the owner picks
what stays.

1. **Baseline:** zone authority, every unit drawn 67 ms behind the newest projection, the own
   body included (today's online path).
2. **Predicted movement** (per client): `mmorpg_client_model::prediction::PredictedMovement`
   moves the own body at once with `mmorpg_core::MovementPredictor`, a private zone over the same
   content holding only that body, so it is the zone's own movement and physics. It keeps every
   sent command until a projection acknowledges it; the first acknowledgement dates the command
   on the zone's ticks, and each projection restarts the prediction from the zone's own body and
   replays what was sent since. Other units, auras and combat are absent from the prediction;
   the zone's answer corrects them within a round trip. Projections round velocity to whole units
   per tick, so mid-air replays may drift by about half a unit per replayed tick
   (`cargo test -p mmorpg-client-model --test prediction`).
3. **Predicted movement plus capped hit rewind** (per match, the same for everyone):
   `ZoneContent::with_hit_rewind(HitRewind::ARENA_TRIAL)` (needs timed swings; bound into the
   content revision). When a swing begins the zone fixes its rewind from its own knowledge: the
   ticks since the tick the strike's client had observed (the #415 lateness, which the zone
   validates and caps at 150 ms) plus the clients' 67 ms interpolation (4 ticks), capped at the
   match's **150 ms** (9 ticks; at most 200 ms for any match). The sweep then tests each body
   where it stood that many ticks earlier, from the swinger's present position, through present
   cover; eligibility, damage and deaths stay in the present. **Clashes are never rewound**: a
   clash compares two swings launched from different views of the past, so it reads the
   present. The position history is canonical (`RewindFrame`s for the last cap of ticks), so a
   restored zone rewinds exactly like the original (`cargo test -p mmorpg-core --test
   hit_rewind`). A command without an observation counts as on time (interpolation only).

The cap is tuning: at 50 ms round trip a hit is rewound fully (3 + 4 ticks); from about 85 ms on
the cap is reached. In the browser the Loadout's **Practice** route offers the option and a
simulated round trip (see [BATTLE_ROYALE.md](BATTLE_ROYALE.md#netcode-trial-680)). A hosted
Battle Royale match chooses rewind per match through its arena content
(`battle_royale_core::arena::content_for`, host configuration `<id>:rewind`), and the browser's
hosted source predicts the own body when the client picks option 2 or 3.

## Mode observations and continuation

A mode controller reads authoritative health/death from the zone's canonical snapshot;
`DamageTaken`, `DamageDealt`, `DamageObserved`, `Blocked` and `Died` retain their existing
entity references and player-killer identity. Match phases, storm, elimination, winner,
rewards and between-life loadout policy remain the consuming mode's responsibility.
Do not mirror player health or physics into a competing match simulation.

For a mode hazard such as Battle Royale's storm, the trusted host calls
`apply_environmental_damage(player, amount)` on the zone. The mode owns eligibility,
tick ordering and replay of these calls. The zone subtracts direct health damage,
bypassing armor, guard and absorption, suppresses calm regeneration and uses normal
death cleanup. It returns actual health lost; unknown players fail before mutation,
and zero damage or dead players do nothing. This grants no attack credit, rage or
rewards. Updated health is visible in the normal projection; lethal damage emits
the existing `Died { killer: None }`, without a fabricated source or attack event.
This host API adds no client command or snapshot field. Match recovery must retain
the mode state that determines future hazard calls as well as the complete zone.

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

This is the reusable combat capability. [Temporary max-level builds](EPHEMERAL_CHARACTERS.md) use the separate ephemeral character admission API (#340);
Battle Royale runtime composition remains battle-royale #13; client entry remains #341.
No playable Battle Royale match or large-player performance claim is made here.

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.
