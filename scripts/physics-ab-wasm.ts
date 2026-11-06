/** Runs the native A/B assertions, fixtures and exact recovery checks in real WASM. */
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
function run(command: string, args: string[]): string {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.status ?? result.signal})`);
  }
  return result.stdout;
}
run("cargo", ["build", "--release", "--locked", "-p", "mmorpg-core", "--example", "physics_ab_wasm", "--features", "physics-ab", "--target", "wasm32-unknown-unknown"]);
const metadata: { target_directory: string } = JSON.parse(run("cargo", ["metadata", "--no-deps", "--locked", "--format-version", "1"]));
const output = join(metadata.target_directory, "physics-ab-wasm");
mkdirSync(output, { recursive: true });
run("wasm-bindgen", ["--target", "nodejs", "--out-dir", output, "--out-name", "physics_ab", join(metadata.target_directory, "wasm32-unknown-unknown", "release", "examples", "physics_ab_wasm.wasm")]);
const require = createRequire(import.meta.url);
const acceptance: { workloads(): string; check_workload_wasm(name: string): void } = require(join(output, "physics_ab.js"));
const names = acceptance.workloads().split("\n");
for (const name of names) {
  acceptance.check_workload_wasm(name);
  console.log(`PASS ${name}: WASM A/B replay, exact midpoint recovery, invariants and query parity`);
}
console.log(`${names.length}/${names.length} WASM A/B workloads passed`);
