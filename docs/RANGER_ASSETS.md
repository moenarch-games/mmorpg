# Ranger assets in both clients

The accepted default remains the Dark archer. Both clients acquire only the default character and equipment they draw from asset-tooling's rolling [`humanoid-ranger-male` release](https://github.com/moritzbrantner/asset-tooling/releases/tag/humanoid-ranger-male), using its `asset-downloads-v1` index. Other skin, outfit and LOD variants are available upstream but are not selected by this update.

```sh
scripts/fetch-bundles.sh
```

The Ranger index, `export.ref.json`, selected glTF/bin files, shared KTX2 textures, attachment JSON and shared clips are retained together under the ignored `web/public/characters/humanoid.ranger.male/`. The Greyhaven inn keeps its existing release and acquisition path. Acquisition never fetches the Ranger review tar.

The rolling release is read at whatever it currently holds, unpinned (#507). Acquisition first downloads the live release's `export.ref.json` and keeps the local copy while it holds that export and every selected entry; after an owner upload it fetches the selected entries again, so a new upload never needs a consumer re-pin. When the release cannot be reached, a complete local copy is kept. To keep a comparison input across uploads, preserve it outside that directory. Acquisition fails only when the live index lacks an entry the game selects. Source and license information is maintained in [asset-tooling's humanoid source documentation](https://github.com/moritzbrantner/asset-tooling/blob/main/docs/humanoid-sources.md); `export.ref.json` identifies the corresponding bundle. `scripts/prepare-ranger-native.sh` prepares every tintable hairstyle the fetched index offers.

Tanner Hilda Brook is drawn on the female figure (#493), asset-tooling's rolling [`humanoid-ranger-female` release](https://github.com/moritzbrantner/asset-tooling/releases/tag/humanoid-ranger-female) (#378). The same acquisition fetches her default entry (sockets, clips and appearance), her `outfit.tanner.skin.light` variant, the `armor.tanner` belt and every tintable hairstyle into the ignored `web/public/characters/humanoid.ranger.female/`. It is read unpinned like the male release. `imported::Figure` names the two figures and `Resident::figure` puts Hilda on the female. Both clients load each figure as its own bundle, with its own skeleton, clips, sockets and `humanoid-appearance`. The female bundle has no headwear, beards, painted head skins or variety options, so her look simply has none. `presets::figure_appearance` adds the enemy presets to the male figure only.

## Browser

```sh
cd web
bun run dev
```

Open `/mmorpg/ranger-loader.html` for the small loading example, or `/mmorpg/` for the game. The example calls the same `QuaterniusUnits` loader and `ThreeSceneRenderer` as the game, loads the default character with bow and quiver, and plays `Walk_Loop` from the shared animation library. A successful load reports the clip count; a failed load reports an error and details remain in the console.

With a running server and the existing Playwright setup, check that same example with `python3 scripts/smoke-ranger-loader.py http://localhost:5173/mmorpg/ranger-loader.html` (optional `--screenshot <path>`).

The production loader uses Three.js `GLTFLoader`, `MeshoptDecoder`, `KTX2Loader`, `SkeletonUtils.clone` and `AnimationMixer`. KTX2 support is detected against the actual renderer that draws the scene. Vite copies the Basis JS/WASM transcoder from the pinned Three.js package. Relative glTF resources remain beside the index. The animation-only GLB is loaded once; its tracks bind to each cloned character's named joints.

The named Greyhaven residents wear their authored `humanoid-appearance` presets (#418). The four Greyhaven Guards share `resident.guard`, but each guard wears his own look that stays the same between frames and sessions (asset-tooling #380). The shared presenter marks them `Resident::varied` and seeds each look with the guard's NPC id. `presets::vary_preset` in `mmorpg-presentation` (the browser calls it through `mmorpg-wasm`) keeps the preset's uniform, faction colour and sword, and draws the rest from the bundle's own options: skin, hairstyle, hair tint (the beard and brows take the same tint), facial hair and face features. It also picks an age, a build, a height and a skin tone when the bundle offers `ages`, `builds`, `heights` and `skinTones`. Acquisition therefore fetches the uniform in both skins and every hairstyle's tintable `hair-1` variant. A guard wears the plain preset until his own look has loaded, or if it fails to load. When the bundle has painted head skins (`headSkins`), half the varied men also show stubble, and both clients put the `skin.head` texture of the man's skin, his age's painted skin (`ages[].skin`) and, with stubble, his hair tint's `stubbleShades` shade on the head material before the skin tone multiplies it. A man with no matching texture (the plain adult without stubble) keeps the authored skin. Acquisition fetches `skin.head` when the release has it.

Loose gear sways a little (Atlas 6.6.39, #498). Items stowed on the hanging sockets (`back.quiver`, `back.bow.stowed`, `back.stowed`, `hip_l.stowed`: the Ranger's quiver and bow) swing behind the body and settle when it starts, stops, sidesteps or turns. `units::sway` in `mmorpg-presentation` runs one damped spring per unit from its presented root motion, and both clients turn those items about their grip by the result (`ImportedUnit::swing`). The settings: about 2.2 Hz with a 0.45 damping ratio, at most about 10° either way, full detail within 25 m of the viewer, fading out by 35 m. Farther away the gear rests as authored and the spring does no work. Reduced motion turns the sway off. A unit that teleports more than 2.5 m in a frame, comes back after an interest gap or changes loadout, preset or outfit starts again from rest. Hair, beards and cloth (robes, skirts, aprons, mantles) stay skinned as authored, because the bundle has no accessory bones to swing them by (asset-tooling #391).

The humanoid enemies wear the Ranger too (#39), in presets the game authors over the bundle's own outfits: `presets::enemy_presets` in `mmorpg-presentation`, which both clients read, keyed by `Resident::preset` (`enemy.*`). Field Marauders wear the farmer's smock with long hair and a dagger. Redbrand Bandits wear the soldier's gambeson in Redbrand red, bareheaded, with a sword. Redbrand Crossbowmen wear the same uniform with the kettle hat and carry the bow and quiver, because the bundle has no crossbow; they loose with the bow release. Redbrand Brutes wear the knight's plate in red under the Ironcrest helm, with sword and shield. Garrick Redbrand wears the plate bareheaded in a deeper crimson, with long black hair and a full beard. The rank and file vary like the guards, each seeded by its creature id; Garrick keeps his authored look. The models scale to each template's collision height, and bodies and hitboxes stay as they are. Acquisition also fetches the knight and farmer outfits in both skins.

## Native

After acquisition, explicitly prepare the compatible native input:

```sh
scripts/prepare-ranger-native.sh
cargo run -p mmorpg-client --locked
```

Preparation requires Bun. It acquires the pinned asset-tooling processor into the user cache and, on Linux x86_64 when needed, Khronos KTX-Software 4.4.2. On other platforms install KTX-Software and set `ASSET_TOOLING_KTX` to its `ktx` CLI. `ASSET_TOOLING_ROOT` can select an existing compatible processor checkout. The processor uses glTF Transform and meshoptimizer to decode geometry and KTX-Software to transcode textures into ordinary PNGs embedded in GLB. These are prepared inputs, not another game-local decoder.

The command prepares the entries the native client draws: the character, the class weapons and starter outfits, and the residents' and enemies' outfit variants, armour, beards, straw hat and tintable hairstyles, and the painted head skins (`skin.head`) when the release has them. For the female figure it prepares Hilda's tanner outfit, its belt and her buns (#493). It writes self-contained GLBs and one separate decoded animation library per figure into each figure's ignored `native/` subdirectory. Repeat preparation after deliberately changing the acquired input. The native build embeds every prepared GLB and the original sockets descriptor; it performs no downloads. The standard `gltf` importer reads the models and library, maps animation targets to character joints by name, and uses the existing native pose/skinning path.

The native client draws the named residents and the humanoid enemies on the Ranger in the same shared presets and seeded variations as the browser, so a guard or bandit is the same man in both clients. It uses the outfit variant, armour less `hiddenArmor`, beards and hair in the preset's tint, headwear, faction colour, skin tone and height. It bakes the face, age and build morph targets into each look once (the face on `face.nodes`; age and build on the figure, plates, beards, hair and headwear, each mesh taking the targets it has), and the class starter outfits wear their preset's face. The painted head skin is sampled at the head's vertices, as every native texture is, so fine wrinkles show less than in the browser. If a preset's prepared entries are missing, the residents and enemies keep their procedural models.

The processing contract and producer commands are documented in [asset-tooling's release-download guide](https://github.com/moritzbrantner/asset-tooling/blob/main/docs/release-downloads.md). Acquisition, asset preparation, runtime presentation and gameplay remain separate owners.
