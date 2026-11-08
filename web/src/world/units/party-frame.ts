import type { PartyChatInput, PartyRequestInput, ZoneSnapshot } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";

export type PartyElements = {
  frame: HTMLElement;
  list: HTMLElement;
  leave: HTMLButtonElement;
  prompt: HTMLElement;
  promptText: HTMLElement;
  accept: HTMLButtonElement;
  decline: HTMLButtonElement;
  targetInvite: HTMLButtonElement;
};

/**
 * The party frame, invite prompt and the target's Invite button (#404). The roster and invite
 * arrive through the projection; every button only names a request for the party service.
 */
export class PartyFrame {
  readonly #elements: PartyElements;
  readonly #send: (request: PartyRequestInput) => void;
  #version = 0;
  #inviteId: bigint | null = null;
  #targetPlayer: number | null = null;
  #rendered = "";
  #rows: { name: string; index: number }[] = [];

  constructor(elements: PartyElements, send: (request: PartyRequestInput) => void) {
    this.#elements = elements;
    this.#send = send;
    elements.leave.addEventListener("click", () => send({ kind: "leave" }));
    elements.accept.addEventListener("click", () => this.#answer("accept"));
    elements.decline.addEventListener("click", () => this.#answer("decline"));
    elements.targetInvite.addEventListener("click", () => {
      if (this.#targetPlayer !== null) {
        send({ kind: "invite-player", player: this.#targetPlayer });
      }
    });
    elements.list.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-member]");
      if (button) {
        send({ kind: "kick", member: Number(button.dataset.member), version: this.#version });
      }
    });
  }

  reset(): void {
    this.#rendered = "";
    this.#rows = [];
    this.#elements.frame.hidden = true;
    this.#elements.prompt.hidden = true;
    this.#elements.targetInvite.hidden = true;
  }

  #answer(kind: "accept" | "decline"): void {
    if (this.#inviteId !== null) {
      this.#send({ kind, invite: this.#inviteId });
      this.#elements.prompt.hidden = true;
    }
  }

  update(snapshot: ZoneSnapshot): void {
    const party = snapshot.party;
    this.#version = party?.version ?? 0;
    this.#rows = (party?.rows ?? []).map(({ name, index }) => ({ name, index }));
    const inParty = (party?.rows.length ?? 0) > 0;
    const target = snapshot.viewer.target;
    this.#targetPlayer = target?.kind === "player" && target.id !== snapshot.viewerId ? target.id : null;
    this.#elements.targetInvite.hidden = this.#targetPlayer === null || (inParty && !party?.leads);
    this.#renderFrame(snapshot);
  }

  #renderFrame(snapshot: ZoneSnapshot): void {
    const party = snapshot.party;
    const key = JSON.stringify(party ?? null, (_key, value) => typeof value === "bigint" ? value.toString() : value);
    if (key === this.#rendered) { return; }
    this.#rendered = key;
    const elements = this.#elements;
    const invite = party?.invite ?? null;
    this.#inviteId = invite?.invite ?? null;
    elements.prompt.hidden = invite === null;
    if (invite) {
      elements.promptText.textContent = `${invite.from} invites you to a party.`;
    }
    const rows = party?.rows ?? [];
    elements.frame.hidden = rows.length === 0;
    elements.list.replaceChildren(...rows.map((row) => {
      const item = document.createElement("li");
      item.className = row.online ? "party-member" : "party-member offline";
      const name = document.createElement("span");
      const status = row.elsewhere ? " · elsewhere" : row.online ? "" : " · offline";
      name.textContent = `${row.leader ? "★ " : ""}${row.name}${row.you ? " (you)" : ""}${status}`;
      item.append(name);
      if (party?.leads && !row.you) {
        const kick = document.createElement("button");
        kick.type = "button";
        kick.textContent = "Kick";
        kick.dataset.member = String(row.index);
        kick.setAttribute("aria-label", `Remove ${row.name} from the party`);
        item.append(kick);
      }
      return item;
    }));
  }

  /** The request a typed party command names, or why it cannot be sent. */
  request(parsed: PartyChatInput): PartyRequestInput | string {
    switch (parsed.kind) {
      case "invite":
        return { kind: "invite-name", name: parsed.name };
      case "leave":
        return { kind: "leave" };
      case "kick": {
        const wanted = parsed.name.toLowerCase();
        const row = this.#rows.find((candidate) => candidate.name.toLowerCase() === wanted);
        return row ? { kind: "kick", member: row.index, version: this.#version } : `${parsed.name} is not in your party.`;
      }
      case "error":
        return parsed.error;
    }
  }
}
