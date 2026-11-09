# Over-the-shoulder movement, focus and aiming

**Status:** the #650 camera/strafe foundation is implemented in this candidate branch, pending real-client acceptance. Temporary manual aiming [#649](https://github.com/moenarch-games/mmorpg/issues/649), radial menus [#653](https://github.com/moenarch-games/mmorpg/issues/653) and further attack-input ownership [#636](https://github.com/moenarch-games/mmorpg/issues/636) remain separate. Canonical decisions: [Decision Atlas](decisions/atlas.md), entries `5.2.i650`, `5.2.i649`, `5.2.i649-2`, and `5.2.i649-3`.

The first slice uses the existing camera and input bindings: mouse-look in free exploration (click the world to capture in browsers), an uncaptured cursor for enemy focus, Tab/right-click target switching, and Escape to clear focus. Native recaptures on clearing; browsers require a world click once Pointer Lock has been released. Touch drag and directional strike/guard input remain available. The sections below also describe *planned* temporary manual aiming and radial menus, not implemented behavior.

## Camera and locomotion

There is **one continuous third-person, over-the-shoulder camera**, including exploration, focused combat and temporary aiming. It never switches into first person. The camera framing and mouse owner change with the player's state; the physical character stays in the same world.

| | Free exploration (no combat focus) | Focused combat (selected opponent) | Temporary manual aiming |
| --- | --- | --- | --- |
| Camera | Shooter-like over-shoulder mouse look | Smoothly tracks the focused enemy, framing both player and target | Same shoulder camera, smoothly handed back to mouse-look aim |
| Cursor | Captured for looking when gameplay owns it; release for UI | Free cursor for abilities and interface | Aim/reticle cursor or captured look input according to the ability |
| Facing | Follows the mouse-controlled look direction | Automatically and continuously turns toward focused enemy | Temporarily follows the manual aim direction |
| W / S | Forward / backpedal | Approach / retreat while facing enemy | Forward / backpedal along aim direction |
| A / D | **Strafe** left / right | **Strafe** in character-local axes, naturally circling the enemy | Strafe left / right |
| Selection | No selection is needed to swing or use spatial attacks | Focus helps aim but does not guarantee hits | Previous focus identity is retained until changed or invalidated |

In focused combat, movement uses **character-local axes** while facing follows the enemy. Therefore pressing A or D moves sideways around the enemy without a special orbit-movement command. Rotating the camera must not silently redirect character strafing.

Focus is a deliberate, persistent target choice, obtainable through the existing selection/Tab routes or an appropriate crosshair selection route while the mouse is captured. Switching focus updates facing and framing smoothly. Losing, clearing or invalidating focus returns to no-focus controls; it must not secretly select a replacement. Any manual UI interaction first releases captured pointer input safely.

Camera transitions preserve spatial continuity and the shoulder offset: avoid viewpoint teleportation, aim jumps, abrupt forced recentering, and rotation fights between focus tracking and free look. Smooth tracking should still be responsive enough for a moving enemy, with stable framing of both combatants. Large opponents may require framing adjustment; no new default camera system is implied.

## Attacks and ability targeting

**Ordinary attacks do not require a selected entity.** The existing [free-aim combat #517](https://github.com/moenarch-games/mmorpg/issues/517) already establishes an immediate sword swing along the character's facing, a physical hit arc/reach, multiple potential victims and an empty-air whiff. Focus supplies automatic direction for the same spatial attack; actual hits remain based on server-authoritative coverage, movement and obstacles. No camera-selected entity becomes an auto-hit.

Combos must work with or without focus. Running, backing and strafing can continue during compatible attacks; authored commitment, recovery and latest-valid input buffering still apply. Keep the existing Warden movement-dependent effects and hit-confirmed Heroic Strike → Shield Bash rules, including the authoritative struck-target identity for that particular combo ([#155](https://github.com/moenarch-games/mmorpg/issues/155), [#157](https://github.com/moenarch-games/mmorpg/issues/157)). Selecting another opponent does not rewrite a committed strike's actual hit or combo target.

An ability's targeting requirement belongs to **that ability**, for example:

| Targeting intent | Examples | Required selection? |
| --- | --- | --- |
| Spatial melee | Ordinary sword swing, ordinary spatial combo hit | No |
| Forward spatial / free shot | Bow shot, skill shot | No, unless that specific ability says otherwise |
| Selected entity | Explicitly target-bound heal, buff, tactical order, selected-target skill | Yes, or its declared self/default behavior |
| Self | Self-defense, self buff | No |
| Manual ground/arc aim | Grenade/bomb, aimed area spell | No; choose a point/direction manually |

The table classifies intended input semantics; it does not silently change all current spells, existing class kits, damage or balance. Preserve per-ability rules such as homing target-based projectiles versus freely moving projectiles (Atlas `5.2.71`). A dedicated hold-key directional strike or guard may temporarily own mouse motion without moving the camera, per [#636](https://github.com/moenarch-games/mmorpg/issues/636), [#154](https://github.com/moenarch-games/mmorpg/issues/154), and [#156](https://github.com/moenarch-games/mmorpg/issues/156).

## Temporary manual aiming, such as a grenade

1. While focused on a wolf, press the grenade/throw ability. **Keep the wolf selected**.
2. Smoothly hand the same shoulder camera from target tracking to mouse-controlled free aim. Show a reticle and, for an arc throw, a predicted landing point/trajectory as a non-authoritative aid.
3. Strafe and aim normally within the ability's existing movement restrictions. The character temporarily faces the aim direction.
4. Deliberately confirm the throw; send an ordinary authoritatively validated ability intent. It may land away from the focused wolf and may miss or be obstructed.
5. Restore the previous focus, facing assistance, free cursor and target-framing camera smoothly. If there was no valid focus, return to mouse-look exploration instead.

### Throw confirmation (owner-approved)

Both interaction styles are available as configurable options for manually aimed grenades, using the existing input/settings system:

- **Default — hold to aim, release to throw:** holding the grenade ability enters shoulder-camera mouse aim; deliberate key release commits the throw. A quick tap can throw immediately along the current aim.
- **Alternative — press to aim, click to throw:** pressing the ability enters an armed aim state; an intentional left-click confirms the grenade.

A cancel, blur, blocking panel, device/remap/context change or refused throw must retire either style cleanly **without a phantom attack or stuck pointer capture**. Forced key release is never a deliberate throw. Neither style changes the existing immediate sword swing or automatically makes every skill held. This answers Atlas Question `5.2.i649-2` through Decision `5.2.i649-3` ([#649](https://github.com/moenarch-games/mmorpg/issues/649)).

## Pie and marking menus

**Planned:** [#653](https://github.com/moenarch-games/mmorpg/issues/653); Atlas `5.2.i653`. A radial/pie menu is an optional fast command surface, especially useful as more combat and utility actions arrive. A practiced **marking-menu flick** lets a player choose an action by direction, keeping the number of dedicated hotkeys small.

Support **both** menu sources:

- **Fixed wheel:** user-configured slots referencing existing semantic actions (abilities, emotes, utility), with stable directions so muscle memory develops.
- **Context-sensitive wheel:** slots derived from supported actions for the current target/situation (NPC Talk/Trade, relevant object interaction, appropriate combat commands). Invalid or absent actions must not become fake client-side interactions.

A typical held gesture is **hold the menu action → show a compact wheel at screen center → move the mouse toward a sector → deliberately release to activate that action**. A short flick can select without waiting for animation. A central dead zone, Escape, losing focus or retiring the held input cancels without dispatch. A tap-to-open and click-to-confirm alternative, plus keyboard/gamepad access, can coexist with the same semantic action choices.

While the wheel owns mouse motion, the **same over-the-shoulder camera holds its current view**. Preserve any focused enemy and restore the previous captured/free cursor, camera mode and facing assistance when the wheel closes. An explicit menu gesture must not also turn the camera, swing a sword, guard or throw a grenade; reuse the existing `input-bindings` context/held lifecycle rather than creating conflicting hotkeys. The MMO simulation keeps running; the wheel is only a client-side command-selection aid.

**Combos:** selecting a combat action with a quick directional flick may serve as one input in an existing timed, authored combination. Repeated menu uses should be responsive enough to sequence actions, but server authority still decides attack timing, actual struck targets, hit confirmation and legal follow-up windows. Nested or multi-stroke gesture chains may be explored later; the first implementation should reuse the current action graph and avoid inventing a new combo engine.

## Owner decisions of 2026-11-08

These refine the sections above; where they differ, these win. Canonical entries: Atlas `5.2.i636` to `5.2.i636-6`, `5.2.i653-2`, `5.2.i679`, `5.2.i679-2` and `5.9.i680`.

**Left click selects and never attacks.** It confirms a free-targeted attack during an armed aim. **Q** is the instant basic swing. Directional gestures keep their own held inputs. **Right click cancels** a held strike, guard, aim or wheel without firing, and cancels a cast. **Escape is the game menu**: it closes an open panel first, and losing browser pointer lock opens the menu. This replaces the Escape cancels described above.

One owner holds the mouse at a time:

| Owner | Mouse motion | Left click | Right click |
| --- | --- | --- | --- |
| Free look | Camera and heading | Select under crosshair | Cancels a cast |
| Focus | Cursor; camera tracks target | Select or click UI | Cancels a cast |
| Strike or guard hold | Direction and lane | Nothing | Cancel, no attack |
| Manual aim | Aim | Confirm (click-to-throw style) | Cancel, no attack |
| Wheel | Pick a sector | Confirm (tap-to-open style) | Cancel, no action |

A wheel cannot open during a hold or aim. Stun, death and focus loss end the current owner without firing. Camera lock and free camera during a hold are both selectable, to compare in play. Wheels are an option alongside the locked camera. Mobile is frozen: no new touch work. Timed hitboxes and clashes are tracked in [#679](https://github.com/moenarch-games/mmorpg/issues/679), arena first, and netcode options in [#680](https://github.com/moenarch-games/mmorpg/issues/680).

## Action-bar rows (#294)

Keys 1–9 and 0 use the first ten action-bar slots; **Shift+1 to Shift+0** use slots 11–20, a second row of the bar in both clients for every class whose kit outgrows ten (Atlas `5.2.i294`). Shift is now only this modifier: the dodge roll moved to **F** (`5.2.i294-2`). Gamepad and touch reach of the second row stays out of scope until the wheels (#653).

## Hold-to-draw power shots (#607)

Aimed Shot and Piercing Shot draw while held, on whichever action-bar slot they sit, Shift row included (Atlas `5.2.i607`). A key, a gamepad button or a touch face held on them starts `Bow_Power_Draw` locally and keeps it drawn; **releasing it sends the shot**, the zone decides then, and `Bow_Release_Upper` plays. A tap fires at once from the partial draw. An action-bar click has no release to wait for and fires at once. Nothing here changes the abilities' cost, cooldown or damage, and no cast time is added: a release out of range or on cooldown is an ordinary refused cast.

The draw does not own the pointer, so free look keeps steering and a focused camera keeps tracking while it is held. Escape, a mouse click, losing focus or pointer lock, opening a panel or Settings, dying, a stun, starting a cast, the bar no longer holding the ability, or pressing another ability all drop the draw without firing; a superseded key's later release does nothing. A press while a panel is open or the ability cannot draw (a dead, stunned or casting viewer, an unlearned ability) is an ordinary use, and the zone answers it as before. Both clients run the same rule (`mmorpg-client-model`).

## Ownership and implementation order

1. **#650** — focus/no-focus over-shoulder camera, automatic facing, pointer capture and consistent strafing; browser and native parity.
2. **#636** — reconcile existing left-click immediate swing/right-click selection with input gesture ownership and the new camera states. Reuse `input-bindings`, including safe cancellation, instead of introducing another key system.
3. **#649** — temporary manual-aim override for one existing throwable, preserving focus and authoritative ability outcomes; extend only after the first playable throw feels correct.

The client owns camera framing, pointer capture and visual aim aids. `input-bindings` owns mapping/held lifecycle. `mmorpg-core` owns the authoritative facing/movement command interpretation, ability legality and spatial hits. `mmorpg-protocol`, local WASM and native/online must agree whenever a gameplay command changes. The renderer never determines damage or projectile impact.

The first acceptance journey is: **free-look strafe → focus a wolf → circle it while the camera tracks and mouse can click UI → swing/continue combo → temporarily aim a bomb away from the wolf → throw or cancel → return to the same focus → clear focus and look freely**. Verify the browser and native user-visible path, with a small state-transition regression test; do not add a second input or camera framework.
