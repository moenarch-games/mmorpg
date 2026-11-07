/** Runs the native A/B assertions, fixtures and exact recovery checks in real WASM. */
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
function run(command: string, args: string[], timeout = 2 * 60 * 1000): string {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], timeout, maxBuffer: 64 * 1024 * 1024 });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.status ?? result.signal})`);
  }
  return result.stdout;
}
run("cargo", ["build", "--release", "--locked", "-p", "mmorpg-core", "--example", "physics_ab_wasm", "--features", "physics-ab", "--target", "wasm32-unknown-unknown"], 20 * 60 * 1000);
const metadata: { target_directory: string } = JSON.parse(run("cargo", ["metadata", "--no-deps", "--locked", "--format-version", "1"]));
const output = join(metadata.target_directory, "physics-ab-wasm");
mkdirSync(output, { recursive: true });
run("wasm-bindgen", ["--target", "nodejs", "--out-dir", output, "--out-name", "physics_ab", join(metadata.target_directory, "wasm32-unknown-unknown", "release", "examples", "physics_ab_wasm.wasm")]);
const require = createRequire(import.meta.url);
const acceptance: { workloads(): string; check_workload_wasm(name: string): void } & Record<string, () => void> = require(join(output, "physics_ab.js"));
const names = acceptance.workloads().split("\n");
if (!process.argv.includes("--authored")) {
  for (const name of names) {
    console.log(`RUN ${name}`);
    acceptance.check_workload_wasm(name);
    console.log(`PASS ${name}: WASM A/B replay, exact midpoint recovery, invariants and query parity`);
  }
  console.log(`${names.length}/${names.length} WASM A/B workloads passed`);
}

const authored = ["mill_towpath_departure_settles_before_horizontal_motion", "redbrand_json_rays_keep_floor_shelf_wall_and_ceiling_contacts", "unit_placement_capsules_query_actual_meshes_and_hulls_on_both_backends", "engine_surface_features_are_stable_for_the_actual_authored_inputs", "real_rock_hull_ray_and_capsule_cast_retain_game_identity", "authored_rigid_boxes_keep_contacts_replay_and_exact_midpoint_recovery", "an_authored_wall_scrape_preserves_ascent_but_its_underside_stops_it", "terrain_probes_agree_at_the_charge_paths_half_unit_rounding_tie", "the_slanted_redbrand_wall_still_stops_an_upward_move", "redbrand_entrance_walls_shelf_ceiling_and_recovery", "meshes_support_without_heightfield_and_change_content_identity", "millbrook_windmill_has_a_solid_round_footprint", "hollow_tent_flaps_block_and_sloped_roofs_have_no_square_corners"];
for (const name of authored) {
  const check = acceptance[name];
  if (!check) {
    throw new Error(`Missing WASM authored check ${name}`);
  }
  console.log(`RUN ${name}`);
  check();
  console.log(`PASS ${name}: real authored geometry in WASM`);
}
