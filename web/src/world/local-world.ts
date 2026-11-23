import { TICK_HZ } from "../replication";
import { decodeCatalog, type ContentCatalog } from "./catalog";
import type { LocalCharacters } from "./local-characters";
import { LocalZoneSource, type LocalZoneHandle } from "./local-zone-source";
import { createSceneryProvider, type SceneryContent, type SceneryProvider } from "./scenery";

/** The initialised wasm-bindgen module surface the local world needs. */
export type LocalZoneModule = SceneryContent & {
  LocalZone: {
    /** A fresh world whose rolls also derive from the page's secret `worldSeed` (#1111). */
    withWorldSeed(worldSeed: bigint): LocalZoneHandle;
    fromCheckpoint(checkpoint: Uint8Array): LocalZoneHandle;
    /** The content revision a fresh zone hosts, without building one (#855). */
    hostedContentRevision(): bigint;
  };
  /** The versioned content catalog export. */
  catalog(): string;
  /** The rate the local host advances at (`mmorpg_core::TICK_HZ`). */
  tickHz(): number;
};

export type LocalWorld = { source: LocalZoneSource; scenery: SceneryProvider; catalog: ContentCatalog };

/**
 * The page's local zone source (resuming the browser's saved world on entry)
 * plus the scenery and content catalog of the same content revision.
 * Mismatched content stops entry instead of drawing or naming a different world.
 */
export function createLocalWorld(module: LocalZoneModule, characters: LocalCharacters): LocalWorld {
  if (module.tickHz() !== TICK_HZ) {
    throw new Error(`The local zone runs at ${module.tickHz()} Hz, not the browser's ${TICK_HZ} Hz.`);
  }
  const scenery = createSceneryProvider(module);
  const catalog = decodeCatalog(module.catalog());
  const revision = module.LocalZone.hostedContentRevision();
  for (const [name, found] of [["Scenery", scenery.scenery.contentRevision], ["Catalog", catalog.contentRevision]] as const) {
    if (found !== revision) {
      throw new Error(`${name} content revision ${found} does not match zone revision ${revision}.`);
    }
  }
  const createZone = (checkpoint: Uint8Array | undefined) =>
    checkpoint ? module.LocalZone.fromCheckpoint(checkpoint) : module.LocalZone.withWorldSeed(freshWorldSeed());
  return { source: new LocalZoneSource(createZone, characters), scenery, catalog };
}

/**
 * A new world's own seed from the browser's randomness (#1111); the saved world keeps it.
 * Never 0, which would mean the content's public seed alone.
 */
export function freshWorldSeed(): bigint {
  for (;;) {
    const seed = crypto.getRandomValues(new BigUint64Array(1))[0] ?? 0n;
    if (seed !== 0n) { return seed; }
  }
}
