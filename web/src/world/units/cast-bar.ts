import { TICK_HZ, type CastState } from "../../replication";
import { wasm } from "../../wasm";
import type { ContentCatalog } from "../catalog";

export type CastBarView = {
  name: string;
  /** Filled share, 0–1. A cast fills; a channel drains backwards as it runs out. */
  fill: number;
  /** Seconds left with one decimal, e.g. `1.4`. */
  remaining: string;
  channel: boolean;
};

/** The cast bar for `cast`: the shared client model's fill (`mmorpg-client-model`, #543), named and timed. */
export function castBarView(cast: CastState, catalog: ContentCatalog): CastBarView {
  const bar = wasm.castBar(cast);
  return {
    name: catalog.abilities.get(cast.ability)?.name ?? `ability ${cast.ability}`,
    fill: bar.fill,
    remaining: (bar.remaining / TICK_HZ).toFixed(1),
    channel: bar.channel,
  };
}
