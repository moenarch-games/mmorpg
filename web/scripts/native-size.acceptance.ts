import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { sizeEvidence } from "coding-tooling/size-evidence";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const baselinePath = join(root, ".performance/baselines/mmorpg-core.json");
function run(args: string[], env = process.env) {
  return spawnSync(process.execPath, [join(root, "web/scripts/native-size.ts"), ...args], {
    cwd: root,
    env,
    encoding: "utf8",
    timeout: 20 * 60 * 1000,
    maxBuffer: 4 * 1024 * 1024,
  });
}
const captured = run(["--capture"]);
assert.equal(captured.status, 0, captured.stderr);
const baseline = readFileSync(baselinePath);
const checked = run([]);
assert.equal(checked.status, 0, checked.stderr);
assert.deepEqual(readFileSync(baselinePath), baseline);
const overridden = run([], { ...process.env, RUSTFLAGS: "-C target-cpu=native" });
assert.equal(overridden.status, 2, overridden.stderr);
assert.equal(JSON.parse(overridden.stdout).status, "unavailable");
const compilerOverride = run([], { ...process.env, RUSTC: "/mmorpg-size-missing-rustc" });
assert.equal(compilerOverride.status, 2, compilerOverride.stderr);
assert.equal(JSON.parse(compilerOverride.stdout).status, "unavailable");
const emptyEncodedFlags = run([], { ...process.env, CARGO_ENCODED_RUSTFLAGS: "" });
assert.equal(emptyEncodedFlags.status, 2, emptyEncodedFlags.stderr);
assert.equal(JSON.parse(emptyEncodedFlags.stdout).status, "unavailable");

const fixture = mkdtempSync(join(tmpdir(), "mmorpg-native-size-"));
try {
  for (const name of [
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTC_BOOTSTRAP",
    "RUSTC_FORCE_INCREMENTAL",
    "CARGO_BUILD_RUSTC",
    "CARGO_INCREMENTAL",
  ]) {
    const override = run([], { ...process.env, [name]: "undeclared" });
    assert.equal(override.status, 2, override.stderr);
    assert.equal(JSON.parse(override.stdout).status, "unavailable");
  }
  const tools = join(fixture, "tools");
  mkdirSync(tools);
  const cargo = spawnSync("rustup", ["which", "cargo"], { encoding: "utf8", timeout: 10000 });
  assert.equal(cargo.status, 0, cargo.stderr);
  symlinkSync(cargo.stdout.trim(), join(tools, "cargo"));
  const missingCompiler = run([], { ...process.env, PATH: tools });
  assert.equal(missingCompiler.status, 2, missingCompiler.stderr);
  assert.equal(JSON.parse(missingCompiler.stdout).status, "unavailable");
  const cargoHome = join(fixture, "cargo-home");
  mkdirSync(cargoHome);
  writeFileSync(join(cargoHome, "config.toml"), '[build]\nrustc = "/mmorpg-size-missing-rustc"\n');
  const inheritedConfiguration = run([], { ...process.env, CARGO_HOME: cargoHome });
  assert.equal(inheritedConfiguration.status, 2, inheritedConfiguration.stderr);
  assert.equal(JSON.parse(inheritedConfiguration.stdout).status, "unavailable");
  const declarationPath = join(fixture, ".performance/size.json");
  const declarationText = readFileSync(join(root, ".performance/size.json"), "utf8");
  const measurement = sizeEvidence(root);
  assert.equal(measurement.status, "passed");
  const target = measurement.data.targets[0];
  assert.ok(target);
  for (const path of [
    ...target.inputs.buildInputs,
    ...target.artifacts.map((artifact) => artifact.path),
  ]) {
    mkdirSync(dirname(join(fixture, path)), { recursive: true });
    copyFileSync(join(root, path), join(fixture, path));
  }
  mkdirSync(dirname(declarationPath), { recursive: true });
  writeFileSync(declarationPath, declarationText);
  const fixtureBaseline = join(fixture, "baseline.json");
  const fixtureBaselineBytes = Buffer.from(JSON.stringify(measurement));
  writeFileSync(fixtureBaseline, fixtureBaselineBytes);
  const compare = () => sizeEvidence(fixture, { baseline: fixtureBaseline });
  const compatible = compare();
  assert.equal(compatible.status, "passed");
  assert.deepEqual(compatible.data.targets[0]?.comparison, {
    state: "comparable",
    baselineBytes: target.bytes,
    deltaBytes: 0,
  });
  assert.equal(
    sizeEvidence(fixture, { baseline: join(fixture, "missing.json") }).status,
    "unavailable",
  );
  for (const change of [
    { target: "aarch64-unknown-linux-gnu" },
    { features: ["different-feature"] },
  ]) {
    const declaration = JSON.parse(declarationText);
    Object.assign(declaration.targets[0], change);
    writeFileSync(declarationPath, JSON.stringify(declaration));
    const result = compare();
    assert.equal(result.status, "unavailable");
    assert.equal(result.data.targets[0]?.comparison.state, "incomparable");
  }
  const missingTool = JSON.parse(declarationText);
  missingTool.targets[0].toolchains = [["mmorpg-size-missing-tool"]];
  writeFileSync(declarationPath, JSON.stringify(missingTool));
  assert.equal(compare().status, "unavailable");
  writeFileSync(declarationPath, declarationText);
  const artifact = join(fixture, target.inputs.entrypoint);
  rmSync(artifact);
  assert.equal(compare().status, "unavailable");
  copyFileSync(join(root, target.inputs.entrypoint), artifact);
  writeFileSync(artifact, Buffer.concat([readFileSync(artifact), Buffer.alloc(65537)]));
  assert.equal(compare().status, "failed");
  assert.deepEqual(readFileSync(fixtureBaseline), fixtureBaselineBytes);
  console.log(
    "Native size acceptance passed: ordinary gate preserves baseline, ambient flags and compiler/wrapper/incremental overrides unavailable, missing compiler and inherited configuration unavailable, compatible archive, missing baseline, target/features incomparable, missing tool/artifact, growth failure.",
  );
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
