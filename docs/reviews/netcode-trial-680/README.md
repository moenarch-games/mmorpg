# Netcode trial (#680): automated comparison and owner playtest guide

The owner decided (#680 Q1, decision A, 2026-11-14) to keep all three arena netcode options
until a human playtest at 50, 100 and 150 ms simulated round trip picks a default
([PVP_COMBAT.md](../../PVP_COMBAT.md#arena-netcode-trial-680-atlas-59i680),
[BATTLE_ROYALE.md](../../BATTLE_ROYALE.md#netcode-trial-680), Atlas 5.9.i680 / 5.9.i680-2).
This page **supplements** that playtest. Nothing here is the owner's verdict, and it doesn't
pick an option. The numbers come from a scripted bot, not from how anything feels.

1. **Baseline:** zone authority, every unit drawn 67 ms behind, the own body included.
2. **Predicted movement:** the own body moves at once and is reconciled against `acknowledged_sequence`.
3. **Predicted + hit rewind:** option 2, plus capped (150 ms) server rewind of timed-swing hits on
   bodies. Clashes are never rewound.

## What a tester should know first

- **Q is the basic strike (#885), not a timed swing.** It never rewinds and never clashes. Timed
  swings, the only attacks that rewind (option 3) and clash, are the Warden's directed
  **Heroic Strike: hold G, aim, release**. It costs 15 Rage.
- **Draw the weapon first (H).** In Exploration, Q and G do nothing.
- **Rage gates both fighters.** A fresh Warden has 0 Rage. You get 6 per hit you deal, the
  sparring partner 4 per hit it takes. So the partner only starts its Heroic Strikes (every 2.5 s
  within reach) after it has taken about four hits, and it swings far less often than
  BATTLE_ROYALE.md suggests. Open each match with a few Q strikes.
- **Tab doesn't select the partner.** It cycles the Training Targets ahead. Click the partner
  ("Player 2"); selecting it turns the camera and character toward it (focused combat).
- **Option 1 couldn't start in the browser before this PR.** "Left the world after an error: The
  world source has no projection for the joined player": without prediction,
  `NetcodeTrialSource.join` skipped the welcome projection. This PR fixes it
  (`web/src/world/battle-royale/netcode-trial.ts`, with a test). **The live Pages site keeps the
  bug until this PR is merged and deployed.**

## Automated comparison

### Method

- [`measure.ts`](measure.ts) (`cd web && bun ../docs/reviews/netcode-trial-680/measure.ts`)
  drives the Practice route's own `NetcodeTrialSource` over the real `mmorpg-wasm` arena zone
  (`LocalZone.forArenaTrial`, Warden with standard gear, with its page-driven sparring
  partner). It needs no GPU.
- Every cell gets the same fixed scripted input for 3,000 ticks (50 s), in five 10 s cycles:
  - running strafes that change direction every 0.75 s;
  - closing in while strafing;
  - basic strikes every 0.75 s at the moving partner;
  - a standing **trade**: a Heroic Strike on the tick the partner's 2.5 s swing rhythm fires;
  - backing off;
  - closing in to the edge of reach, with a Heroic Strike at 2.15–2.55 m.

  Aim and facing come from where the bot's screen draws the partner, as a player's would.
- Each cell ran twice. All 9 cells produced byte-identical metrics both times
  (`repeatable: true` in [`results.json`](results.json)).
- **Movement**:
  - *input→motion*: after each change of held direction, how long until the drawn own body
    moves the new way at half the speed the zone eventually gives it;
  - *offset*: drawn own body vs the zone's present position while moving;
  - *corrections*: how far each reconciliation moved the predicted body.
- **Hits**:
  - *looked*: on the attacker's screen, the partner stood inside the blow when the projection
    for its resolving tick arrived;
  - *confirmed*: the zone attacked or swept the partner;
  - *outside present*: the zone confirmed a hit although the partner's present position was
    outside the swing, i.e. rewind admitted it.
- **Clashes**: the outcome around each of the 5 scripted trades.

### Results

| Option | RTT | Input→motion median / p90 | Own-body offset mean | Corrections >10 cm · p95 · max | Basic: looked / confirmed / looked-but-missed (of 30) | Heroic began / pressed | Heroic: looked / confirmed / looked-but-missed / outside present | Trades (5) | Clashes | Partner swings |
| --- | ---: | ---: | ---: | --- | --- | ---: | --- | --- | ---: | ---: |
| 1 Baseline | 50 | 117 / 317 ms | 54 cm | — | 25 / 24 / 2 | 9 / 11 | 1 / 0 / 1 / 0 | 2 both miss, 3 no partner swing | 0 | 5 |
| 1 Baseline | 100 | 167 / 267 ms | 66 cm | — | 23 / 21 / 4 | 6 / 12 | 3 / 1 / 2 / 0 | 3 both miss, 1 refused, 1 no partner swing | 0 | 4 |
| 1 Baseline | 150 | 217 / 317 ms | 81 cm | — | 15 / 14 / 1 | 4 / 11 | 2 / 0 / 2 / 0 | 1 both miss, 1 refused, 3 no partner swing | 0 | 2 |
| 2 Predicted | 50 | 17 / 217 ms | 10 cm | 236 · 10.9 · 16.0 cm | 26 / 17 / 11 | 6 / 13 | 1 / 1 / 1 / 0 | 2 both miss, 1 refused, 2 no partner swing | 0 | 3 |
| 2 Predicted | 100 | 17 / 217 ms | 27 cm | 196 · 10.5 · 16.4 cm | 29 / 21 / 9 | 10 / 11 | 3 / 0 / 3 / 0 | 5 both miss | 0 | 5 |
| 2 Predicted | 150 | 17 / 117 ms | 34 cm | 90 · 6.2 · 15.9 cm | 22 / 17 / 10 | 5 / 15 | 2 / 0 / 2 / 0 | 2 both miss, 3 no partner swing | 0 | 3 |
| 3 Pred.+rewind | 50 | 17 / 217 ms | 10 cm | 236 · 10.9 · 16.0 cm | 26 / 17 / 11 | 6 / 13 | 1 / 0 / 1 / 0 | 2 both miss, 1 refused, 2 no partner swing | 0 | 3 |
| 3 Pred.+rewind | 100 | 17 / 217 ms | 27 cm | 196 · 10.5 · 16.4 cm | 29 / 21 / 9 | 10 / 11 | 3 / **2** / 1 / **2** | **2 own hit**, 2 both miss, 1 no partner swing | 0 | 5 |
| 3 Pred.+rewind | 150 | 17 / 117 ms | 34 cm | 90 · 6.1 · 15.9 cm | 22 / 17 / 10 | 6 / 15 | 2 / **2** / 0 / **2** | **2 own hit**, 1 both miss, 2 no partner swing | 0 | 4 |

"Refused" and the gap between Heroic Strikes pressed and begun are `not-enough-resource`: too
little Rage. "No partner swing" means the partner had too little Rage to swing on that rhythm tick.

### What the numbers suggest

These readings are for the owner to weigh, not a recommendation.

- **Movement.**
  - Baseline answers a direction change one round trip plus interpolation later: 117, 167 and
    217 ms median. Its drawn body trails the zone by 54–81 cm while running.
  - Prediction answers in about one tick (17 ms) at every latency.
  - Prediction's cost is small, frequent corrections: up to about 16 cm, 90–236 per 50 s above
    10 cm, with the most at 50 ms. Options 2 and 3 move identically; rewind only changes hits.
- **Hit registration.**
  - Baseline basic strikes almost always agree with the screen: 1–4 of 30 looked like hits but
    missed.
  - With prediction the own body is drawn ahead while other units stay 67 ms behind, so 9–11 of
    30 basic strikes looked like hits that the zone didn't confirm. Basic strikes are never
    rewound, in option 3 too.
  - For Heroic Strikes, rewind confirmed 2 hits at 100 ms and 2 at 150 ms where the partner's
    present position was outside the swing. Options 1 and 2 confirmed 0–1 Heroic hits per cell.
- **Clashes.**
  - No clash happened in any cell. Each trade ended as both-miss, own hit, or no swing from the
    partner.
  - The bot aims at the drawn partner while it circles at 2 m, and a clash needs both whole arcs
    to cover each other in the server's present. Under these scripted trades that never lined
    up. With rewind, trades turned into one-sided hits instead (2 of 5 at 100 and 150 ms).
  - Whether clashes are reachable on purpose at higher latency is a question for the human
    playtest.

### Caveats

- A scripted bot with fixed timing, not a person. A human reacts to the partner's visible
  wind-up, which the bot doesn't.
- "Looked like a hit" approximates what the attacker's screen shows. It uses the whole arc
  (Heroic Strike 2.5 m, ±30°; basic strike roughly the sword chain's 2.55 m, ±50°), ignores
  height lanes, and doesn't model animation.
- "Outside present" uses the whole arc on the resolving tick, so a few could come from the slice
  timing rather than rewind. The pattern appears only in option 3.
- The link is simulated: fixed whole-tick delay, no jitter or loss (as the trial itself does).
  Practice runs locally with one partner.
- Rage scarcity limits how many timed swings and trades happen per cell, so the clash and rewind
  counts are small.

## Recorded sessions (local, not in git)

The same scripted input was played in real Chromium on the built `web/dist` Practice route:
headless with WebGL on ANGLE/Vulkan on the host GPU (RTX 3060 Ti), Playwright `recordVideo`,
1280×720. Script: [`record.py`](record.py)
(`uv run --with playwright docs/reviews/netcode-trial-680/record.py --out <dir>`).

Each video runs about 64 s:

- about 10 s of Loadout and targeting, then about 50 s of play;
- a burned-in caption gives the option and delay;
- the HUD stays visible.

The bot draws the weapon (H), clicks the partner to target it, then runs the timeline:

- running strafes with frequent direction changes;
- six basic strikes (Q) at the moving partner each cycle;
- diagonal runs;
- a standing Heroic Strike (hold G) trade;
- backing off and closing in;
- a Heroic Strike at the edge of reach while moving.

Browser sessions run on wall-clock time, so they aren't tick-identical like `measure.ts`.

Files are in `/home/moenarch/moritzbrantner/netcode-trial-680/`:

- `index.html` plays all nine in a 3×3 grid, with "Play all from start";
- `trial-<baseline|predicted|rewind>-<50|100|150>ms.webm`, plus an `-end.png` frame each;
- `session-log.txt` holds each session's start and end status. The partner ended at 4–48% health
  in every cell; the bot took 22–32 damage in each 150 ms cell.

## Owner playtest guide

### Where

- **Pages:** `https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=<option>&delay=<ms>`.
  Option 1 works there only after this PR is deployed.
- **Locally:** `scripts/fetch-bundles.sh`, then `cd web && bun run dev`. Open
  `http://localhost:5173/mmorpg/?activity=battle-royale&netcode=<option>&delay=<ms>`.
- In the Loadout keep **Practice**, pick the Warden, and **Start practice**.
  **Reset practice (F9)** starts a fresh match with the same choice.

| | 50 ms | 100 ms | 150 ms |
| --- | --- | --- | --- |
| 1 Baseline | [`netcode=baseline&delay=50`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=baseline&delay=50) | [`netcode=baseline&delay=100`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=baseline&delay=100) | [`netcode=baseline&delay=150`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=baseline&delay=150) |
| 2 Predicted | [`netcode=predicted&delay=50`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=predicted&delay=50) | [`netcode=predicted&delay=100`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=predicted&delay=100) | [`netcode=predicted&delay=150`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=predicted&delay=150) |
| 3 Pred.+rewind | [`netcode=rewind&delay=50`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=rewind&delay=50) | [`netcode=rewind&delay=100`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=rewind&delay=100) | [`netcode=rewind&delay=150`](https://moenarch-games.github.io/mmorpg/?activity=battle-royale&netcode=rewind&delay=150) |

### In every cell (about 2–3 minutes)

1. **Setup.** Draw the weapon (**H**), then click the sparring partner ("Player 2") to focus it.
2. **Build Rage.** Land four or five basic strikes (**Q**) so both of you have Rage for Heroic
   Strikes. Watch the partner's Rage bar on its frame.
3. **Movement.** Run strafes (A/D) around the partner, reversing every half second or so. Then
   run forward/back and diagonally. Does your character answer at once? Does it snap, rubber-band
   or slide?
4. **Hits on a moving target.**
   - Basic strikes (Q) while you and the partner both circle.
   - Then Heroic Strikes (hold **G**, aim, release) as it strafes past.
   - Note swings that look like hits but miss, and the opposite.
5. **Clashes.** Stand in front of the partner. Watch for its Heroic Strike wind-up and release
   yours at the same moment, several times. Does a clash ("Clashed") happen when it looks like it
   should? Is it consistent?
6. **Edge of reach.** Back off, then close in and release a Heroic Strike just as the partner
   comes into reach. **This matters most in option 3**, where rewind may land hits that the
   present position would miss. Do they feel fair from both sides?

### Feedback template (one per cell)

```text
Cell: <1 Baseline | 2 Predicted | 3 Pred.+rewind> @ <50 | 100 | 150> ms
Movement responsiveness (1–5, notes: delay, snapping, rubber-banding):
Hit registration (1–5, notes: looked-hit-but-missed / hit-that-looked-like-a-miss, basic vs Heroic):
Attack clashes (did they happen when expected? consistent? 1–5):
Anomalies / unfair moments (what, roughly when, reproducible steps):
```

After all nine: which option(s) should stay, and which should be the arena default?
