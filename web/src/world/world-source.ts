import type { WorldCommand } from "../command-wire";
import type { EntityState, ZoneSnapshot } from "../replication";
import type { ContentCatalog } from "./catalog";
import type { SceneryProvider } from "./scenery";

/**
 * Whether a joined source's projections are live, or it is restoring a lost
 * connection to its zone host while the last known scene stays visible.
 */
export type LinkState = "connected" | "reconnecting";

/** The class and sex a new player chooses on entry; the zone takes the choice once. */
export type JoinCharacter = { classId: "warden" | "ranger" | "arcanist"; sex: "female" | "male" };

/** The default entry character: a male Warden, like the native client. */
export const DEFAULT_JOIN_CHARACTER: JoinCharacter = { classId: "warden", sex: "male" };

/** Wire values of a class choice (0 Warden, 1 Ranger, 2 Arcanist; 0 female, 1 male). */
export function classChoiceCodes(character: JoinCharacter): { classId: number; sex: number } {
  return {
    classId: ["warden", "ranger", "arcanist"].indexOf(character.classId),
    sex: character.sex === "female" ? 0 : 1,
  };
}

/**
 * Where the presentation layer gets its world. A source admits one local
 * player, carries that player's intent to an authoritative zone, and hands
 * back only decoded player-scoped projections. `LocalZoneSource` runs the
 * shared zone simulation in WASM; `OnlineZoneSource` talks to a zone host
 * over WebTransport. Both implement this contract, so presentation cannot
 * tell them apart.
 */
export type WorldSource = {
  /**
  /**
   * Enters the world as a new player and resolves to its ID once the first
   * projection addressed to it arrived. Rejects while joined. A join that
   * rejects leaves the source unjoined, so entry can be retried. The new player
   * chooses `character`'s class and sex before any other command.
   */
  join(character?: JoinCharacter): Promise<number>;
  /**
   * Leaves the world or cancels a pending join; the zone removes the unit (a
   * network host after its reconnect grace). Does nothing when not joined.
   */
  leave(): void;
  /**
   * Sends intent under the next strictly increasing sequence and returns that sequence, or `null`
   * when the source dropped the intent (a network source during an interruption or with a full
   * queue). A projection whose acknowledged sequence reaches it has resolved the intent. Throws when
   * not joined.
   */
  sendCommand(command: WorldCommand): number | null;
  /**
   * Progresses by elapsed wall-clock seconds and returns the projections received since the last
   * call in tick order; intermediate sheets/events are retained. Local catch-up is bounded to four
   * ticks. Throws, leaving the source unjoined, when the session failed closed since the last call.
   */
  advance(deltaSeconds: number): readonly ZoneSnapshot[];
  /** The newest decoded projection addressed to the joined player, if any. */
  latestProjection(): ZoneSnapshot | null;
  /** Interpolated entities to draw now; empty before the first projection. */
  sample(): readonly EntityState[];
  /** `reconnecting` while the source restores its connection; always `connected` for a local zone. */
  linkState(): LinkState;
};

/** A world the page can enter: a source and the scenery and catalog of the content revision it serves. */
export type ZoneWorld = { source: WorldSource; scenery: SceneryProvider; catalog: ContentCatalog };
