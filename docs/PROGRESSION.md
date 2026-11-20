# Starter progression rules

Issue #74 defines the immutable rule queries in `mmorpg-core`. Issue #75 applies them on authoritative creature deaths and persists/projects
progression. Issue #76 owns the browser XP bar and feedback. The rule queries
themselves never grant XP.

Player progression supports levels 1–10. The next-level thresholds are
current-level XP, not cumulative totals:

| Current level | XP to next level | Estimated minutes (#908) |
| --- | ---: | ---: |
| 1 | 100 | 1 |
| 2 | 300 | 3 |
| 3 | 600 | 6 |
| 4 | 1,000 | 10 |
| 5 | 1,600 | 16 |
| 6 | 2,000 | 20 |
| 7 | 2,400 | 24 |
| 8 | 2,800 | 28 |
| 9 | 3,200 | 32 |
| 10 | 0 (cap) | — |

The total from level 1 to level 10 is 14,000 XP. The future authority consumes
each threshold on level-up and carries remaining current-level XP forward.
At the cap, XP and its next-level threshold are zero. The existing health and
damage growth rules remain the stat foundation; resources belong to #23.

`kill_experience(player_level, creature_level)` returns the nominal kill
reward, before authoritative eligibility. Creature levels remain valid through
60, independently of the starter player cap. Base XP is `45 + 5 × creature
level`. A creature one/two/three/four levels below pays 80/60/40/20% of base;
five or more below pays zero. Equal level pays 100%. Each level above adds 10%
to a maximum of 150%. Round the final integer reward down. At player level 10
the reward is zero.

Examples: a level-1 player gets 50 XP from a level-1 creature, so two eligible
kills reach level 2. A level-5 player gets 70 from level 5, 52 from level 4,
10 from level 1, or 82 from level 6. A level-6 player gets zero from level 1.
Quest XP and other rewards are outside this kill rule.

## Pacing (#908)

The owner's moderate-leveling choice (2026-11-17, B) wants the first levels
quickly and later early levels in roughly 20–40 minutes of mixed Greyhaven
play, as an initial playtest target rather than a fixed rule. #908 tuned only
the threshold curve above; kill rewards, quest rewards, eligibility and the
level cap are unchanged.

The measurement is the headless quest-chain journey
(`crates/mmorpg-core/tests/quest_chain.rs`): a new Warden plays the opening
quests to Millbrook's *The Missing Farmhand*, fighting whatever attacks on the
way, and prints its level-up ticks (60 Hz). For #908 the opening was *Trouble in
the Woods* (six wolves) and *Pelts for the Tanner* (five pelts); #984 replaced
it, see below:

| Curve | Level 2 | Level 3 | Level 4 | Level 5 | Three quests end |
| --- | ---: | ---: | ---: | ---: | --- |
| Before (100–900, 4,500 total) | 0:50 | 1:41 | 3:08 | 5:54 | 6:23, level 5 |
| After (above, 14,000 total) | 0:50 | 2:16 | 5:15 | — | 5:44, level 4 |

The bot earns about 200 XP per game minute: it never reads, explores or
detours. The estimated minutes in the table assume a human earns about half
that, 100 XP per minute of mixed combat, quests and exploration; later quests
pay more XP while lower-level creatures pay less, so the estimate holds the
rate flat. The curve's 14,000 XP compares with 10,840 XP from all 32 Greyhaven
quests, so the cap arrives near the end of the chain with ordinary combat
supplying the rest rather than past its middle, as the old 4,500 did. The
journey asserts level 2 within two minutes and level 3 or 4 after the opening
quests. The human rate and later-level minutes are not yet playtested.

Both public queries return `None` for unsupported levels. The capped threshold
is `Some(0)`, distinct from invalid input. No client command can call these
queries to mutate XP. The authority applies these rules and rejects invalid persisted progression;
#74 introduced no snapshot changes and #75 deliberately versions them.

Exact public-API fixtures cover the curve, rounding, gray rewards, bonus cap,
player cap and invalid inputs. Conventions sourceRevision:
`46d8793bb3034326561f876dcc67dbaa5aa1e432`.

## World activities (#916)

Combat stays a worthwhile route (owner choice B, 2026-11-17), and two world
activities pay explicit, bounded XP beside quests. Kill, tap, party and quest
rules are unchanged; there is no multiplier and no anti-grind penalty.

- **Discovery.** Greyhaven lists every named area but the Outpost, where every
  character starts, as a discovery (`ZoneContent::with_discoveries`, 26
  areas). The first tick a living character stands inside one it has not
  discovered pays `discovery_experience(level)` = `(45 + 5 × level) × 3 / 2`,
  rounded down: 75 at level 1, 97 at 4, 135 at 9, zero at the cap; about one
  and a half equal-level kills. The discovered set is a durable character fact
  (canonical snapshot and character record), written with the XP it paid, so
  replay, reconnect and recovery never pay an area twice; hosts that track
  rewards also hold a `Discovery` receipt. The whole pool is about 2,700 XP,
  a fifth of the 14,000 curve.
- **Night defense.** The Millbrook raid's participation reward (#531: 120 XP
  and 15 copper once per incident) gains a defence bonus: when the barn is
  defended, each rewarded participant still within 60 m earns 240 XP more,
  once per incident and character (`RaidDefence` receipt). A lost raid pays no
  bonus. With the 120 XP that is about 100 XP a minute over the raid's three
  75 s phases, kills aside, and raids come at most once a night after a
  one-hour cooldown.

No crafting system exists yet, so crafting XP is left to #917.

The quest-chain journey now also walks a mixed path. Discovering the Wolfrun
Woods and Millbrook Farm on the way moves the opening chain's level-ups to
0:31, 2:26 and 4:47 (from 0:50, 2:16 and 5:15) and ends the three quests at
5:16 on level 4 with 230/1,000 XP. A 41 s detour from Millbrook through the
Eastwater Fen and the Southmeadow Downs into the Farmland and Hamlets then
discovers three more areas for 291 XP, reaching 521/1,000. That is faster
than the bot's 200 XP a minute of fighting, as it should be for a walk to new
places, but the pool is finite and the far regions take minutes to reach;
combat remains the open-ended route. Human pace is not yet playtested.

### The Wolfrun Cart opening (#984)

The opening is now one investigation instead of two wolf quotas: *The
Wolfrun Cart* (explore the Wolfrun Woods, take back three Supply Sacks from
wolves, turn in to the Tanner for 250 XP), then *The Missing Farmhand*. Its
journey, re-measured:

| Opening | Level 2 | Level 3 | Level 4 | First quest turned in | Millbrook lead done |
| --- | ---: | ---: | ---: | --- | --- |
| Wolf quotas (#916) | 0:31 | 2:26 | 4:47 | not printed | 5:16, level 4 (230/1,000) |
| The Wolfrun Cart (#984) | 0:31 | 1:53 | — | 1:53 | 2:21, level 3 (315/600) |

The bot now fights three wolves instead of eleven, so it spends 2:21 instead of
5:16, and the exploring detour still reaches 585/600. At the human rate above
(about half the bot's), the opening takes about 5 minutes of play before
reading the quest text, finding the cart without a waypoint and looting by
hand. For a first-time player that is plausibly the 8–12 minutes #984 aims
for, but live play has not measured it yet.

## Authoritative awards (#75)

The admitted living tapper receives the nominal reward once on the creature's
alive-to-corpse transition, when its XZ distance from the corpse is at most
45 metres (the interest radius). The killer may be a different player or NPC;
tapping decides reward ownership. Missing/dead/out-of-range tappers get no XP. Leaving clears taps so a reused
session-local player ID cannot inherit rewards.
No command grants XP, and duplicate/stale attack sequences fail before rewards.

Level-ups subtract thresholds in order, carry remainder and discard XP at
level 10. Health increases by the maximum-health growth, preserving missing
health; an Arcanist's mana grows the same way by the mana-maximum growth (22
per level), while rage and focus keep their fixed maximum. Existing level-based
melee damage applies automatically. XP and level
are character-durable facts copied into canonical recovery state. Session
identity remains separate; no persistence I/O occurs during awards.

Snapshot schema/wire version 8 carries canonical player XP and exact self
XP/threshold in every projection, so lost cosmetic events cannot erase progress.
The browser decoder accepts this state; the XP bar and level-up feedback display
it through #76. Public API tests kill two level-1 wolves to reach level 2, restore at 50 XP,
continue identically, reject invalid restored XP and cover tapper eligibility.
The Greyhaven wolf-hunt scenario checks XP after its kill, death, reconnect and
spirit release. Earlier snapshots fail closed; see PROTOCOL.md for migration.

## Browser feedback (#76)

The accessible native HTML progress bar displays received current-level XP and
threshold exactly. At the cap it shows a completed bar with “Maximum level”.
It defines no XP curve or grant operation. Level increases between accepted
self snapshots show “You reached level N!” for three seconds, even if
intermediate snapshots or cosmetic events were lost. Initial/restored session
levels produce no false celebration, stale snapshots cannot rewind the display
or refresh feedback, and entering a new session resets prior presentation.

Focused presentation fixtures cover exact XP, skipped levels without events,
stale/duplicate ticks, feedback expiry, cap and reset. Real Chromium checks the
labelled bar and initial authoritative values whenever a game session opens.
