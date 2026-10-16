# Native library size evidence

The opt-in native pilot measures `mmorpg-core`, the maintained gameplay library used by the host and both clients. It counts the raw bytes of `target/x86_64-unknown-linux-gnu/release/libmmorpg_core.rlib`, including Rust archive metadata. This is neither linked executable size nor a runtime, memory, download-size or per-symbol measurement.

From the repository root with Linux x64, Rust from `rust-toolchain.toml` and Bun 1.4.2:

```sh
bun install --cwd web --frozen-lockfile
bun run --cwd web typecheck:size
bun run --cwd web size:budget
bun run --cwd web test:size-evidence
```

`size:budget` builds the library with Cargo's locked release profile, no default features, the explicit GNU Linux x64 target and the repository-local target directory. It remaps the checkout prefix to `/mmorpg` so checkout path length does not inflate the archive. Ambient compiler controls (`RUSTC` and `RUSTC_*`, including bootstrap), wrapper, incremental, Rust-flag, release-profile and `RUSTUP_TOOLCHAIN` overrides are unavailable, rather than silently changing the declared build. Inherited Cargo configuration is unsupported unless it contains only dev/test profile settings, which cannot affect this release build. Cargo and rustc must pass bounded availability probes before compilation. Other host platforms are currently unavailable for this pilot.

`.performance/size.json` selects that one archive and records the target, profile, features, Rust/Cargo versions, Cargo manifests and lockfile, toolchain declaration, producer script and source-package dependency graph. The committed baseline in `.performance/baselines/mmorpg-core.json` retains the exact bytes and SHA-256. Source edits are candidates rather than comparability inputs. Compiler, target, feature or declared build-input differences are incomparable, not improvements or regressions; missing tools, baselines and artifacts are unavailable.

The initial archive budget was 2 MiB (raised to 3 MiB by #112, below), with at most 64 KiB growth against a compatible reviewed baseline. These limits provide headroom over the initial approximately 1.65 MiB archive while catching accidental growth. They are an explicit pilot decision, not an existing runtime or fleet-wide performance guarantee. The shared collector comes from the public `coding-tooling/size-evidence` source export at revision `122ad55cbc5fd82233f2aa4af17278554db5cc78`; package publication is unnecessary.

The script typecheck uses Bun's native `bun-types` provider for TOML parsing and declares its Undici type import explicitly. It preserves package symlinks so the isolated Bun dependency graph resolves through this consumer, with every declaration still checked.

Ordinary gates never update the baseline. To propose an intentional update, capture to a disposable file, require a passed result, review its build identity, bytes and hashes, and then replace the committed baseline in the same reviewed change:

```sh
bun run --cwd web size:baseline:capture > /tmp/mmorpg-core-size-candidate.json
```

A capture explicitly has no requested comparison; it proves only the absolute budget. Changing build flags requires matching declaration and producer changes before capture. The acceptance command exercises the real built archive and public collector through compatible comparisons, target/feature mismatches, missing inputs/tools, a growth failure and preservation of the committed baseline. It also proves the producer refuses ambient flags. CI repeats the build, budget comparison and acceptance command and retains `artifacts/native-size.json`.

This completes one native-library adoption for coding-tooling#160. It does not roll the capability out to the native client, zone-host executable, WASM bundle or other repositories.

Resolved shared policy sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.


The #83 inventory/schema-v7 change intentionally advances the reviewed baseline
to **1,818,308 bytes**, SHA-256
`ffc794c54ef39595ae7e11be36ad441aeeee520cdeb38de0e1b98dd29529bbee`.
Its build identity is identical to the initial 1,736,620-byte baseline. The
81,688-byte cumulative increase exceeded the 65,536-byte growth gate; the failed
comparison is retained in
[the raw observation](../.performance/observations/inventory-v7-growth.json).
The preceding #82 pure catalog/bag archive measured 1,784,858 bytes, so #83 adds
33,450 bytes for authoritative bags, queued moves, snapshots and declared RNG
seed identity. The new APIs and stored records explain expected feature growth;
this archive includes Rust metadata and is not a linked-code cost estimate.
The absolute 2 MiB and per-baseline 64 KiB budgets remain unchanged. The explicit
capture passed the absolute budget, and the ordinary gate never updated evidence.

The #102 atomic loot settlement change advances the reviewed baseline to
**1,893,188 bytes**, SHA-256
`1111e9b8119b7f18b6f2895326e74d420ff2a454b062a58888d05eba75b1b0c5`.
Its complete build identity matches the inventory baseline. The cumulative
74,880-byte increase exceeded the unchanged 65,536-byte growth gate; the
[failed observation](../.performance/observations/loot-settlement-growth.json)
retains the exact comparison. The preceding #88 loot catalog measured
1,880,316 bytes, so this public settlement function and typed error add 12,872
archive bytes. These are raw archive/metadata bytes, not a linked-code or runtime
cost estimate. Explicit capture passed the unchanged 2 MiB absolute limit;
the ordinary gate and its adversarial acceptance checks do not update baselines.

The #103 corpse authority/schema-v8 change advances the reviewed baseline to
**1,971,554 bytes**, SHA-256
`e02b7da4676de49a783d3959fa50793b3cab882847d5922daf452d1f3b791bc2`.
Its complete build identity and inputs match the #102 baseline. The cumulative
78,366-byte increase exceeds the unchanged 65,536-byte growth gate;
[the failed comparison](../.performance/observations/corpse-loot-v8-growth.json)
retains the observation. The preceding #105 content-binding archive measured
1,925,682 bytes, so corpse generation, claim authority and canonical public
records add 45,872 archive bytes. Raw archives include Rust metadata;
this is not a linked-code or runtime cost estimate. Explicit capture passed the
unchanged 2 MiB absolute limit. The ordinary comparison and adversarial acceptance
checks retain both budgets and never update evidence automatically.

The #112 classes-and-abilities/schema-v9 change raises the absolute budget once,
by owner decision, from 2 MiB to **3 MiB** (`maxBytes` 3,145,728 in
`.performance/size.json`) and advances the reviewed baseline to **2,546,286
bytes**, SHA-256
`9f27c3074126edfa71d48b7c72d01f8ad1ec0be801e4e1aa3a8233edb771f00a`. The ability
catalog, casting, auras, class resources and their canonical and projection
records add 574,732 archive bytes (metadata 1.03 → 1.43 MB, object code 0.84 →
0.99 MB), in proportion to the added code, so the archive could not fit 2 MiB.
[The failed comparison](../.performance/observations/class-abilities-v9-growth.json)
retains the observation against the #103 baseline. Its complete build identity
matches that baseline. The 64 KiB per-baseline growth gate is unchanged and applies
to later changes; the explicit capture passed the new absolute limit.

The #66 equipment/schema-v10 change advances the reviewed baseline to
**2,650,028 bytes**, SHA-256
`9140b9ba2aa02530572917c4bf029611152650d7ef42dc704a9cb9d21d38f4a6`. Its complete
build identity matches the #112 baseline. Equipment slots and stats, the equip
and unequip intents, gear-aware health and damage, the larger item and loot
catalogs and their canonical and projection records add 103,742 archive bytes,
above the unchanged 65,536-byte growth gate;
[the failed comparison](../.performance/observations/equipment-v10-growth.json)
retains the observation. These are raw archive/metadata bytes, not a linked-code
or runtime cost estimate. The explicit capture passed the unchanged 3 MiB
absolute limit, and the ordinary gate never updated evidence.

The #67 vendor/schema-v11 change advances the reviewed baseline to
**2,753,096 bytes**, SHA-256
`3f02fb2d7487a908c8112f932339ddf82d8c72c1d1d64217cb185358abc9d5a8`. Its complete
build identity matches the #66 baseline. Vendor stock validation, sale values,
the buy and sell intents and their settlement, the content binding and the
canonical trade intent records add 103,068 archive bytes, above the unchanged
65,536-byte growth gate;
[the failed comparison](../.performance/observations/vendor-v11-growth.json)
retains the observation. These are raw archive/metadata bytes, not a linked-code
or runtime cost estimate. The explicit capture passed the unchanged 3 MiB
absolute limit, and the ordinary gate never updated evidence.

The #25 quests/schema-v14 change advances the reviewed baseline to
**3,128,164 bytes**, SHA-256
`8d3dc2c716586bc395f02223bc79edf107e75e8239c94675f5e7f04e78f0f4a2`. Its complete
build identity matches the previous baseline (2,834,026 bytes). Quest content
validation and fingerprinting, the quest log, accept/abandon/turn-in settlement,
kill/collect/talk/explore progress, markers, quest drops and the nine authored
Greyhaven quests with their dialog text add 294,138 archive bytes, above the
unchanged 65,536-byte growth gate;
[the failed comparison](../.performance/observations/quests-v14-growth.json)
retains the observation. These are raw archive/metadata bytes, not a linked-code
or runtime cost estimate. The explicit capture passed the unchanged 3 MiB
absolute limit with 17,564 bytes to spare, so the next content step will need a
deliberate budget decision.

The #72 native ability input/HUD change adds the client-only
`input-bindings-core` git dependency. That changes `Cargo.lock`, a declared build
input, so the comparison against the #25 baseline was incomparable rather than a
size change. `mmorpg-core` itself is untouched: the recaptured baseline is
**3,128,246 bytes** (+82 against #25), SHA-256
`194dd6bf2ac1d0c86710ee1b380a46e21641b33ffd07c1a42970a4e0fb99cbde`. Only the
`Cargo.lock` input hash differs from the previous identity; target, profile,
features, toolchain and the other inputs are unchanged. The explicit capture
passed the unchanged 3 MiB absolute limit.

The #28 native pointer change intentionally advances the shared Rust/browser
`3d-lab` pin. Its `Cargo.lock`, `web/bun.lock` and `web/package.json` changes
alter declared build inputs, so the old baseline comparison is incomparable.
[The original CI report](../.performance/observations/native-pointer-28-incomparable.json)
retains that unavailable result from PR #169 head
`2e93fb6bade114aa41116321fe0f74ff3bde8a92` (run 38878740294, job
116900073110). It reports **3,128,246 bytes**, with no size-limit failure.
The explicit local recapture has the same size and archive SHA-256 as the old
baseline, `a96d72ce2170065e8f46cfe2f7ceb4bb844f0a8734c89901a477f958ccd76819`;
only the three declared input hashes and resulting identity change. Core source,
its dependency closure, target, profile, features and toolchains are unchanged.
The 3 MiB absolute and 64 KiB growth limits remain unchanged. CI must still
pass the ordinary comparison and existing adversarial acceptance command.

The #166 development-only `coding-tooling` update changes the declared web
manifest and lockfile inputs. The retained
[CI observation](../.performance/observations/coding-tooling-166-incomparable.json)
reports an incomparable baseline, with no size-limit failure. The explicit
recapture has the same **3,128,246 bytes** and archive SHA-256 as the #28
baseline; only those two input hashes and the resulting identity change.
The 3 MiB absolute and 64 KiB growth limits remain unchanged.

The #167 `game-server` update changes the declared Cargo manifest and lockfile
inputs, making the previous comparison incomparable. The retained
[observation](../.performance/observations/game-server-167-incomparable.json)
records that result. The explicit recapture remains **3,128,246 bytes**, with
the same archive SHA-256 as the #28 baseline. Only those two input hashes and
the resulting identity change; the 3 MiB absolute and 64 KiB growth limits
remain unchanged.

After merging #166 into #167, the baseline was explicitly recaptured with both
dependency updates present. The combined identity changes all four Cargo/web
manifest and lockfile hashes against #28; archive bytes and SHA-256 remain
unchanged. The [combined observation](../.performance/observations/game-server-167-combined-incomparable.json)
retains the unavailable comparison against the merged #166 baseline.
