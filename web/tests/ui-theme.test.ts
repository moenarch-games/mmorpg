import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { themeCss } from "../scripts/ui-theme";

test("the shared theme becomes CSS custom properties", () => {
  const css = themeCss(readFileSync(new URL("../../content/ui/theme.json", import.meta.url), "utf8"));
  assert.match(css, /--ui-color-gold: #c6a85f;/);
  assert.match(css, /--ui-color-gold-bright: #e5cf86;/);
  assert.match(css, /--ui-radius-panel: 10px;/);
  assert.match(css, /--ui-text-display: 30px;/);
  assert.match(css, /--ui-motion-fast: 120ms;/);
});

test("a malformed theme fails closed", () => {
  assert.throws(() => themeCss(JSON.stringify({ schema_version: 2 })), /schema/);
  assert.throws(() => themeCss(JSON.stringify({ schema_version: 1, color: { gold: "gold" } })), /color.gold/);
  assert.throws(() => themeCss(JSON.stringify({ schema_version: 1, radius: { panel: "10" } })), /radius.panel/);
});
