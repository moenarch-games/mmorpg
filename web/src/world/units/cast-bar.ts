import { TICK_HZ, type CastState } from "../../replication";
import type { ContentCatalog } from "../catalog";

export type CastBarView = {
  name: string;
  /** Filled share, 0–1. A cast fills; a channel drains backwards as it runs out. */
  fill: number;
  /** Seconds left with one decimal, e.g. `1.4`. */
  remaining: string;
  channel: boolean;
};

export function castBarView(cast: CastState, catalog: ContentCatalog): CastBarView {
  const progress = cast.total > 0 ? Math.min(1, Math.max(0, cast.elapsed / cast.total)) : 1;
  return {
    name: catalog.abilities.get(cast.ability)?.name ?? `ability ${cast.ability}`,
    fill: cast.channel ? 1 - progress : progress,
    remaining: (Math.max(0, cast.total - cast.elapsed) / TICK_HZ).toFixed(1),
    channel: cast.channel,
  };
}
