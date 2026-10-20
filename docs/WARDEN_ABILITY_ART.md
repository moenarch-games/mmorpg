# Approved Warden ability artwork

The browser action bar uses the owner's selected painted PNGs for **Heroic Strike (1), Shield Bash (2), Rallying Cry (3) and Cleave (4)**. The existing key labels, learned/locked appearance, resource/range feedback, cooldown/GCD sweeps, cast state and tooltips remain separate controls/layers. Charge (17) and other abilities retain their existing glyphs. This changes presentation only; ability legality and outcomes still come from the zone.

## Source and distribution

The producer is asset-tooling commit **`11b19021779349fbb2b465956b5ff609b48f0fc5`** ([PR #241](https://github.com/moritzbrantner/asset-tooling/pull/241)), `review/warden-icons/approved/`, profile `approved-png-v1`. Manifest SHA-256: **`a59d54c4191a07de613769726ad1c41513f5256f0289281fb0b000e7fcdeddc5`**. The producer's exact owner selection supersedes the earlier three-icon SD 1.5 experiment; it includes all four approved FLUX.2 [klein] candidates and their license/model/workflow provenance.

`web/assets/warden-icons/producer-manifest.json` preserves that exact manifest; `source.json` records the handoff and attribution. The game intentionally packages only the eight **64/512 px display derivatives**, unchanged, as `warden-<ability>-<size>.png`; it does not claim this subset is a complete producer bundle. Vite bundles the images under content-based URLs. Desktop uses the compact image; high-DPI uses the approved larger derivative. Images are created only when slots change, with browser decoding/caching and no per-frame fetch/decoder.

The original artwork was generated through ComfyUI with FLUX.2 [klein] 4B (Black Forest Labs, Apache-2.0), using the project's own briefs. The approved manifest retains exact master/derivative identities and rights provenance. Neither the game nor normal builds require ComfyUI/MCP, model weights, FFmpeg or live generation.

Every normal `bun run build` / `bun run dev` checks the fixed producer-manifest content pin, required stable ability IDs and selected file lengths/hashes before serving the UI. Missing, corrupt, swapped or unapproved image bytes fail clearly. This consumer check is a fixed selection of verified outputs, not a second asset bundle/image parser.

## Reconcile the source

Use a separate clean asset-tooling worktree at the declared commit, hydrate its approved review folder and install its locked dependencies, then from MMORPG:

```sh
bun web/scripts/package-warden-icons.ts /path/to/asset-tooling-warden-handoff --check
```

`--write` intentionally recopies this exact selection through the producer's `verifyAssetBundle` / `readAssetBundleAsset` API. The producer's full 20-entry export is verified before copying the eight selected display derivatives. It never regenerates approved art. Source acquisition/LFS hydration is separate from offline package verification.

## Focused evidence

- Four Bun package tests cover stable ID mapping, missing images, corrupted/swapped bytes and retargeted producer metadata.
- The existing real Chromium Warden combat proof checks the four loaded painted images plus the fifth Charge glyph, locked levels, Rage consumption on Heroic Strike, desktop/context-menu tooltip and a real touch long-press at 390×844 / DPR 2. It uses the actual WASM zone and normal action bar.
- Independently verified and decoded all 20 producer PNGs from the consumer checkout without generation/network during verification. The actual [published producer review](https://moritzbrantner.github.io/asset-tooling/review/warden-icons/) displays approved 512/64/48/32 views; the served derivatives matched their approved hashes.
- [Before/after evidence](reviews/warden-hotbar/README.md) distinguishes the existing public build from the local candidate build. Public game publication is verified separately after merge/deployment. The deployed public game passed the same focused Chromium proof after Pages deployment of `048a83015deaf13cbaf57a5472480dbe29ecb582`.

```sh
(cd web && bun test tests/warden-icons.test.ts && bun run build)
uv run --with playwright==1.57.0 python scripts/smoke-browser.py BrowserAcceptance.test_warden_action_bar_rage_and_combat_text
```

Shared convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.

## Native reuse (#171)

The native egui/wgpu HUD embeds the same selected 512 px PNG bytes. Its build checks the exact producer manifest, stable IDs, lengths and SHA-256 hashes. The existing egui texture manager receives four color and four grayscale locked textures once; repeated frames reuse them. The PNG decoder has 512×512 dimensions and a 4 MiB allocation limit. Runtime decode failures are cached and displayed beside the bar. No remote fetch or generation is involved.

The four painted slots retain their labels, unlock levels, resource tint, cooldown overlays, cast selection and existing semantic slot actions. Charge and other classes retain their text faces.

Native evidence uses the actual offscreen wgpu renderer and Greyhaven simulation, rather than a physical window: [1280×720](reviews/warden-hotbar/native-warden.png) and [2560×1440 at 2× scale](reviews/warden-hotbar/native-warden-2x.png). The existing native smoke run passed real host connection/reconnection, GPU HUD/world rendering and the two-client inn interaction. Focused control/HUD tests cover semantic keys and projected cooldown/cast state; the Warden slot-one proof starts with an authoritative nearby-enemy/rage fixture and applies Heroic Strike through the normal semantic slot mapping.

```sh
python3 scripts/smoke-native.py
cargo test -p mmorpg-client --locked --test native_ability_art --test native_ability_controls --test combat_hud
MMORPG_NATIVE_ABILITY_ART_EVIDENCE=1 MMORPG_NATIVE_ABILITY_ART_DPI=2 MMORPG_SMOKE_FRAME_DIR=artifacts/native-warden cargo test -p mmorpg-client --locked --lib the_egui_interface_paints_over_the_native_world -- --ignored --nocapture
```
