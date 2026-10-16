# Native world presentation parity (#182)

Independent acceptance authored before production, against `f25cfe71e720b72c86aa35e143e979c3603086cf`.
The published Pages reference is successful run `38957949724`, source `6548f85`;
relevant browser/native source is unchanged between that revision and this baseline.
Root verified every served artifact byte against the published artifact. Local
`web/dist` contains parked #27 work and is **not** this reference.
Shared convention sourceRevision: `5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.

Owner-approved split: #182 owns the complete current world/models/environment;
#183 owns the complete native entry, HUD, panels and interactive UI. Neither
scope is replaced by clearing caches, a browser launcher, or a color-count smoke.
Current published viewer ability poses/effects belong to this world scope;
unpublished #27 remote/ordinary action observations do not.

## Executable mapping

| Obligation | Independent evidence |
| --- | --- |
| Six class/sex appearances | `native_presentation_parity`: real core choices → actual submitted unit draws; historical SceneBox baseline collapses all 15 pairs |
| All current class/sex/headwear combinations and neutral remote look | `native_unit_frame_parity` + current Pages `unitModel`/`characterLook` generators; invalid headwear rejected |
| Every current creature template, elite appearance, NPC role | Same frame comparison with actual catalog/spawn identities; hostile/neutral, tapped, selected and initial corpse cases; GPU gallery images |
| All 34 current prop kinds, not terrain-only success | `native_world_reference`: each prop-kind world differs from an equivalent prop-free world; actual draw comparison; all-kind GPU views |
| Authored grass/relief, structures, terrain, roads/shore/water/far ring | Actual shared scenery export through current Pages generators; prop/region frames, material and placement comparisons; existing world/geometry tests; paired region images |
| Normalized Ranger asset | Exact authored mesh topology/attribute digest and normalized asset identity on actual submitted draws; material/transform comparisons; actual class images |
| Fog, sky gradient, hemisphere light, sun and full cosmetic day/night | Five cycle phases, repeat endpoint, frozen phases; environment/sky-stop comparison; real GPU lighting/fog/day-night mutations and paired day/night images |
| Opacity, vertex colors, emissive and unlit materials | Actual draw material comparison; real GPU color/opacity/emissive/light mutations; lamps/runes/staff and water visible in paired images |
| Sails/flames/reeds and shared immutable geometry | Animated/frozen clock frames and material/model comparisons; same-resource `Arc` identity retained across frames |
| Walk/jump/turn and reduced motion | Animated/frozen unit input cases against current Pages `UnitAnimators` and actual native submitted draws |
| Published viewer sword/bow/staff ability poses, casting/channeling/interruption/release and spell visuals | Abilities 1–12, cast/channel/interrupted/released and barrier-aura fixtures through actual Pages `SpellEffects.spawn/sync/viewerAction`; projected native events/cast/aura and reduced motion |
| Real renderer consumes the inspected frame | Explicit GPU mesh/material/transform/environment/lighting/fog/opacity/emissive/vertex-color mutations must change actual readback pixels |
| Selection, health bars, occlusion, camera, focus, reconnect/session retirement | Retain `crates/mmorpg-client/tests/native_pointer_contract.md` real-window application journey, `presentation.rs` health/interpolation/reset tests, `connected_world.rs`, native controls tests, and existing GPU health-bar test; independently review migrated picking consumes the actual presented geometry/transforms/sidedness/visibility, including instances |
| Source/build identity and local edits | Reuse #180 launcher acceptance `scripts/test-dev-native.py` plus actual launcher smoke; preserve executable/source identity in completion evidence |

`native_world_reference` runs the independent current-browser generators, not a
native producer or copied implementation. It stores geometry digest/bounds/counts
once per resource and only references/transforms/materials per scene. Per-binary
`OnceLock` shares its export. No generated export or image is committed.

Procedural geometry bounds and nonempty topology **do not prove shape parity**.
A cube with the same bounds could satisfy those assertions. Exact digest matching
is required for the normalized authored asset, not arbitrary procedural
backend tessellation. Therefore independent paired native/browser image review
of **all current prop categories, creature families/elite, NPC roles and six
player looks**, plus regions and day/night, is a required completion gate.
Review silhouettes, material/color, placements, terrain/road/shore, sky, readable
night lighting, glow and animation. Backend pixels need not be identical. Record
any mismatches and fix them; GPU input mutations alone do not prove model parity.

## Focused commands

Use an isolated `CARGO_TARGET_DIR`; never overwrite a running user's executable.

```sh
cargo test -p mmorpg-client --test native_world_reference --locked -- --nocapture
cargo test -p mmorpg-client --test native_draw_identity --test native_world_frame_parity --locked
cargo test -p mmorpg-client --test native_presentation_parity --test native_unit_frame_parity --locked
MMORPG_SMOKE_FRAME_DIR=/tmp/mmorpg182-final-frames cargo test -p mmorpg-client --lib world_gpu_acceptance --locked -- --ignored --nocapture --test-threads=1
```

The GPU helper must use the production `SceneRenderer` and return its actual
640×360 RGBA readback. No alternate acceptance renderer is permitted. Capture
actual native-window behavior through the existing pointer application contract,
then run actual `scripts/dev-native.sh --no-update --smoke` and existing
`scripts/smoke-native.py`. Follow the native pointer contract's required X11/GPU
setup and retain all existing authority/dependency, protocol and full CI gates.

Baseline evidence: early real appearance reproduction `d3c6cdf`, 0 passed/1 failed,
0.09 s; all six class/sex payloads identical. Full agreed frame/GPU contract is
red because the baseline lacks the application frame API; this is **not** a
passing full acceptance claim. The independent reference inventory is green.
Root retained actual legacy native hub/relief frames and actual public Pages hub
at eye `[5,8.5,31]`, target `[-14,3,4]`; those exhibit the reported gap. Full
post-implementation paired review and real-window verification remain required.

Instance draw comparison normalizes only the exporter/native cache-identity envelope
inside the explicitly recognized `instances:<chunk>:<cull>:scenery:...` or
`instances:<chunk>:<cull>:native-scenery:...` grammar. Chunk, cull class, geometry
ordinal and instance ordinal remain exact, and canonical duplicates fail closed
on both sides. Unit/static/animated IDs, actual resource keys and asset identity
are unchanged. World and unit differentials are separate required binaries so
world implementation can be tested without temporary unit API stubs.

The unit reference must query shared relief **per entity position**, for both body
placement and spell anchors, exactly as production `WorldView` calls
`scenery.reliefAt`. The Rust fixture bridge samples the public WASM export's
backing `mmorpg_wasm::scenery::relief_at(x,z)`; missing samples fail rather than
falling back to zero. This corrects an oracle translation defect without changing
model/material/transform/identity expectations or adding a native-only offset.

`scripts/test-dev-native.py` additionally exercises edits, missing-input build
failure and restoration for the actual embedded OBJ/manifest/material paths.
Generated `web/dist`, WASM binding and dependency outputs must not trigger loops.
Its fake Cargo proves watch-input classification and recovery only; actual
locked native build and launcher smoke remain separately required.

Remaining adapter acceptance concerns: preserve projected health bars on the new
`unit_draws` path and migrate the existing GPU health-change proof to that actual
render path; retire dynamic effect geometry on expiry/reset (for example changing
Barrier bubble radii) rather than retaining every sampled mesh indefinitely.
These require proportionate behavioral regression evidence, not new size budgets.


### Actual session camera acceptance

`cargo test -p mmorpg-client --test native_session_camera --locked` compares
first-session heading priority, published default pitch 0.36, actual spawn,
rendered-terrain eye clearance, repeated projections and session reset against
an independent browser-policy oracle. Expected production seams are
`Presentation::camera_view(&mut OrbitCamera, &WorldScene, Instant) -> CameraView`
and `WorldScene::surface_y(x: f32, z: f32) -> f32` (the existing rendered terrain
triangle interpolation). The real App render and six-look GPU gallery must use
this same session-lifecycle method. The existing actual-window pointer contract
also checks initial App framing and preservation of later user orbit. Bare
`OrbitCamera::default().view` math tests remain useful but do not establish
session initialization parity.

The six default-spawn class/sex GPU frames must still differ; relocating the
actor or applying a gallery-only view is not an acceptance repair. Image mismatch
diagnostics report byte count without dumping whole buffers. Original failed
captures/logs remain evidence. Pointer occluder and new-path health-bar fixture
translations remain required in a subsequent independent acceptance turn.

### Final adapter fixture translations

`native_session_camera` additionally verifies `surface_y` against barycentric
interior points of both actual 4m terrain-grid triangles in the steepest and
highest cells. This is independent of the sampler formula. No-viewer frames
must leave startup initialization pending. Default class appearance is the
published entry selection: Warden ironcrest helm, Ranger cap, Arcanist hood.
`Presentation::set_default_appearance(PlayerClass)` supplies that existing
cosmetic input in actual App startup and gallery; generic `new` and explicit
`set_headwear(None)` retain their meanings.

The actual-window pointer fixture now supplies its same wall, roof and triangle
through `WorldScene.additional_draws`, with identical positions, dimensions and
triangle winding; the projection of pointer coordinates uses the same grounded
session camera as App. Existing occlusion assertions remain unchanged. The
existing `projected_health_bars_change_gpu_pixels` test now renders actual
`unit_draws` through `offscreen_frame_pixels`; its pixel-change assertion is
unchanged.

`cargo test -p mmorpg-client --test native_presentation_resources --locked`
checks dynamic Barrier radius geometry with Weak resource handles: old unused
radius resources retire, authoritative aura removal retires expired resources,
and session reset releases remaining resources. An accepted ability event alone
is not a continuously active Barrier; the fixture supplies/removes the actual
Absorb aura.

The explicit GPU test `empty_frame_renders_submitted_background_without_hidden_css_sky`
checks the whole empty frame against its submitted sRGB color (one output-byte
quantization allowance), verifies changing background changes real pixels, and
verifies inactive CSS sky/cloud values do not replace it. The named region
captures reuse the actual browser DEBUG_VIEWPOINTS; eye clearance is applied if
needed. Every world/prop camera is printed as `GALLERY_CAMERA` JSON for exact
public reproduction. Well/barrel cameras approach the actual subject instead
of sitting behind nearby architecture. Required postimplementation GPU captures
and actual paired image review remain obligations; passing compilation is not
visual evidence.
