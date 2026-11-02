/**
 * Builds `mmorpg-wasm` for the browser and generates its wasm-bindgen bindings
 * into the ignored `web/src/generated/mmorpg-wasm` directory.
 *
 * Prerequisites: the `wasm32-unknown-unknown` target (installed through
 * `rust-toolchain.toml`). The matching wasm-bindgen CLI is selected from PATH
 * or automatically installed in an isolated Cargo-target cache.
 *
 * Idempotent: cargo rebuilds incrementally, and the generated directory is
 * replaced as a whole only after wasm-bindgen succeeded.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CRATE = "mmorpg-wasm";
const ARTIFACT = "mmorpg_wasm";
const TARGET = "wasm32-unknown-unknown";
const CARGO_BUILD_TIMEOUT_MS = 20 * 60 * 1000;
const TOOL_TIMEOUT_MS = 2 * 60 * 1000;

export const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(webRoot, "..");
const generatedRoot = join(webRoot, "src", "generated");
export const generatedDirectory = join(generatedRoot, "mmorpg-wasm");

/** The wasm-bindgen version resolved by Cargo.lock, which the generated WASM requires. */
export function lockedWasmBindgenVersion(lockfile: string): string {
  const match = /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lockfile);
  if (!match?.[1]) {
    throw new Error("Cargo.lock does not resolve the wasm-bindgen crate.");
  }
  return match[1];
}

/** Parses `wasm-bindgen --version` output such as `wasm-bindgen 0.2.129`. */
export function parseCliVersion(output: string): string | null {
  return /^wasm-bindgen ([0-9]+\.[0-9]+\.[0-9]+)\b/.exec(output.trim())?.[1] ?? null;
}

function run(command: string, args: readonly string[], timeout: number): string {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    timeout,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    throw new Error(`${command} could not run: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with ${result.signal ?? `exit status ${result.status}`}.`);
  }
  return result.stdout;
}

/** Pick only a CLI compatible with the version compiled into the WASM module. */
export function chooseMatchingCli(
  expected: string,
  onPathVersion: string | null,
  cachedVersion: string | null,
  cachedExecutable: string,
): string | null {
  if (onPathVersion === expected) return "wasm-bindgen";
  if (cachedVersion === expected) return cachedExecutable;
  return null;
}

function cliVersion(executable: string): string | null {
  try {
    return parseCliVersion(run(executable, ["--version"], TOOL_TIMEOUT_MS));
  } catch {
    return null;
  }
}

/** Never overwrite a newer system CLI; install the matching executable locally if needed. */
function matchingCli(expected: string, targetDirectory: string): string {
  const installRoot = join(targetDirectory, "tools", "wasm-bindgen-cli", expected);
  const cachedExecutable = join(installRoot, "bin", process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen");
  const existing = chooseMatchingCli(
    expected, cliVersion("wasm-bindgen"), cliVersion(cachedExecutable), cachedExecutable,
  );
  if (existing) return existing;

  console.log(`Installing wasm-bindgen CLI ${expected} into ${installRoot} (required by Cargo.lock).`);
  run("cargo", [
    "install", "--locked", "--force", "--version", `=${expected}`,
    "--root", installRoot, "wasm-bindgen-cli",
  ], CARGO_BUILD_TIMEOUT_MS);
  if (cliVersion(cachedExecutable) !== expected) {
    throw new Error(`The installed wasm-bindgen CLI does not match Cargo.lock's version ${expected}.`);
  }
  return cachedExecutable;
}

function cargoTargetDirectory(): string {
  const metadata: unknown = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], TOOL_TIMEOUT_MS),
  );
  if (typeof metadata !== "object" || metadata === null || !("target_directory" in metadata) ||
      typeof metadata.target_directory !== "string") {
    throw new Error("cargo metadata did not report a target directory.");
  }
  return metadata.target_directory;
}

/** Deletes only a path strictly inside `web/src/generated`. */
function removeGenerated(path: string): void {
  const inside = relative(generatedRoot, path);
  if (inside === "" || inside.startsWith("..") || isAbsolute(inside)) {
    throw new Error(`Refusing to delete ${path} outside ${generatedRoot}.`);
  }
  rmSync(path, { recursive: true, force: true });
}

/** Builds the release WASM module and returns the generated bindings directory. */
export function buildWasm(): string {
  const expected = lockedWasmBindgenVersion(readFileSync(join(repoRoot, "Cargo.lock"), "utf8"));
  const targetDirectory = cargoTargetDirectory();
  const bindgenCli = matchingCli(expected, targetDirectory);
  run("cargo", ["build", "--release", "--locked", "--target", TARGET, "-p", CRATE], CARGO_BUILD_TIMEOUT_MS);
  const wasm = join(targetDirectory, TARGET, "release", `${ARTIFACT}.wasm`);
  if (!existsSync(wasm)) {
    throw new Error(`cargo did not produce ${wasm}.`);
  }
  mkdirSync(generatedRoot, { recursive: true });
  const staging = join(generatedRoot, `.mmorpg-wasm-${process.pid}`);
  removeGenerated(staging);
  try {
    run(bindgenCli, ["--target", "web", "--out-name", ARTIFACT, "--out-dir", staging, wasm], TOOL_TIMEOUT_MS);
    removeGenerated(generatedDirectory);
    renameSync(staging, generatedDirectory);
  } finally {
    removeGenerated(staging);
  }
  return generatedDirectory;
}

if (import.meta.main) {
  try {
    const directory = buildWasm();
    console.log(`Generated ${relative(webRoot, directory)}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
