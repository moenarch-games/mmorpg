# Native imported-mesh allocation observation (#338)

**Confirmed buffer churn; gameplay frame-rate impact remains unmeasured.** The normal `Presentation::unit_draws` path calls `Model::draws`/`draws_placed` for each imported unit every presented frame. CPU skinning creates a new `Mesh`/`Arc` for every primitive, including when clip time is unchanged. `FrameRenderer::render` keys by stable resource name but compares the mesh's weak pointer; every replacement misses that comparison. Each miss creates both vertex and index buffers and uploads the entire posed mesh. This report measures that same materialization and actual GPU upload branch, without modifying rendering behavior.

## Reproduction and environment

```sh
bash scripts/fetch-bundles.sh
cargo test -p mmorpg-client --locked --lib imported_mesh_buffer_observation -- --ignored --nocapture
cargo test -p mmorpg-client --locked --lib unchanged_imported_pose_rebuilds_mesh_identity -- --ignored --nocapture
```

Measured source: `ff3db97` (base `048a83015deaf13cbaf57a5472480dbe29ecb582`), committed Cargo.lock; Rust 1.98.1 (`48a229cea`, LLVM 22.1.8), `x86_64-unknown-linux-gnu`, default features, unoptimized development/test profile, no task-specific RUSTFLAGS. Machine: AMD Ryzen 7 5700X, 16 logical CPUs; NVIDIA GeForce RTX 3060 Ti, Vulkan, NVIDIA driver 595.91.07. Shared interactive machine; timing is advisory. Convention source: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.

The bounded workload uses the existing pinned Ranger body (13 primitives), existing idle clip, fixed camera, 640×360 target and 1, 10, 50 placed units. Import/device setup is outside measurements. Each mode has three frames: first upload, then two continuations. Animated time is 0.25 + frame/30 seconds; unchanged time is always 0.25. Retained control clones the same draw meshes, preserving Arc identity. Held equipment, scenery meshes, HUD and normal game/network updates are excluded to isolate the boundary. The actual world renderer submits every frame and waits for device completion. This is not a capture of a whole interactive session or a production frame-rate benchmark.

## Deterministic work per continuation frame

Animated and unchanged-pose modes produce identical counts:

| Characters | CPU mesh creations | GPU buffer creations | Uploaded mesh bytes | Live cached meshes |
| --- | --- | --- | --- | --- |
| 1 | 13 | 26 | 1,373,616 | 13 |
| 10 | 130 | 260 | 13,736,160 | 130 |
| 50 | 650 | 1,300 | 68,680,800 | 650 |

Each animated primitive creates one CPU mesh and two GPU buffers each frame; the mean payload is about 105,663 bytes per primitive (primitive sizes differ). Retained controls create/upload the same GPU resources on frame zero, then create **zero buffers**, upload **zero mesh bytes** and reuse 13/130/650 meshes per continuation. CPU mesh creation in the retained control is zero inside the timed region; its initial meshes are prepared outside it.

Counts are taken at the actual successful upload/cache-reuse branches. Uploaded bytes mean vertex/index payload, excluding uniforms, instance updates, driver bookkeeping and physical bus traffic. Live mesh count means entries in the application's retained GPU cache, not actual driver allocation residency or peak VRAM. Replacement keeps the cache bounded; this demonstrates churn rather than an accumulating leak.

## Advisory timings and recommendation

[Raw observations](observation.txt) retain each frame's CPU materialization time, render submission time and submission-to-device-completion time in microseconds. In this debug run, 50 changing-pose characters took roughly 0.54 s to materialize and 0.18 s to submit/complete each frame; the retained control submitted/completed in roughly 3 ms. These measurements include debug validation/allocation work and are not release performance, isolated GPU timing, or an expected player frame rate. No threshold or CI gate is added.

The repeated full index/vertex uploads scale linearly and are large enough to warrant a local retained-buffer follow-up. The smallest remedy is stable per-instance/per-primitive GPU buffers with bounded vertex updates for changed poses, retaining static indices/material colors and avoiding CPU rematerialization for identical clip/time/tint inputs. Keep equipment and disappearing instances' lifetime explicit. CPU skinning cost would remain and needs separate release measurement before proposing GPU skinning. This investigation changes neither the animation/renderer architecture nor browser/gameplay/physics authority; implementation of a remedy is outside #338.
