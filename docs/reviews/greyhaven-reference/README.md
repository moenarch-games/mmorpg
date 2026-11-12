# Greyhaven reference captures (render-engine#74)

The approved visual reference for Greyhaven (owner decision 2026-11-11 on
[render-engine#74](https://github.com/moritzbrantner/render-engine/issues/74)): native captures
from `greyhaven_lighting_shots`, 1280×720, render-engine `3d0acc68`. All nine shots were captured
again for #753 (on mmorpg `620a7b55` with #753's changes), so that the native and browser shots
differ only in rendering:

- **Motion:** both capture with reduced motion. The campfire and cave-torch flames hold a steady
  brightness instead of flickering at whatever moment each client sampled, the reeds and idle
  poses hold still, and levels of detail switch at once instead of cross-fading. Neither client
  sways its trees in the wind, so there is no wind time to fix.
- **Content:** both show the browser's default character, a male Warden (the native shots
  showed a female Ranger). The browser holds its local zone once entered (`holdWorld`), so the
  units stand where they were at entry, as in the native shots.
- **Outpost material:** the native client baked the Outpost kit's and the fort's textures into
  vertex colours, so the stone hall and the chimneys read flat grey. It now draws them with
  their textures and normal maps, as the browser does (client asset loading; the asset-tooling
  output was fine).
- **Anti-aliasing:** already the same. Both draw the main HDR pass with 4× MSAA (render-engine
  `SAMPLE_COUNT`, three-lighting's `samples: 4`) and alpha-masked leaves by an alpha-test
  `discard` at the material's cutoff (0.5 by default); neither uses alpha-to-coverage.
- **Levels of detail:** already the same. Both pick levels through the shared
  `mmorpg_presentation::lod::Lods` with the same descriptors, bias 1 and the camera's vertical
  field of view. A browser capture of `frame-lake` alone measured exactly as the one after all
  other shots, so the selection's hysteresis does not depend on the shot order either.

Files:

- `re9-<shot>.png`: the native reference, lossless. The Outpost at noon, dusk and night, a
  campfire and the lantern post by night (`noon`, `dusk`, `night`, `torch-campfire`,
  `torch-lantern`), and the four `outpost_frame_time` views at the zone's starting hour, 08:00
  (`frame-hub`, `frame-woods`, `frame-vale`, `frame-lake`).
- `views.json`: each shot's eye, target, field of view and hour.
- `<shot>.jpg`: for review, the native reference, the browser at the same pose and hour, and the
  FLIP heat map, left to right.

The browser is compared against the native reference with FLIP per shot. The numbers are
recorded, not a gate. Browser: three.js 0.186.1 with `@moritzbrantner/three-lighting`, Chrome
157.0.8103.38 (Playwright `--channel chrome` under `xvfb-run`), ANGLE on OpenGL ES 3.2, RTX 3060
Ti, pixel ratio 1. Before is `620a7b55` without #753's changes on the same machine.

| Shot | Hour | FLIP mean | FLIP p99 | Before #753 (mean / p99) |
| --- | --- | --- | --- | --- |
| `noon` | 12.0 | 0.0402 | 0.3054 | 0.0634 / 0.3428 |
| `dusk` | 17.8 | 0.0184 | 0.1908 | 0.0514 / 0.2688 |
| `night` | 23.5 | 0.0177 | 0.1465 | 0.0381 / 0.2101 |
| `torch-campfire` | 22.0 | 0.0131 | 0.1062 | 0.0567 / 0.2636 |
| `torch-lantern` | 22.0 | 0.0137 | 0.0422 | 0.0731 / 0.1489 |
| `frame-hub` | 8.0 | 0.0429 | 0.3298 | 0.0643 / 0.3852 |
| `frame-woods` | 8.0 | 0.0618 | 0.6759 | 0.0618 / 0.6758 |
| `frame-vale` | 8.0 | 0.0315 | 0.2668 | 0.0352 / 0.2793 |
| `frame-lake` | 8.0 | 0.0697 | 0.6463 | 0.0697 / 0.6465 |

Two browser captures in a row measured the same to four places for every shot here. What
remains is rendering, and these known differences:

- The Outpost modules' wear: the browser ages each building from the kit's wear masks
  (darker, mossy stone, missing roof units); the native client draws the pristine modules.
- The woods (`frame-woods`, `frame-lake`): the error stays on the leaf cards' and trunks'
  edges and their shadows, with the levels picked by the same shared selection; it is the
  shading and shadowing of the foliage, not its content.

Reproduce:

```sh
bash scripts/fetch-bundles.sh && bash scripts/prepare-ranger-native.sh
MMORPG_LIGHTING_SHOTS="$PWD/artifacts/lighting" cargo test --release --locked -p mmorpg-client \
  --lib greyhaven_lighting_shots -- --ignored --nocapture
(cd web && bun install --frozen-lockfile && bun run build)
xvfb-run -a python scripts/capture-browser-reference.py docs/reviews/greyhaven-reference/views.json \
  --out artifacts/greyhaven-reference --channel chrome
# in a render-engine checkout, per shot:
cargo run --release --locked --manifest-path oracle/Cargo.toml -- --flip-images \
  <mmorpg>/docs/reviews/greyhaven-reference/re9-noon.png \
  <mmorpg>/artifacts/greyhaven-reference/noon.web.png noon.flip.png
```
