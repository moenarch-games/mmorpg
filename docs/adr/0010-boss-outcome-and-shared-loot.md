# Thorn's boss outcome and shared Need/Greed loot are committed through the run directory

Status: accepted; implements the boss part of #404 (Atlas 5.1.28, 5.1.31 to 5.1.35, 5.1.37, 5.1.83, 5.1.i404-4 to 5.1.i404-6).

## Decision

Captain Ulric Thorn is an ordinary zone creature, authored as a `DepthsBoss` on the depths site: level 6 elite, 900 hp, standing only once every chamber of a run is cleared. The zone (`mmorpg_core::depths_boss`) owns his fight and the shared pool; scaling is the frozen run profile (5.1.26, 5.1.27) in adds and mechanics, never multipliers.

A run's `DepthsRun` holds the `BossAttempt` (eligible character snapshot at first engagement, phase, called adds) and, once he falls, a `BossOutcome`: a monotonic **version**, the characters eligible at the kill (5.1.35), the drops (each a `DropState` with identity `(run, drop index)`) and the roll seed. Every state the directory must not lose (kill, each settled drop) raises the version.

The grant is **two-step**, reserve then deliver:

1. The zone settles a drop deterministically (tick-based windows, Need before Greed, `resolve_roll` over stable per-character hashes) and marks it `Awarded` for the winner, in canonical state.
2. The host's instance service reports the outcome to the run directory in a fenced transaction (`InstanceDirectory::report_outcome`, opaque bytes, only newer versions replace) and only then submits the confirmation input `[1, 13, run, version]`. The zone delivers an item, or boss experience and the victory evidence, only for versions the directory holds, writing a `DepthsRecord` receipt in the character record in the same step.

Recovery rebinds the run from the directory: the boss does not stand again (`begin_depths_run` restores the outcome), confirmed drops wait to be delivered, and delivery checks the arriving character's receipts so an item already committed to a character is not granted twice. An open window at a crash becomes a manual claim (`Held`): its answers were never durable, and it is never rerolled. The asymmetric hole is the intended one: a crash may lose an uncommitted grant, it can never duplicate one. Boss credit follows the same path, so the boss reward does not inherit the creature-kill gap ADR 0009 notes. Adds give no rewards so a retry cannot farm them.

Alternatives considered: writing grants directly into bags at settlement and reporting to the directory afterward (a commit between the two after a crash lets a re-kill or a second winner duplicate the reward), and storing the pool in the character record (several writers for one drop).

## Run end

The directory learns the outcome and which characters still need the run (`outstanding`: an unanswered roll or an undelivered award). A character on a completed run who touches the public threshold and owes nothing is released at once and starts a fresh run; a run with no remaining bound character retires immediately, without waiting for `MMORPG_DEPTHS_GRACE_MS` (5.1.31). A character with an open roll resumes the room. A completed run can never be resumed into a second boss kill or reward.

## Behaviour decided here

- The boss chamber's passage is open; he only exists after the chambers are clear, so the part 2 barricade is gone.
- Adds: two for parties of one or two, three for three or four, four for five, Bandits then a Crossbowman and a Brute.
- Telegraphed attacks: *Reaver's Sweep* (a 4.5 m ring on a 2 s wind-up, from 50 % health, on a 10 s cooldown) and, for parties of three or more below 25 % health, *Last Command* (a 7 m ring on a 2.5 s wind-up) alternating with it.
- Roll windows are 30 s; eligible characters may answer while dead (5.1.35). Manual claims need a living claimant in the zone; a winner whose bag is full keeps the item reserved until it fits, until the run ends.
- Need eligibility is the item's own equipment slot; the catalog has no class or level restriction (5.1.33).

## Not covered

No follow-on story quest yet; the durable `thorn_defeated` evidence (5.1.37) waits for one. Characters are not recoverable into a run whose directory record is gone. The in-memory `LocalInstances` directory loses a run across a process restart, like every other single-process host state (ADR 0009).
