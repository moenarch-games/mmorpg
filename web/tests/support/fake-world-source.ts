import type { WorldCommand } from "../../src/command-wire";
import type { EntityState, ZoneSnapshot } from "../../src/replication";
import { DEFAULT_JOIN_CHARACTER, type JoinCharacter, type LinkState, type WorldSource } from "../../src/world/world-source";
import { playerEntity, testSnapshot } from "./snapshots";

/** An in-memory world that honours the `WorldSource` contract without any rules. */
export class FakeWorldSource implements WorldSource {
  readonly sent: { player: number; command: WorldCommand }[] = [];
  /** While set, joins are refused and change nothing. */
  refuseJoins = false;
  #nextPlayer = 1;
  #player: number | null = null;
  #tick = 0n;
  #latest: ZoneSnapshot | null = null;

  /** Class choices of every accepted join, in order. */
  readonly joinedAs: JoinCharacter[] = [];

  async join(character: JoinCharacter = DEFAULT_JOIN_CHARACTER): Promise<number> {
    if (this.#player !== null) throw new Error("already joined");
    if (this.refuseJoins) throw new Error("join refused");
    this.#player = this.#nextPlayer;
    this.#nextPlayer += 1;
    this.joinedAs.push(character);
    this.#publish();
    return this.#player;
  }

  leave(): void {
    this.#player = null;
    this.#latest = null;
  }

  sendCommand(command: WorldCommand): number {
    if (this.#player === null) throw new Error("not joined");
    this.sent.push({ player: this.#player, command });
    return this.sent.length;
  }

  advance(deltaSeconds: number): void {
    if (this.#player !== null && deltaSeconds > 0) {
      this.#tick += 1n;
      this.#publish();
    }
  }

  latestProjection(): ZoneSnapshot | null {
    return this.#latest;
  }

  linkState(): LinkState {
    return "connected";
  }

  sample(): readonly EntityState[] {
    return this.#latest?.entities ?? [];
  }

  #publish(): void {
    const player = this.#player;
    if (player === null) return;
    this.#latest = testSnapshot({
      zoneId: 1, tick: this.#tick, contentRevision: 1n, acknowledgedSequence: 0, viewerId: player,
      entities: [playerEntity(player, [0, 90, 0])],
    });
  }
}
