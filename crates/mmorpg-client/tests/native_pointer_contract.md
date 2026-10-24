# Native pointer and NPC application acceptance (#28)

Independent acceptance context `/root/native_acceptance` authored this contract
against `28ce451f2e118c6a664117d8d6d9071dc60bb954`, before production changes.
Authority: the current `docs/decisions/atlas.md` 5.1.11, 5.2.1, 5.2.16, 5.4.18,
6.1.1 and 6.3.1–6.3.3; issue #28 and its full independent obligation handoff
[6211062131](https://github.com/moenarch-games/mmorpg/issues/28#issuecomment-6211062131).
The geometry prerequisite is 3d-lab #125, merged in #126 at
`df52c324a5692d04d2016d8a29f5bc4368e4aac0`. This acceptance commit does not adopt it.
Convention resolution: `sourceRevision=5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.

## Executable application boundary

Run on Linux X11 with Python 3, libX11/libXtst and a working wgpu adapter:

```sh
python3 ~/.local/share/coding-agent/disk-maintenance.py run -- cargo test -p mmorpg-client --bin mmorpg-client native_pointer_application_contract --locked -- --ignored --nocapture
```

`support/native_pointer_application.rs` is included only in desktop binary tests.
It creates the **real App, real winit event loop/window, real WindowRenderer GPU
surface, real ClientSession/run_session and real TLS/WebTransport zone host**.
Pointer motion, button presses/releases and focus/leave events are explicit
winit `WindowEvent`s dispatched into the same live `App::window_event` with the
real `ActiveEventLoop`. This exercises application interpretation, not OS pointer
routing or physical hardware. Keyboard input originates in XTest OS events;
an assertion requires those key events actually arrive through winit. Neither
source claims physical mouse/keyboard hardware coverage. There is no alternate
picking implementation, fake projection result, fake NPC transaction, or local
quest/vendor rule. The public winit/application boundary is the exercised input
seam; private inspection is confined to the existing App and test fixtures.

The test checks the native **window title** for visible identity, actual role
context, reward choices and refusal feedback, and requires successful surface
frames after authoritative selection. It does not call an isolated adapter draw
as its feature proof. Existing native smoke separately reads back GPU pixels. The test also logs and
rejects CPU adapters for a same-options compatible-surface request on the actual
window; this metadata probe is separate from the App surface frames.
Native controls in this executable contract are Enter for selected-NPC context,
1 for its displayed first quest/buy action, 2 for a merchant's displayed first
bag-slot sale, Right for the next displayed reward choice, and Escape to cancel
the context/armed pointer. Equivalent discoverable bindings/feedback locations
are routine UI choices, but changing this contract's inputs/observations requires
independent re-verification of the same journey; do not silently weaken it.

For the additional OS pointer qualification, run the **same** complete contract
under a controlled X11 display with `MMORPG_NATIVE_POINTER_OS=1`. That mode uses
XTest pointer/key events and asserts their ordinary-winit delivery; it never
falls back to direct dispatch. All scenarios/outcome assertions are identical.
The default handler mode is useful integrated evidence, but does not discharge
that remaining actual OS-pointer proof on its own. On the current `:0` Wayland /
XWayland session, OS pointer delivery remained unavailable after bounded checks;
provisioning an isolated X11 display is a harness option, not missing GPU evidence.

## Fixture transparency

The host uses the actual immutable Greyhaven world definition, scenery, NPC
identities/names/roles, quest and vendor catalogs, plus original loot and ability
catalogs. Only creature wandering/aggro are disabled in the fixture. Canonical
restore sets the player near each **actual NPC**, or beyond reach, and places two
actual creature bodies for separated/overlap cases; the ordinary core projection
and actual renderer then display that state. Setup grants 20 copper so a real
purchase is reachable, resets quest state between independent NPC scenarios,
and supplies earned objective progress/pelts for actual completion/reward-choice
transactions. These are declared setup inputs, never substituted outcomes.
Solid wall/roof/terrain-occlusion fixtures are uploaded to the same real renderer
and queried by the application. A host projection freeze provokes the actual
session stall/reconnect path; the real host continues ticking and owns resume.

## Complete obligation mapping

| Obligation | Independent application case or preserved evidence |
| --- | --- |
| Exact visible creature, second identity, no implicit attack | Dispatch actual winit cursor/button events at farther creature 221 then 220 by rendered torso coordinates; actual host confirms exact EntityRef, native title names it, GPU frames advance, auto-attack remains false. |
| Camera drag, overlapping bodies, empty ground | Real-handler winit drag changes camera without selecting; collinear humanoid fixture selects front 220 over rear 221; empty sky cannot invent a target/nearest substitute. |
| Solid scenery, roof and terrain occlusion | Upload each real rendered occluder in front of a body; clicking its former torso issues no selection. Upstream geometry tests are complementary, never sufficient alone. |
| Cancel, focus loss and cursor leave | Press then injected Focused(false)/CursorLeft then real release cannot select; Escape while context/press is armed cancels without selecting or closing the world. |
| Keyboard alternatives | Real XTest Tab plus NPC Enter/action/reward keys; delivered ability/cancel alternatives remain covered by existing controls/application regressions below. |
| Exact Marshal, Tanner, Bram and guard | Each actual NPC is selected through native pointer/zone confirmation; Marshal exposes Trouble in the Woods, Tanner honest prerequisite/current state, Bram his actual shop, guard honest unsupported feedback and no fabricated quest/trade. Selection sends no role action. |
| Reachable quest success, completion and reward choice | Accept Marshal quest 1 through native context, complete with earned objective fixture and authoritative copper; Tanner quest 2 completion consumes five actual pelts, credits copper and selected second reward (item 6), leaving unchosen reward item 9 unchanged. |
| Reachable merchant buy and sell | Native Bram actions change real inventory/copper/revision atomically: buy catalog offer 0 once (5 copper/item 2), sell one item from actual bag slot 0 at core catalog price. |
| Range refusal, no substitution/through-wall selection | Both selected Marshal and Bram at 8 m show explicit refusal, unchanged quests/bag/copper/revision; exact NPC assertions and rendered occluder cases disallow nearest substitution/hidden selection. |
| Reconnect one-shot retirement | First settle one native purchase; stall real received projections until reconnect; issue pointer/context/action during interruption, resume same player at newer epoch, verify command counts and inventory/copper/revision/quests unchanged. |
| Existing #71/#72/#121 health, abilities, resources, cast/cooldown and models | Reuse `tests/native_ability_controls.rs`, `tests/combat_hud.rs`, `tests/presentation.rs`, `tests/unit_models.rs`, `tests/architecture_input_bindings.rs`, session one-shot tests, `tests/connected_world.rs`, and every health/HUD/relief GPU check invoked by `scripts/smoke-native.py`. These delivered slices are regressions, not new implementations. |
| Gameplay/input/render authority and bounded composition | Reuse core quest/vendor refusal/reward tests, host `dependency_boundary.rs`, protocol/native/WASM fixtures and existing input architecture tests. Independent actual-diff review must confirm semantic input lifecycle remains in input-bindings and ray/geometry math remains in pinned 3d-lab; MMO owns EntityRef and thin command/UI composition. No second generic picker/input runtime/gameplay rule is authorized. |
| Intentional prerequisite compatibility | Pin Rust and browser 3d-lab together at merged revision per README. Reuse all existing `scripts/smoke-browser.py` BrowserAcceptance cases, especially its five world-click identity/context/cancel/occlusion/overlap cases, plus Bun tests/build and protocol fixtures. This is renderer-pin regression evidence; completed browser #153 is not reopened or counted as native proof. |

## Completion and evidence limits

The application acceptance is explicitly ignored in ordinary Cargo runs because
it requires an OS desktop/GPU. **It must be run explicitly on the candidate**,
alongside native smoke, before readiness/merge; an ignored result is not a pass.
`coding-tooling inspect` selects the native workspace scope and full completion
gate, not a proven narrow impact graph. AGENTS validation tiers apply: focused
local iteration and required native smoke, then existing full GitHub CI gates.
No new workflow, budget, evidence collector or completion bypass is introduced.

The baseline run stops at the first observed failure. Later executable
assertions and reused regressions are **unrun on this candidate until recorded**;
they must all pass before #28 closes. The owning issue/PR records actual commands,
raw initial failures/classification, acceptance commit and implementation head.
Linux X11 is the initial exercised platform; other desktop platforms are unverified.

Owner reproduction after implementation: run `scripts/dev-native.sh`, click two
specific visible creatures without attacking, drag the camera, select each named
NPC and use its advertised context/action; complete a reachable quest/trade, try
outside reach, cancel/leave/focus away, and interrupt/resume. Record actual native
frames and visible UI with authoritative outcomes. Native winit cannot run on
Pages. Because intentional 3d-lab adoption also changes the browser dependency,
verify the existing published browser click journey after the merged Pages
artifact is deployed; record that compatibility result separately from native
acceptance and keep #28 open if its required publication evidence remains pending.

## Initial evidence and input-source correction

The first compile found two acceptance-author API typos (not production failures).
The first runtime setup omitted the ability catalog and correctly received core
class refusal; the independent fixture was corrected to preserve every catalog.
A fixed setup sleep then read a stale pre-relocation projection; it was replaced
with an assertion/wait for the actual fixture projection. Both raw logs and
source inputs were preserved before corrections.

A subsequent experiment used XTest for pointer events. Explicit ordinary-winit
event counters exposed that this desktop did **not** deliver its press/release
requests to the application, even after bounded X11 focus/raise, window-coordinate
targeting and delivery waits. The earlier XTest-click timeout is therefore
**unqualified input evidence, not a product failure**. Pointer acceptance was
independently restored to the original actual-WindowEvent handler seam before
implementation; all target coordinates, lifecycle scenarios, real host/GPU and
expected authoritative/UI outcomes remain unchanged. OS pointer routing remains
an explicit environment/harness evidence gap. XTest keyboard delivery is also
unrun beyond the baseline's first missing click and remains fail-closed in this
contract. Do not call either unverified OS path passed. If its helper/inputs are
changed later, independently reverify that the same keyboard journey is exercised.

The independent isolated-X11 run subsequently **resolved pointer delivery**:
Xvfb used an OS-selected display, Wayland was unset, and the NVIDIA Vulkan ICD
was selected explicitly. The actual window-compatible adapter request logged
`NVIDIA GeForce RTX 3060 Ti`, `DiscreteGpu`, Vulkan, driver `595.91.07`.
The real application connected/chose Warden, received the canonical fixture,
and received OS pointer press/release through its ordinary winit handler
(frame 30). It then failed at the expected baseline behavior:
`exact clicked identity must be confirmed by zone`; the real host observed no
`SelectTarget` command. This is a qualified baseline behavioral red. The
isolated Xvfb was stopped after the run. Later NPC/keyboard/reconnect assertions
remain authored and unrun because the baseline stops at this first missing click.
The raw logs, exact runner/environment and source snapshots are retained under
`/tmp/mmorpg28-independent/` and summarized on the owning issue/PR.
