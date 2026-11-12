# Greyhaven reference captures (render-engine#74)

The approved visual reference for Greyhaven (owner decision 2026-11-11 on
[render-engine#74](https://github.com/moritzbrantner/render-engine/issues/74)): native captures
from `greyhaven_lighting_shots`, 1280×720, render-engine `df5597b5`. The four `frame-*` shots are
from mmorpg `05a1e3eb`. The shots at a set hour (`noon`, `dusk`, `night`, `torch-*`) were
captured again at `eb58c43b` with the fog of their own hour (#757): before, they kept the fog
colour of the zone's 08:00, which the renderer adds unexposed, so the night shots were lifted
by a daytime grey-blue that neither client shows in play
([render-engine#102](https://github.com/moritzbrantner/render-engine/issues/102)).

- `re9-<shot>.png`: the native reference, lossless. The Outpost at noon, dusk and night, a
  campfire and the lantern post by night (`noon`, `dusk`, `night`, `torch-campfire`,
  `torch-lantern`), and the four `outpost_frame_time` views at the zone's starting hour, 08:00
  (`frame-hub`, `frame-woods`, `frame-vale`, `frame-lake`).
- `views.json`: each shot's eye, target, field of view and hour.
- `<shot>.jpg`: for review, the native reference, the browser at the same pose and hour, and the
  FLIP heat map, left to right.

The browser is compared against the native reference with FLIP per shot. The numbers are
recorded, not a gate. Browser at the same revision: three.js 0.186.1 with
`@moritzbrantner/three-lighting`, Chromium 158.0.8127.18 headless (Playwright 1.65.0), ANGLE on
OpenGL ES 3.2, RTX 3060 Ti, pixel ratio 1.

| Shot | Hour | FLIP mean | FLIP p99 | Before #757 (mean / p99) |
| --- | --- | --- | --- | --- |
| `noon` | 12.0 | 0.0593 | 0.3211 | 0.0594 / 0.3211 |
| `dusk` | 17.8 | 0.0482 | 0.2629 | 0.0663 / 0.2657 |
| `night` | 23.5 | 0.0355 | 0.1994 | 0.0984 / 0.2373 |
| `torch-campfire` | 22.0 | 0.0206 | 0.0905 | 0.0771 / 0.2399 |
| `torch-lantern` | 22.0 | 0.0763 | 0.2024 | 0.0955 / 0.2755 |
| `frame-hub` | 8.0 | 0.0601 | 0.3496 | |
| `frame-woods` | 8.0 | 0.0537 | 0.5587 | |
| `frame-vale` | 8.0 | 0.0295 | 0.2084 | |
| `frame-lake` | 8.0 | 0.0697 | 0.6465 | |

The campfire and the cave torches flicker, and the two clients capture them at different
moments, so the torch shots vary between browser captures: over five captures, `torch-campfire`
0.021 to 0.036 and `torch-lantern` 0.037 to 0.076 (before #757: 0.068 to 0.078 and 0.071 to
0.109). With the flicker held steady on both sides (reduced motion) they measure 0.0134 and
0.0137, and their mean luminance matches within 0.2 %: the point lights themselves agree.

Reproduce (the `frame-*` shots at `05a1e3eb` with this directory's test and script changes
applied, the others at `eb58c43b` with #757's):

```sh
bash scripts/fetch-bundles.sh && bash scripts/prepare-ranger-native.sh
MMORPG_LIGHTING_SHOTS="$PWD/artifacts/lighting" cargo test --release --locked -p mmorpg-client \
  --lib greyhaven_lighting_shots -- --ignored --nocapture
(cd web && bun install --frozen-lockfile && bun run build)
python scripts/capture-browser-reference.py docs/reviews/greyhaven-reference/views.json \
  --out artifacts/greyhaven-reference
# in a render-engine checkout, per shot:
cargo run --release --locked --manifest-path oracle/Cargo.toml -- --flip-images \
  <mmorpg>/docs/reviews/greyhaven-reference/re9-noon.png \
  <mmorpg>/artifacts/greyhaven-reference/noon.web.png noon.flip.png
```
