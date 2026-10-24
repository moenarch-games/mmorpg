import { TICK_HZ } from "../replication";
import { decodeCatalog, type ContentCatalog } from "./catalog";
import type { LocalCharacters } from "./local-characters";
import { LocalZoneSource, type LocalZoneHandle } from "./local-zone-source";
import { createSceneryProvider, type SceneryContent, type SceneryProvider } from "./scenery";

/** The initialised wasm-bindgen module surface the local world needs. */
export type LocalZoneModule = SceneryContent & {
  LocalZone: { new (): LocalZoneHandle & { contentRevision(): bigint }; fromCheckpoint(checkpoint: Uint8Array): LocalZoneHandle };
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
  const zone = new module.LocalZone();
  const scenery = createSceneryProvider(module);
  const catalog = decodeCatalog(module.catalog());
  const revision = zone.contentRevision();
  for (const [name, found] of [["Scenery", scenery.scenery.contentRevision], ["Catalog", catalog.contentRevision]] as const) {
    if (found !== revision) {
      throw new Error(`${name} content revision ${found} does not match zone revision ${revision}.`);
    }
  }
  const createZone = (checkpoint: Uint8Array | undefined) =>
    checkpoint ? module.LocalZone.fromCheckpoint(checkpoint) : new module.LocalZone();
  return { source: new LocalZoneSource(createZone, characters), scenery, catalog };
}
