import { decodeCatalog, type ContentCatalog } from "./catalog";
import type { LocalCharacters } from "./local-characters";
import { LocalZoneSource, type LocalZoneHandle } from "./local-zone-source";
import { createSceneryProvider, type SceneryContent, type SceneryProvider } from "./scenery";

/** The initialised wasm-bindgen module surface the local world needs. */
export type LocalZoneModule = SceneryContent & {
  LocalZone: new () => LocalZoneHandle & { contentRevision(): bigint };
  /** The versioned content catalog export. */
  catalog(): string;
};

export type LocalWorld = { source: LocalZoneSource; scenery: SceneryProvider; catalog: ContentCatalog };

/**
 * One local zone per page plus the scenery and content catalog of the same
 * content revision. Mismatched content stops entry instead of drawing or
 * naming a different world.
 */
export function createLocalWorld(module: LocalZoneModule, characters: LocalCharacters): LocalWorld {
  const zone = new module.LocalZone();
  const scenery = createSceneryProvider(module);
  const catalog = decodeCatalog(module.catalog());
  const revision = zone.contentRevision();
  for (const [name, found] of [["Scenery", scenery.scenery.contentRevision], ["Catalog", catalog.contentRevision]] as const) {
    if (found !== revision) {
      throw new Error(`${name} content revision ${found} does not match zone revision ${revision}.`);
    }
  }
  return { source: new LocalZoneSource(zone, characters), scenery, catalog };
}
