import type { WorldCommand } from "../../command-wire";
import type { SessionConnector } from "../../session/connection";
import type { SessionRoute } from "../../session/route";
import { OnlineZoneSource } from "../online-zone-source";
import { DEFAULT_JOIN_CHARACTER, type JoinCharacter, type WorldSource } from "../world-source";
import {
  canSendCharacterCommand, decodeMatchProjection, encodeBuild,
  encodeCharacterCommand, type ArenaEquipment, type MatchProjection,
} from "./protocol";

/** A mode envelope over the existing session source, without another transport or animation loop. */
export class BattleRoyaleSource implements WorldSource {
  readonly #session: OnlineZoneSource;
  #match: MatchProjection | null = null;

  constructor(options: {
    route: SessionRoute;
    connector: SessionConnector;
    /** The shared fortress content revision (#552) the host must project. */
    contentRevision: bigint;
    equipment(character: JoinCharacter): ArenaEquipment;
  }) {
    const expectedMatch = BigInt(`0x${options.route.matchId.slice(3)}`);
    const contentRevision = options.contentRevision;
    this.#session = new OnlineZoneSource({
      route: options.route,
      contentRevision,
      connector: options.connector,
      activityProtocol: {
        encodeCommand: encodeCharacterCommand,
        initialCommand: (character) => encodeBuild(character, options.equipment(character)),
        decodeProjection: (payload, sheets) => {
          const match = decodeMatchProjection(payload, contentRevision, sheets);
          if (match.matchId !== expectedMatch) {
            throw new Error("The host sent another Battle Royale match.");
          }
          const zone = match.zone;
          // Delivery acknowledges mode setup as well as character commands. Session time continues
          // after gameplay freezes; keep the original projection's wire bytes for the Rust presenter.
          zone.tick = match.tick;
          zone.acknowledgedSequence = match.acknowledgedSequence;
          if (!this.#match || match.tick > this.#match.tick) { this.#match = match; }
          return { snapshot: zone, finished: match.phase === "finished" };
        },
      },
    });
  }

  get match(): MatchProjection | null { return this.#match; }

  join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    this.#match = null;
    return this.#session.join(character);
  }

  leave(): void { this.#session.leave(); }

  sendCommand(command: WorldCommand): number | null {
    const match = this.#match;
    const viewerId = this.#session.latestProjection()?.viewerId;
    if (!match || !match.players.some((player) => player.id === viewerId && player.alive) ||
      !canSendCharacterCommand(command, match.phase)) {
      return null;
    }
    return this.#session.sendCommand(command);
  }

  advance(_deltaSeconds: number) { return this.#session.advance(); }
  latestProjection() { return this.#session.latestProjection(); }
  sample() { return this.#session.sample(); }
  linkState() { return this.#session.linkState(); }
}
