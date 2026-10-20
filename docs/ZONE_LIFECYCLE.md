# Empty zones and suspension

The PostgreSQL/S3 zone host waits **30 seconds of continuous empty residence**, including the existing reconnect grace, then freezes the zone at a completed authoritative boundary. A confirmed checkpoint permits its live runtime to unload. The zone keeps its fenced placement lease; storage workers and encoded recovery evidence remain available. A later direct connection cold-loads the checkpoint and journal and admits through the existing session path.

`MMORPG_EMPTY_ZONE_GRACE_MS` proposes an alternative grace in milliseconds, clamped to **1,000–300,000 ms** by Greyhaven's zone policy. Without it there is no adaptive adjustment. The public host-policy seam also accepts zone-owned advisory expected-return hints; no hint, memory-pressure heuristic or load heuristic automatically changes the initial policy. Host proposals always remain within the zone bounds. The lifecycle loop observes once per second, outside gameplay ticks.

The transition logs `zone_suspended` with the tick/effective grace and `zone_resumed` with explicit real elapsed time and `progression=freeze`. Failed confirmation logs `zone_suspension_waiting` and retains the frozen writer until a safe retry or existing persistence failure handling. A failed cold restore rejects admission and remains retryable. It never starts a fresh or anonymous replacement world.

All gameplay **freezes by default**: NPC schedules, creature respawns, encounter/raid recurrence, preparations, recovery and other simulation-timed state resume from the preserved tick. `ResumeInput` carries measured elapsed time; `OfflineTransition` is an explicit deterministic bounded subsystem contract, with no production Greyhaven opt-ins. No wall clock is introduced into core. A future gameplay opt-in must define and persist its own authorized resume transition; this infrastructure does not approve offline raids or catch-up simulation.

Cold restore is the correctness path. There is no prewarming scheduler to enable or require. Ordinary file-development hosts and browser-local WASM remain resident. Server status describes resident matches; a suspended zone can be absent from `/matches/<id>/status` until cold admission even though its configured route can restore it.

The [architecture decision](adr/0004-durable-zone-suspension.md) describes checkpoint/lease ordering and scope. No snapshot/command/canonical/recovery format or SQL migration changed. `game-server` revision `a3cdf8c8bf915167988de2ef5e1f9b39cf847d13` provides the shared atomic empty-match and cold-admission hooks.

## Focused verification

```sh
cargo test -p mmorpg-game-server --locked --lib lifecycle
```

For the explicitly selected durable proof, configure `MMORPG_CHARACTERS_POSTGRES_URL`, `MMORPG_POSTGRES_STANDBY_URL`, `MMORPG_CHECKPOINT_S3_ENDPOINT`, `MMORPG_CHECKPOINT_S3_BUCKET` and the S3 credentials for disposable replicated PostgreSQL/S3 services, then run:

```sh
cargo test -p mmorpg-game-server --features postgres --locked --lib durable_empty_zone_unloads_freezes_and_cold_admits_without_prewarming -- --ignored --nocapture
```

This proof allocates a dedicated fenced test-zone stream, starts a real WebTransport listener on an OS-selected port, actually unloads the empty runtime, reads and restores the same authoritative state from object/journal evidence, and cold-admits a real client without prewarming. It is opt-in and does not run in normal CI. The initial local run used PostgreSQL primary/physical standby plus RustFS and passed. Shared convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.
