# Greyhaven PBR materials: wood, rock and foliage (#53)

`browser-native.jpg` shows the shipped game camera at real GPU: the top row is the browser (Chrome over ANGLE on an RTX 3060 Ti), the bottom row the native client (`greyhaven_ground_shots`, offscreen). From left to right: the market square's `settlement-props` wood, a `limestone-rocks` outcrop with the `river-stones` beside the brook, and the `greyhaven-nature` trees. The two rows are different times of day.

What the materials are: the packaged glTF base colour, normal and metallic-roughness textures of those bundles, through three.js `GLTFLoader` and `LitMaterial` in the browser and through `crate::nature` and render-engine natively. Foliage keeps its alpha masks and double sides.

Browser observations (`window.__valeDebug`, `?debug`): `limestone` draws 72 glTF image references as 3 textures, `climbing` 10 and `river` 3; `greyhaven-nature` shares its 49 references down to the 31 distinct images. No renderer-specific texture memory figure is claimed.
