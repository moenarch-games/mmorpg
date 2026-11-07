# Over-the-shoulder movement, focus and aiming

**Status:** approved product intent, **not yet the implemented default controls**. Implementation: [focus camera and strafing #650](https://github.com/moenarch-games/mmorpg/issues/650), [temporary manual aim #649](https://github.com/moenarch-games/mmorpg/issues/649), existing [attack-input gestures #636](https://github.com/moenarch-games/mmorpg/issues/636). Canonical decisions: [Decision Atlas](decisions/atlas.md), entries `5.2.i650`, `5.2.i649` and `5.2.i649-2` (open question).

This describes the desired *feel* of Greyhaven combat, not a claim about the current native or Pages implementation. The current drag-orbit controls, left-click immediate swing and right-click entity selection are documented in [NATIVE_CLIENT.md](NATIVE_CLIENT.md) and [README.md](../README.md).

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

A cancel, blur, blocking panel, device/remap/context change or refused throw must retire the aim cleanly **without a phantom attack or stuck pointer capture**. A manually aimed ability does not globally impose hold-to-release on ordinary immediate attacks. The exact user-facing throw confirmation (hold key then release versus press to arm and click to confirm) remains an explicit product question; see Atlas `5.2.i649-2` and [#649](https://github.com/moenarch-games/mmorpg/issues/649).

## Ownership and implementation order

1. **#650** — focus/no-focus over-shoulder camera, automatic facing, pointer capture and consistent strafing; browser and native parity.
2. **#636** — reconcile existing left-click immediate swing/right-click selection with input gesture ownership and the new camera states. Reuse `input-bindings`, including safe cancellation, instead of introducing another key system.
3. **#649** — temporary manual-aim override for one existing throwable, preserving focus and authoritative ability outcomes; extend only after the first playable throw feels correct.

The client owns camera framing, pointer capture and visual aim aids. `input-bindings` owns mapping/held lifecycle. `mmorpg-core` owns the authoritative facing/movement command interpretation, ability legality and spatial hits. `mmorpg-protocol`, local WASM and native/online must agree whenever a gameplay command changes. The renderer never determines damage or projectile impact.

The first acceptance journey is: **free-look strafe → focus a wolf → circle it while the camera tracks and mouse can click UI → swing/continue combo → temporarily aim a bomb away from the wolf → throw or cancel → return to the same focus → clear focus and look freely**. Verify the browser and native user-visible path, with a small state-transition regression test; do not add a second input or camera framework.
