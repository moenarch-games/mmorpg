# Starter progression rules

Issue #74 defines the immutable rule queries in `mmorpg-core`. Issue #75 applies them on authoritative creature deaths and persists/projects
progression. Issue #76 owns the browser XP bar and feedback. The rule queries
themselves never grant XP.

Player progression supports levels 1–10. The next-level thresholds are
current-level XP, not cumulative totals:

| Current level | XP to next level |
| --- | ---: |
| 1 | 100 |
| 2 | 200 |
| 3 | 300 |
| 4 | 400 |
| 5 | 500 |
| 6 | 600 |
| 7 | 700 |
| 8 | 800 |
| 9 | 900 |
| 10 | 0 (cap) |

The total from level 1 to level 10 is 4,500 XP. The future authority consumes
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

Both public queries return `None` for unsupported levels. The capped threshold
is `Some(0)`, distinct from invalid input. No client command can call these
queries to mutate XP. The authority applies these rules and rejects invalid persisted progression;
#74 introduced no snapshot changes and #75 deliberately versions them.

Exact public-API fixtures cover the curve, rounding, gray rewards, bonus cap,
player cap and invalid inputs. Conventions sourceRevision:
`46d8793bb3034326561f876dcc67dbaa5aa1e432`.

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
