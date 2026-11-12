import { encodeCommand, type WorldCommand } from "../../command-wire";
import { FixedTickClock } from "../../demo-clock";
import type { SessionConnector } from "../../session/connection";
import type { SessionRoute } from "../../session/route";
import { OnlineZoneSource } from "../online-zone-source";
import { DEFAULT_JOIN_CHARACTER, type JoinCharacter, type WorldSource } from "../world-source";
import { OwnBodyPrediction, type PredictionHandle } from "./netcode-trial";
import {
  canSendCharacterCommand, decodeMatchProjection, encodeBuild,
  encodeCharacterCommand, type ArenaEquipment, type MatchProjection,
} from "./protocol";

/**
 * The fortress content revisions (#552) a host may project: a match without hit rewind, and one
 * running the arena netcode trial's capped hit rewind (#680), the host's per-match option.
 */
export type ArenaContentRevisions = { baseline: bigint; hitRewind: bigint };

/** A mode envelope over the existing session source, without another transport or animation loop. */
export class BattleRoyaleSource implements WorldSource {
  readonly #session: OnlineZoneSource;
  readonly #revisions: ArenaContentRevisions;
  readonly #createPrediction: (() => PredictionHandle) | null;
  /** Local ticks for the predicted own body (#680); projections arrive on the session's own clock. */
  readonly #clock = new FixedTickClock();
  #prediction: OwnBodyPrediction | null = null;
  #match: MatchProjection | null = null;

  constructor(options: {
    route: SessionRoute;
    connector: SessionConnector;
    contentRevisions: ArenaContentRevisions;
    equipment(character: JoinCharacter): ArenaEquipment;
    /** Predicts the own body (#680, chosen per client); without it, today's interpolated baseline. */
    prediction?: () => PredictionHandle;
  }) {
    const expectedMatch = BigInt(`0x${options.route.matchId.slice(3)}`);
    const revisions = options.contentRevisions;
    this.#revisions = revisions;
    this.#createPrediction = options.prediction ?? null;
    this.#session = new OnlineZoneSource({
      route: options.route,
      contentRevision: revisions.baseline,
      connector: options.connector,
      activityProtocol: {
        encodeCommand: encodeCharacterCommand,
        initialCommand: (character) => encodeBuild(character, options.equipment(character)),
        decodeProjection: (payload, sheets) => {
          const match = decodeMatchProjection(payload, [revisions.baseline, revisions.hitRewind], sheets);
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

  /** Whether the joined match rewinds hits (#680), told by its content revision; null before a projection. */
  get hitRewind(): boolean | null {
    const match = this.#match;
    return match ? match.zone.contentRevision === this.#revisions.hitRewind : null;
  }

  /** Whether this client predicts its own body (#680). */
  get predicted(): boolean { return this.#createPrediction !== null; }

  join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    this.#match = null;
    this.#prediction?.free();
    this.#prediction = this.#createPrediction ? new OwnBodyPrediction(this.#createPrediction()) : null;
    this.#clock.reset();
    return this.#session.join(character);
  }

  leave(): void {
    this.#session.leave();
    this.#prediction?.free();
    this.#prediction = null;
  }

  sendCommand(command: WorldCommand): number | null {
    const match = this.#match;
    const viewerId = this.#session.latestProjection()?.viewerId;
    if (!match || !match.players.some((player) => player.id === viewerId && player.alive) ||
      !canSendCharacterCommand(command, match.phase)) {
      return null;
    }
    const sequence = this.#session.sendCommand(command);
    if (sequence !== null) { this.#prediction?.sent(sequence, encodeCommand(command)); }
    return sequence;
  }

  advance(deltaSeconds: number) {
    const received = this.#session.advance();
    const prediction = this.#prediction;
    if (prediction) {
      // Projections first: reconciling replays the movement sent since up to the local tick.
      prediction.receive(received);
      const due = this.#clock.advance(deltaSeconds);
      for (let tick = 0; tick < due; tick += 1) {
        prediction.step();
        prediction.record();
      }
    }
    return received;
  }

  latestProjection() { return this.#session.latestProjection(); }

  sample() {
    const entities = this.#session.sample();
    const viewerId = this.#session.latestProjection()?.viewerId;
    const prediction = this.#prediction;
    // The own body is drawn where the prediction puts it while alive in the match.
    if (!prediction || viewerId === undefined || this.#session.latestProjection()?.viewer.dead) { return entities; }
    return prediction.draw(entities, viewerId, this.#clock.fraction);
  }

  linkState() { return this.#session.linkState(); }
}
