import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { chooseMatchingCli, lockedWasmBindgenVersion, parseCliVersion } from "../scripts/build-wasm";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("WASM build prerequisites", () => {
  test("the lockfile version satisfies the crate's non-exact wasm-bindgen requirement", () => {
    const requirement = /^wasm-bindgen = "([^"]+)"$/m.exec(read("../../crates/mmorpg-wasm/Cargo.toml"))?.[1];
    const resolved = lockedWasmBindgenVersion(read("../../Cargo.lock"));
    expect(requirement).toMatch(/^[0-9]+\.[0-9]+$/);
    expect(resolved.startsWith(`${requirement}.`)).toBe(true);
  });

  test("reads only well-formed CLI versions", () => {
    expect(parseCliVersion("wasm-bindgen 0.2.129\n")).toBe("0.2.129");
    expect(parseCliVersion("wasm-bindgen 0.2")).toBeNull();
    expect(parseCliVersion("wasm-pack 0.2.129")).toBeNull();
    expect(parseCliVersion("")).toBeNull();
  });

  test("prefers a compatible CLI and ignores newer incompatible global CLIs", () => {
    const cached = "/cache/wasm-bindgen";
    expect(chooseMatchingCli("0.2.130", "0.2.130", null, cached)).toBe("wasm-bindgen");
    expect(chooseMatchingCli("0.2.130", "0.2.140", "0.2.130", cached)).toBe(cached);
    expect(chooseMatchingCli("0.2.130", "0.2.140", null, cached)).toBeNull();
  });

  test("fails closed when the lockfile lacks wasm-bindgen", () => {
    expect(() => lockedWasmBindgenVersion('[[package]]\nname = "serde"\nversion = "1.0.0"\n')).toThrow();
  });
});
