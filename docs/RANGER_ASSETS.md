# Ranger assets in both clients

The accepted default remains the Dark archer. Both clients acquire only the default character and equipment they draw from asset-tooling's rolling [`humanoid-ranger-male` release](https://github.com/moritzbrantner/asset-tooling/releases/tag/humanoid-ranger-male), using its `asset-downloads-v1` index. Other skin, outfit and LOD variants are available upstream but are not selected by this update.

```sh
scripts/fetch-bundles.sh
```

The Ranger index, `export.ref.json`, selected glTF/bin files, shared KTX2 textures, attachment JSON and shared clips are retained together under the ignored `web/public/characters/humanoid.ranger.male/`. The Greyhaven inn keeps its existing release and acquisition path. Acquisition never fetches the Ranger review tar.

Re-running acquisition keeps the local rolling input. To deliberately adopt a newer rolling output, first preserve any comparison input you need outside that directory, remove its `RELEASE` file, and run acquisition again. Builds do not silently refresh accepted assets. Source and license information is maintained in [asset-tooling's humanoid source documentation](https://github.com/moritzbrantner/asset-tooling/blob/main/docs/humanoid-sources.md); `export.ref.json` identifies the corresponding bundle; acquisition is pinned to the committed copy in `web/assets/ranger/export.ref.json` (the export with face morph targets, #411), refetches when the local copy differs and fails if the release serves another export. No per-file consumer hash list is required.

## Browser

```sh
cd web
bun run dev
```

Open `/mmorpg/ranger-loader.html` for the small loading example, or `/mmorpg/` for the game. The example calls the same `QuaterniusUnits` loader and `ThreeSceneRenderer` as the game, loads the default character with bow and quiver, and plays `Walk_Loop` from the shared animation library. A successful load reports the clip count; a failed load reports an error and details remain in the console.

With a running server and the existing Playwright setup, check that same example with `python3 scripts/smoke-ranger-loader.py http://localhost:5173/mmorpg/ranger-loader.html` (optional `--screenshot <path>`).

The production loader uses Three.js `GLTFLoader`, `MeshoptDecoder`, `KTX2Loader`, `SkeletonUtils.clone` and `AnimationMixer`. KTX2 support is detected against the actual renderer that draws the scene. Vite copies the Basis JS/WASM transcoder from the pinned Three.js package. Relative glTF resources remain beside the index. The animation-only GLB is loaded once; its tracks bind to each cloned character's named joints.

The named Greyhaven residents wear their authored `humanoid-appearance` presets (#418). The four Greyhaven Guards share `resident.guard`, but each guard wears his own look that stays the same between frames and sessions (asset-tooling #380). The shared presenter marks them `Resident::varied` and seeds each look with the guard's NPC id. `varyPreset` in `quaternius-units.ts` keeps the preset's uniform, faction colour and sword, and draws the rest from the bundle's own options: skin, hairstyle, hair tint (the beard and brows take the same tint), facial hair and face features. It also picks an age, a build, a height and a skin tone when the bundle offers `ages`, `builds`, `heights` and `skinTones`. Acquisition therefore fetches the uniform in both skins and every hairstyle's tintable `hair-1` variant. A guard wears the plain preset until his own look has loaded, or if it fails to load. The painted age head skins (`headSkins`) are not applied yet.

## Native

After acquisition, explicitly prepare the compatible native input:

```sh
scripts/prepare-ranger-native.sh
cargo run -p mmorpg-client --locked
```

Preparation requires Bun. It acquires the pinned asset-tooling processor into the user cache and, on Linux x86_64 when needed, Khronos KTX-Software 4.4.2. On other platforms install KTX-Software and set `ASSET_TOOLING_KTX` to its `ktx` CLI. `ASSET_TOOLING_ROOT` can select an existing compatible processor checkout. The processor uses glTF Transform and meshoptimizer to decode geometry and KTX-Software to transcode textures into ordinary PNGs embedded in GLB. These are prepared inputs, not another game-local decoder.

The command prepares only the character, sword and shield used by the native client. It writes self-contained GLBs and one separate decoded animation library into the ignored `native/` subdirectory. Repeat preparation after deliberately changing the acquired input. The native build embeds those files and the original sockets descriptor; it performs no downloads. The standard `gltf` importer reads the models and library, maps animation targets to character joints by name, and uses the existing native pose/skinning path. Native still carries the sword/shield only; browser bow/quiver support does not add native equipment behavior.

The processing contract and producer commands are documented in [asset-tooling's release-download guide](https://github.com/moritzbrantner/asset-tooling/blob/main/docs/release-downloads.md). Acquisition, asset preparation, runtime presentation and gameplay remain separate owners.
