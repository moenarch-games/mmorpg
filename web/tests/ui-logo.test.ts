import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { logoSvg, splashHtml } from "../scripts/ui-logo";

const logo = readFileSync(new URL("../../content/ui/logo.json", import.meta.url), "utf8");

test("the shared logo becomes SVG in theme colours, timed only on the splash", () => {
  const animated = logoSvg(logo, true);
  assert.match(animated, /^<svg class="logo" viewBox="0 0 100 100"/);
  assert.equal(animated.match(/<circle /g)?.length, 2);
  assert.match(animated, /class="logo-gem" style="--start:1450ms;--length:500ms"/);
  assert.match(animated, /stroke="var\(--ui-color-gold-bright\)"/);
  assert.doesNotMatch(logoSvg(logo, false), /--start/);
});

test("the splash carries the title and the hold and fade", () => {
  const splash = splashHtml(logo);
  assert.match(splash, /id="splash" class="splash" style="--hold:2700ms;--fade:650ms"/);
  assert.match(splash, />GREYHAVEN</);
  assert.match(splash, />MMORPG</);
});

test("a logo of another schema or an unknown colour fails closed", () => {
  assert.throws(() => logoSvg(JSON.stringify({ schema_version: 2 }), false), /schema/);
  const bad = JSON.parse(logo);
  bad.shapes[0].color = "red; x";
  assert.throws(() => logoSvg(JSON.stringify(bad), false), /colour/);
});
