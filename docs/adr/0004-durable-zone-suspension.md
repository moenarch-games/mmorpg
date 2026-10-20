# Durable empty-zone suspension

Status: accepted; implements Atlas 1.5 and #144 for the PostgreSQL/S3 host.

## Boundary

`game-server` owns atomic admission/tick fencing, empty-runtime suspension, bounded durability confirmation, eviction and cold hosted admission. MMORPG supplies Greyhaven's host policy and the existing fenced PostgreSQL/S3 recovery adapters. Suspension retains the zone's placement lease and its monotonic permit; it does not retire/reassign zone ownership or grant a second writer.

The host observes continuously empty residence outside simulation ticks. Active and reconnectable slots prevent suspension. A zone supplies base/minimum/maximum grace and optional advisory expected-return hints. The host chooses a bounded proposal; the initial driver has no adaptive heuristic and uses 30 seconds, with an explicit operator proposal clamped to 1–300 seconds. Hints alone never change gameplay or the chosen grace.

At expiry, the shared runtime atomically verifies emptiness, captures one complete boundary and freezes. The storage worker rejects outstanding character claims, nonempty sessions or a recovery image different from its last committed image. It uploads/verifies the immutable checkpoint, commits its fenced reference and only then prunes the journal. Only successful confirmation permits eviction. Failure retains a frozen resident runtime for retry; uncertain/cancelled mutating storage work follows the existing persistence fence. Other zones retain their own runtime and journal boundaries.

## Cold admission and time

The cold resolver restores only configured, successfully suspended zones. It checks the current permit, loads the referenced object, replays the retained PostgreSQL suffix and checks exact agreement with the committed suspension boundary. The existing adapter and `MatchRuntime::restore_from_recovery` validate authoritative content, sessions and canonical state before placement. The initial durable recovery basis already exists; cold placement does not bootstrap a new world.

The lifecycle layer measures real elapsed suspension with `Instant` at the actual freeze callback and supplies `ResumeInput` explicitly. `frozen_resume` preserves the entire image by default. All existing Greyhaven schedules, respawns, raids, preparation and recovery timers therefore freeze. No production subsystem opts into offline progression. `OfflineTransition` defines the separate deterministic, bounded, transactional per-subsystem seam; its test subsystem demonstrates replay from the same checkpoint/input without running skipped ticks. A future gameplay opt-in must own its semantics and durable resume input, rather than treating this trait as blanket approval.

Cold admission is correct with prewarming absent. No portal predictor or prewarming scheduler is enabled. All restored mutations still use the existing publication gate, character leases and epoch fencing. A failed cold read refuses entry and remains retryable, without anonymous/fresh-world fallback. No protocol, canonical, checkpoint-envelope or SQL format change is needed.

## Scope and evidence

The durable PostgreSQL/S3 host unloads empty zones. Development file hosts and the browser-local WASM host remain resident; they do not acquire an object-store lifecycle implicitly. The generic live-host status reports resident runtimes: an unloaded zone may be absent from per-match status until cold admission, while process readiness remains the existing live-host transport/capacity fact.

Verified with policy/explicit-resume tests, a warning-free PostgreSQL-feature compile, and one real replicated-PostgreSQL/RustFS/WebTransport cycle proving actual eviction, exact frozen recovery and cold session admission without prewarming. Upstream runtime tests additionally cover occupied/reconnectable refusal, failed confirmation, cancellation and concurrent cold admissions. Deployment-scale adaptation and a production offline gameplay opt-in are not claimed.
