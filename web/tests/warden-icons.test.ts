import { describe, expect, test } from "bun:test";
import { cp, mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { WARDEN_ICON_DIRECTORY, verifyWardenIcons } from "../scripts/warden-icon-package";
import { artworkOf } from "../src/world/units/ability-presentation";

async function packageCopy(run: (directory: string) => Promise<void>): Promise<void> {
  const directory = await mkdtemp(path.join(os.tmpdir(), "mmorpg-warden-icons-"));
  try {
    await cp(WARDEN_ICON_DIRECTORY, directory, { recursive: true });
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe("approved Warden display package", () => {
  test("all four stable ability IDs resolve approved bundled regular and high-DPI images", async () => {
    await verifyWardenIcons();
    for (const id of [1, 2, 3, 4]) {
      const artwork = artworkOf(id);
      expect(artwork?.regular).toEndWith(`/warden-${id}-64.png`);
      expect(artwork?.highDpi).toEndWith(`/warden-${id}-512.png`);
    }
    expect(artworkOf(5)).toBeNull();
    expect(artworkOf(999)).toBeNull();
  });

  test("a missing selected image refuses packaging", async () => {
    await packageCopy(async (directory) => {
      await unlink(path.join(directory, "warden-4-64.png"));
      await expect(verifyWardenIcons(directory)).rejects.toThrow();
    });
  });

  test("changed or swapped approved image bytes refuse packaging", async () => {
    await packageCopy(async (directory) => {
      const file = path.join(directory, "warden-1-512.png");
      const bytes = await readFile(file);
      bytes.writeUInt8(bytes.readUInt8(bytes.length - 1) ^ 1, bytes.length - 1);
      await writeFile(file, bytes);
      await expect(verifyWardenIcons(directory)).rejects.toThrow("approved bytes");
      await cp(path.join(directory, "warden-2-512.png"), file);
      await expect(verifyWardenIcons(directory)).rejects.toThrow("approved bytes");
    });
  });

  test("a changed producer manifest cannot silently retarget an ability", async () => {
    await packageCopy(async (directory) => {
      const file = path.join(directory, "producer-manifest.json");
      const text = await readFile(file, "utf8");
      await writeFile(file, text.replace("warden.ability.1", "warden.ability.9"));
      await expect(verifyWardenIcons(directory)).rejects.toThrow("owner-approved pin");
    });
  });
});
