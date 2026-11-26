# Proposed refactoring sequence

Planning follow-up to [the inventory](README.md) (#1217), against main `2921a84cdb3461529eafa8135630ef4b1c19b0d1`. This is a proposal for owner review, not an approved architectural decision or completed implementation. The goal is fewer semantic owners, stronger locality and discoverable existing behavior, not a target file count or a rewrite. Keep existing crate boundaries unless an actual consumer demonstrates a need to change them.

## 1. Establish the navigation path first (R1)

- Extend existing [task knowledge](../../../.coding-tooling.json) rather than creating another index format. Introduce narrowly matched cross-crate scopes for combat/actions, locomotion, progression, quest credit, world incidents and client behavior; include source examples, existing focused tests and relevant current docs/Atlas links. Test lookup from *both* producer and consumer paths, e.g. core melee, Action Lab and shared clip playback.
- Use the convention's `CONTEXT.md` for a short domain glossary and routes into existing capabilities; link from AGENTS/README. If a domain needs deeper current-state instructions, put them under the domain's existing documentation or `docs/domain/`, not in a second historical decision record.
- Before proposing new behavior, locate the existing owner, data/state, consumers and tests; when an owner cannot be found, state the exact search scope and uncertainty. Update existing navigation in the same change as ownership moves (REPO-019).
- Reconcile stale descriptions while touching a domain. Example: [Greyhaven quest loader](../../../crates/mmorpg-core/src/content/greyhaven_vale/quests.rs) starts with an old quest-count description, although its authoritative catalogue is JSON; don't repeat a changing count as static prose.
- Acceptance: an agent investigating attack movement via either source or lab discovers ActionGraph, the current melee path, production baseline, controller ordering, Atlas 5.7–5.9 and real lab/test entry points without reconstructing closed issues. Use the existing coding-tooling inspect command, not a parallel discovery tool. No blanket CI gate in this work package.

## 2. Consolidate combat and movement (R2–R4, first implementation priority)

The inventory's C1–C5 identify actual gaps. Start with a structural change that keeps the current standing chain, lunge, jumping contact, guard, directed and timed arena behavior. Do not mix it with tuning or adding abilities.

Proposed internal layout (illustrative; keep public re-exports and names where useful):

```text
mmorpg-core/src/
  action.rs                 public facade; existing ActionGraph
  action/
    baseline.rs             existing production configuration
    authored.rs             existing lab authoring
    definition.rs           typed actions, phases, conditions, effects, windows
    validation.rs           definition validation and effective overrides
    runtime.rs              live region state, timing, arbitration, interruptions
    inspection.rs           read-only reports of that same state
  melee.rs                  weapon-profile selection and basic strike execution
  strike.rs                 directed attack execution
  swing.rs                  timed sweep, clash and interception
  casting.rs                cast/ability orchestration
  combat.rs                 damage/death and zone combat coordination
  stance.rs                 movement context and landing
  zone.rs                   fixed tick coordinator and physics integration
sim-combat/src/
  melee.rs                  pure weapon-family profiles
  swing.rs                  pure hit geometry
sim-movement/src/
  walk.rs                   pure movement primitives
```

**R2 — Make the graph the shared phase and transition authority.** [action.rs](../../../crates/mmorpg-core/src/action.rs) already holds the graph and many concepts (~2,947 lines); split its internals only at seams that help callers, retaining its API. [melee.rs](../../../crates/mmorpg-core/src/melee.rs) has PendingStrike/MeleeState, chain/window/follow_up and the contact countdown; [combat.rs](../../../crates/mmorpg-core/src/combat.rs) ticks swing_timer and dispatches player_swing. Expose the actual basic-strike phases and variant to the existing graph/inspection and use the existing arbitration/edge semantics for the chain where equivalent. Derive reports from canonical state, not a second ticking state machine. Preserve the #928 hit-triggered early follow-up, queue behavior, timed landing reward, Warden directed combo, #915 mastery and costs. If the current state lacks recovery identity after contact, add only the information actually needed; preserve canonical snapshot validation/replay and wire compatibility, or document and re-record a necessary format change. Keep arena timed swings and creature-specific attacks ([creature_attack.rs](../../../crates/mmorpg-core/src/creature_attack.rs)) as specialized execution primitives; NPC graph authoring was deferred. Do not force every attack into the same collision algorithm.

**R3 — One controller composition path.** [zone.rs](../../../crates/mmorpg-core/src/zone.rs) currently applies graph movement and then replaces velocity with the lunge; [stance.rs](../../../crates/mmorpg-core/src/stance.rs) tracks landing and [sim-movement walk.rs](../../../crates/sim-movement/src/walk.rs) owns ordinary locomotion. Separate the calculation of a move's desired drive (including lunge) from the application of authored phase restrictions, and make their order explicit. Preserve surface, water/current, forced motion, collision, support and gravity semantics. Physics Engine remains the contact authority. Then, as a separate deliberate behavior change, author the owner's standing, running, jumping and air-momentum decisions in the existing graph and verify in the lab. Avoid a second movement controller or independent prediction rules. The current predictor reuses core but does not observe all combat state; verify reconciliation before claiming parity.

**R4 — One spatial/timing contract, two presentation adapters.** Existing sim-combat coverage, [strike.rs](../../../crates/mmorpg-core/src/strike.rs), [units/imported/playback.rs](../../../crates/mmorpg-presentation/src/units/imported/playback.rs), [units/effects.rs](../../../crates/mmorpg-presentation/src/units/effects.rs) and [units/contact.rs](../../../crates/mmorpg-presentation/src/units/contact.rs) are the starting point. Inventory current contact markers and effect geometry for three basic moves. Make authored, validated weapon trajectories (or explicit spell volumes) accessible to core without linking presentation; project the same timing/shape identity to visual consumers. Retain current collision/mitigation policies and actual contact outcomes; do not let renderer poses decide damage. Existing recoil/hitstop remains cosmetic; simple authoritative hit reactions can use the existing reaction rules after the structural baseline is established. This step requires actual asset/clip and live-play evidence; the current arc/height-band approximation is not a verified physical sweep.

**Verification:** use the existing combat integration binary's action_graph, action_windows, action_traversal, movement_strikes, movement_melee, timed_swing, combo and weapon_mastery modules; existing protocol combat recovery/fixtures, Action Lab and native/browser presentation tests. Reuse deterministic replay comparison for behavior-preserving changes. R3/R4 hot-path changes follow the existing work-operation contract, not new budgets. Play the three moves in native and browser for the later feel/tuning slice.

## 3. Shared client behavior and presentation (R5)

The orbit camera is **already consolidated**: [client-model camera.rs](../../../crates/mmorpg-client-model/src/camera.rs) implements orbit/zoom/focus; [native camera.rs](../../../crates/mmorpg-client/src/camera.rs) is an adapter; [WASM camera.rs](../../../crates/mmorpg-wasm/src/camera.rs) exposes it to [orbit-camera.ts](../../../web/src/world/orbit-camera.ts). Do not repeat the camera migration. Shared mouse ownership, front-end menu descriptions, HUD and impact cues also exist.

A verified remaining candidate is local character roster semantics:

- [Native roster.rs](../../../crates/mmorpg-client/src/roster.rs) implements name length, normalization, duplicate comparison and ID checks; [browser character-selection.ts](../../../web/src/character-selection.ts) implements them again. Browser name comparison uses NFKC + lowercase, native lowercase only; browser explicitly permits combining marks, native uses alphabetic characters; their accepted IDs and storage fields differ. [character-roster.ts](../../../web/src/character-roster.ts) validates/loads/saves browser records. Similar names in comments are not evidence of actual parity.
- Move genuinely shared **pure** roster rules into a focused module in existing mmorpg-client-model (browser via mmorpg-wasm); reuse core class IDs rather than another enum. Keep filesystem, localStorage, retained-ID/save lookup and renderer-specific preview adapters at their current boundaries. Do not normalize old IDs into different characters or silently reject old names. Explicitly decide the accepted common name policy and preserve compatibility with existing rosters before converging formats. A shared validation corpus should exercise Unicode/whitespace, duplicate names and retired IDs.
- Next compare native/browser minimap, world map, roster UI, control mapping, effect state and settings against already existing shared modules (e.g. client-model mouse_owner, world_map, exploration, front_end, impact; presentation lighting, imported clips). Only extract remaining duplicate semantic decisions, not renderer/platform code. #1154 is a candidate list, not proof every listed copy still exists. Native GLB/meshopt/KTX2 (#1155) is a separate provider evaluation: do not invent a third asset importer.

## 4. World events, route following and quests (R6–R7)

**R6 — Shared route sampling, distinct event policies.** [routine.rs](../../../crates/mmorpg-core/src/routine.rs) implements `legs` and `along`; [cart.rs](../../../crates/mmorpg-core/src/cart.rs) already consumes them. [crossing.rs](../../../crates/mmorpg-core/src/crossing.rs) implements separate `length`, `point_at` and `distance`; [supply.rs](../../../crates/mmorpg-core/src/supply.rs) also defines the identical distance helper but patrols move through their creature/physics rules. Extract only the shared deterministic polyline sampling/length semantics into one internal module (e.g. core route.rs), or extend the existing helper if that gives a simpler interface. Preserve route start conventions, reverse direction, exact corner/zero-leg handling, endpoint headings, integer rounding and pace; they differ today. First compare outputs on existing routes and reuse existing tests. Do not force patrols following physical bodies to teleport along a polyline. Cart robbery, crossing health/retry, patrol interception, raid and NPC routines have different scheduling, participants, outcomes and persistence; keep those event policies in their modules. Reuse route and existing incident/reward boundaries for #1215 rather than a universal event framework. Inspect unattended-zone lifecycle and durable economy before any caravan implementation.

**R7 — Quest catalogue and credit.** [quest.rs](../../../crates/mmorpg-core/src/quest.rs) already owns objective definitions, log validation, progress, accept/turn-in, credit and quest sheet. [Greyhaven quests.json](../../../content/greyhaven/quests.json) is authored data; the loader converts ObjectiveSource into the typed model. Specialized modules (npc_quest, construction, mechanism, carry, raid, crossing, supply) own their world actions. Keep quest credit at the existing zone boundary; compare shared once-only completion/progress and participant attribution across those modules before extracting any helper. Optional internal split by definitions/validation, log/progress, and turn-in/credit behind the existing interface, not new parallel quest runtimes. Avoid generic abstractions that erase important distinct progress sources (inventory count, stored progress, durable evidence, shared outcome). Preserve stable objective codes, quest IDs, rewards and save semantics, especially #1212 (open seal late-arrival credit). Use existing tests/content and tests/zone quest, incident receipt, recovery and reward coverage. Add new content through the same typed catalogue.

## 5. Progression and ability organization (R8)

[ability.rs](../../../crates/mmorpg-core/src/ability.rs) has a 59-entry ABILITY_CATALOG at the baseline, with class and level gating and typed effects. [casting.rs](../../../crates/mmorpg-core/src/casting.rs) checks learned() in ready_ability and has the effect dispatcher, cast/channel advancement and aura handling. [progression.rs](../../../crates/mmorpg-core/src/progression.rs), [mastery.rs](../../../crates/mmorpg-core/src/mastery.rs), [equipment.rs](../../../crates/mmorpg-core/src/equipment.rs) and sim-combat class rules already own relevant facts.

- Retain existing ability IDs and typed catalog. Consider splitting ability definitions/catalogue into meaningful class/family files while keeping one lookup and existing content identity; split casting internals only when the admission/resource, timing, targeting and effect-dispatch seams are stable. Do not make each class another full executor.
- Before adding a talent tree, trace current unlocks (including mastery-gated landing techniques), prerequisites, saved character fields and shared HUD ability presentation; retrieve existing Atlas decisions. One authoritative unlock query should be usable by validation and projected UI. The tree is progression data over that system, not a competing ability list, cast engine or duplicate cooldowns. A specific tree structure or respec rule is not decided here.
- Acceptance: existing unlock/cost/cooldown, mastery, ability and durable character checks remain green; later add one bounded talent branch through the existing grant/save and action pathways. No balance changes in structural commits.

## 6. Defer large crate and persistence reorganization

#1156 (engine vs game) needs actual Battle Royale consumer analysis before changing crate boundaries. Begin by reducing cross-domain coupling *inside* mmorpg-core; zone.rs and [snapshot.rs](../../../crates/mmorpg-core/src/snapshot.rs) coordinate state and canonical recovery and are not inherently duplicate merely because they touch many systems. Do not split the state across new service writers. #1139 (handoff + persistence) is a separate distributed authority project, not incidental cleanup. Retain physics A/B, deterministic scenarios, compatibility fixtures, procedural visual fallbacks and opt-in lab paths until a replacement/retirement decision has evidence.

## Proposed order and stopping criteria

| Package | Depends on | Acceptance focus |
| --- | --- | --- |
| R1 navigation | Inventory | Relevant owners/docs/tests discoverable from code and lab paths |
| R2 action-state integration | R1 | Existing basic and directed phases visible; no parallel timing authority; replay parity |
| R3 movement composition | R2 | Single composition order; no unrestricted replacement lunge; current behavior before separate tuning |
| R4 geometry/presentation contract | R2–R3 | Contact/shape/effect agreement on the three chosen moves; live play |
| R5 shared clients | R1 | Verified shared semantic duplicates removed, old roster compatibility and parity |
| R6 routes | R1 | Equivalent route samples, distinct event policies; safe base for #1215 |
| R7 quest credit | R6 where applicable; #1212 resolved/rebased | No duplicate grants or progress drift |
| R8 ability/progression | R2 | Existing unlocks and casts work; no duplicate rules for future talents |

Proposed sequencing: R1 → R2 → R3 first, then tune moves; R5 roster and R6 route sampling are independent bounded slices when a worker is free, not reasons to block combat. R4 before adding many attack variants; R8 before expanding talent trees. The table is a proposal for separate owner-approved scopes, not permission to launch parallel workers or open all issues. Each eventual implementation follows one task/branch/PR and existing happy-path/format/clippy/CI rules. Delete obsolete code only when consumers (including WASM, feature-gated diagnostics and external consumers) have migrated and relevant checks pass. No arbitrary line-count goals, automatic dead-code deletion or new generic framework.

**Evidence and limits:** based on source inspection, not local compile/test or runtime traces. The roster comparison and route duplication are directly inspected; broader native/browser duplication, all quest credit paths, actual asset trajectories, Battle Royale consumers and dead-code reachability remain audit tasks. Proposed names/layouts and work-package boundaries are not implementation decisions yet.
