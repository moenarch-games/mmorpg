import { expect, test } from "bun:test";
import { hasOnlineCharacterId, onlineCharacterId } from "../src/world/online-character-identity";

function storage() {
  const values = new Map<string, string>();
  return { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
}

test("online aliases persist per profile and retire local slots without storing progression", () => {
  const first = storage();
  const second = storage();
  const one = () => "1".repeat(32);
  const two = () => "2".repeat(32);
  const id = onlineCharacterId(first, "local-1", one);
  expect(onlineCharacterId(first, "local-1", two)).toBe(id);
  expect(onlineCharacterId(second, "local-1", two)).not.toBe(id);
  expect(hasOnlineCharacterId(first, "local-1")).toBe(true);
  expect(hasOnlineCharacterId(first, "local-2")).toBe(false);
  expect(onlineCharacterId(first, "aelric-stormward", two)).toBe(`online-${two()}`);
});
