# Greyhaven World Studio

A local web app for reshaping the valley: find a place on a map rendered from the
game, paint it in Krita, bring the paint back, and hand the consequences to an agent.
It builds on the scripts in [the Krita workflow](WORLD_LAYOUT_EDITOR.md#painting-terrain-and-biomes-in-krita).

## Start it

Needs a repository set up with `mise run setup`, and Krita to paint with.

```sh
(cd crates/mmorpg-world-studio/frontend && bun install --frozen-lockfile && bun run build)
cargo run -p mmorpg-world-studio
```

Open <http://127.0.0.1:8737/>. On first use, **Render the map** builds the game and
photographs the zone into `artifacts/studio/world/tiles`.

## The workflow

1. **Frame the place.** Pan and zoom until the frame covers it, and choose its size and detail.
   The map runs on `@moritzbrantner/maps`.
2. **Paint it in Krita.** Download the layered document, paint the `paint height` and
   `paint biome` layers, save as `.ora`, and upload it. The layer images are also shown.
3. **Change the terrain.** The studio reports what the paint would change. Applying it creates
   branch `world-edit/<id>` in a worktree under `artifacts/studio/worktrees/`. The checkout
   the studio runs from stays untouched.
4. **Let the agent reconcile it.** Describe what the place should become. The studio starts
   `claude -p` in that worktree with a brief of what changed (`src/brief.rs`). It may edit
   files, run `cargo test`, `check` and `fmt`, and commit to the branch, up to $5. It cannot
   push. Review the branch like any other, and remove it with
   `git worktree remove artifacts/studio/worktrees/<id>` when done.

Set `WORLD_STUDIO_AGENT` to run a different executable with the same arguments.

## Finding clear ground

To place an animal, prop or spawn, ask `mmorpg-scenery`'s `clear-spot` example for the nearest
clear ground (#863). Coordinates and distances are in units (100 per metre), as content source
writes them:

```sh
cargo run --release -p mmorpg-scenery --example clear-spot -- --near 52000,66000 --radius 300 --off-routes --count 5
```

It indexes Greyhaven once (about 2 s): colliders and static meshes, props with colliders, the
cottages' footprints, roads, lakes, lake beds and rivers. Each spot printed has nothing within
`--radius` (default 200) and lies at least two radii from the others. `--shore` keeps spots
300–900 units from water, `--off-routes` keeps clear of every resident's walk home and errand
legs, and `--avoid-fields` keeps off the farmland's fields. `--max-distance` (default 20000)
and `--step` (default 50) bound the search. It prints each spot in units and metres, with its
ground height and its distance from water within 900 units. It exits with status 1 when no
spot is clear.

## Boundaries

The studio is development tooling: no game crate links it and it links none
(`mmorpg-game-server/tests/dependency_boundary.rs` checks the resolved graph). It does run game
code, as a separate program rather than a link: it asks each checkout for its terrain by running
`cargo run -p mmorpg-scenery --example export-world-layers` there (`src/grid.rs`), so a
branch's worktree answers with that branch's paint. One program serves the app and offers the
same capture and import as `capture` and `import` commands. It starts programs and creates
branches, so it listens only on the loopback address and refuses requests whose `Host` or
`Origin` is anywhere else; do not put it behind a proxy.

Jobs run one at a time. Exports and edits survive a restart in `artifacts/studio/state.json`;
job logs do not. Exports render from the last `web/dist`, and an edit starts from the current
`HEAD`, so merge or discard one edit before starting another on the same ground. The map shows
the last render: use **Re-render map** after merging an edit.
