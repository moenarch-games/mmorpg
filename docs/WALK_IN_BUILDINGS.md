# Walk-in building presentation

Both clients use `mmorpg-presentation::buildings::WalkIn` for entry transforms,
door animation, roof hiding and interior camera distance. The inn and Marshal's
house retain their existing footprint behavior.

The monuments kit is fetched by `scripts/fetch-bundles.sh` into
`web/public/buildings/monuments.kit`; native builds embed its two walk-in entries.
`Spec::mausoleum(descriptor, origin, quarters)` reads the kit's descriptor
attachment, including `interior.volume` and `collisionProxies`. The browser's
`BuildingPresenter.mausoleum` exposes the same constructor. Origin is in zone
units and receives the usual scenery relief; quarter turns rotate about +Y.
Neither constructor registers a zone placement. Placing the mausoleum and its
authoritative collision remains #683.

The room volume, including its vertical bounds, decides whether the roof hides;
the porch and stepped footprint do not. Both entries retain the same transform,
and no door or synthetic door ID is required. The shared camera boom clips its
sight line against the actual authored wall, column and furnishing boxes, leaving
0.3 m clearance. It preserves ordinary orbit heading, pitch and user zoom, pulls
in immediately when required to avoid showing an obstructing wall, and eases back
out at the normal zoom rate. Reduced motion restores distance immediately.
Selective wall fading is a later improvement, as approved in Atlas 6.3.i714.

The Greyhaven inn and Marshal's home have separate authoritative closed gable meshes in
`mmorpg-core` (IDs 148 and 199). Each follows the pitched outer roof and extends down to
the real ceiling underside; the enclosed attic is not traversable. These static shapes
remain solid even when `WalkIn` hides the roof and ceiling for the indoor camera. Entering
and leaving still requires the open doorway, and jumping or mantling cannot use the
cutaway as a hole. The cutaway is limited to feet below the actual ceiling, so a
character above the roof cannot make it disappear. The zone content hash and recovery
include both meshes.

An interior presenter also supplies sky occlusion / exposure (`lighting`) and a
steady warm 1,800 lm lamp above the room centre (`light`). The browser building
collection composes these with scenery lights; the native world uses the same
building collection as its camera and draws. New placements must enter that
shared collection so their transforms, room, camera and lighting stay together.

The focused Rust building test uses an excerpt of the actual monuments release
descriptor (bundle ref `77387a7c0f9c7e193bd6e2f0e546d0c8c2da6d0d7ec50be987c3328593ebbc75`,
descriptor attachment `f2b9e29ab4242ee412658534458fc57eb79a4c85de5af886f696cd8791e8f81a`).
It checks the presentation contract without adding a mausoleum to Greyhaven.
Runtime visual acceptance of a placed mausoleum remains unverified until #683.
