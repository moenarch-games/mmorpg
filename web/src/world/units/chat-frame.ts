import type { WorldCommand } from "../../command-wire";
import { chatTextError, EMOTES, type EmoteName, type ZoneSnapshot } from "../../replication";

/** The chat log keeps the newest lines only. */
export const MAX_CHAT_LOG_LINES = 50;

export type ChatEntry = { key: number; text: string; kind: "say" | "yell" | "emote" | "system" };

/**
 * Reads a typed line: `/y` or `/yell` yells, `/s`, `/say` or no prefix says, and `/wave`, `/bow`,
 * `/cheer`, `/laugh` or `/point` emotes. Returns the command, or the reason the line cannot be sent;
 * the zone still decides who sees it.
 */
export function parseChatInput(raw: string): { command: WorldCommand } | { error: string } {
  const emote = /^\/([a-z]+)\s*$/iu.exec(raw)?.[1]?.toLowerCase();
  if (emote !== undefined && (EMOTES as readonly string[]).includes(emote)) {
    return { command: { kind: "emote", emote: emote as EmoteName } };
  }
  const match = /^\/(y|yell|s|say)(?:\s+|$)/iu.exec(raw);
  const channel = match && /^y/iu.test(match[1] ?? "") ? "yell" : "say";
  const text = match ? raw.slice(match[0].length) : raw;
  if (raw.startsWith("/") && !match) {
    return { error: `Use /s to say, /y to yell, or /${EMOTES.join(", /")}.` };
  }
  const problem = chatTextError(text);
  return problem === null ? { command: { kind: "chat", channel, text } } : { error: problem };
}

/** Received lines only: what the zone delivered, with readable speaker names. */
export class ChatLog {
  #identity: string | null = null;
  #tick = -1n;
  #next = 0;
  #entries: ChatEntry[] = [];

  get entries(): readonly ChatEntry[] { return this.#entries; }

  reset(): void {
    this.#identity = null;
    this.#tick = -1n;
    this.#entries = [];
  }

  /** Appends a local note, such as why a typed line was not sent. */
  note(text: string): void {
    this.#push({ text, kind: "system" });
  }

  /** Takes each projection once; returns whether lines were added. */
  receive(snapshot: ZoneSnapshot): boolean {
    const identity = `${snapshot.zoneId}:${snapshot.contentRevision}:${snapshot.viewerId}`;
    if ((this.#identity !== null && this.#identity !== identity) || snapshot.tick <= this.#tick) {
      return false;
    }
    this.#identity = identity;
    this.#tick = snapshot.tick;
    let added = false;
    for (const line of snapshot.chat) {
      const you = line.speaker === snapshot.viewerId;
      const speaker = you ? "You" : `Player ${line.speaker}`;
      if (line.channel === "emote") {
        this.#push({ text: `${speaker} ${you ? line.emote : `${line.emote}s`}.`, kind: "emote" });
      } else {
        const verb = line.channel === "yell" ? (you ? "yell" : "yells") : (you ? "say" : "says");
        this.#push({ text: `${speaker} ${verb}: ${line.text}`, kind: line.channel });
      }
      added = true;
    }
    for (const event of snapshot.events) {
      if (event.kind === "error" && event.code === "chat-throttled") {
        this.#push({ text: "You can speak or emote once per second.", kind: "system" });
        added = true;
      }
    }
    return added;
  }

  #push(entry: Omit<ChatEntry, "key">): void {
    this.#entries = [...this.#entries, { ...entry, key: this.#next++ }].slice(-MAX_CHAT_LOG_LINES);
  }
}

export type ChatElements = {
  frame: HTMLElement;
  log: HTMLElement;
  form: HTMLFormElement;
  input: HTMLInputElement;
};

/**
 * The browser chat frame: a polite live log of received lines and a text field. Focusing the field
 * hands the keyboard to it (held movement is released by the controls), Enter sends, Escape returns
 * to the world.
 */
export class ChatFrame {
  readonly #log = new ChatLog();
  readonly #elements: ChatElements;
  readonly #send: (command: WorldCommand) => void;
  readonly #onLeave: () => void;
  #rendered = -1;

  constructor(elements: ChatElements, send: (command: WorldCommand) => void, onLeave: () => void) {
    this.#elements = elements;
    this.#send = send;
    this.#onLeave = onLeave;
    elements.form.addEventListener("submit", (event) => {
      event.preventDefault();
      const raw = elements.input.value;
      if (raw.trim().length > 0) {
        const parsed = parseChatInput(raw);
        if ("command" in parsed) {
          this.#send(parsed.command);
        } else {
          this.#log.note(parsed.error);
          this.#render();
        }
      }
      elements.input.value = "";
      this.close();
    });
    elements.input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        elements.input.value = "";
        this.close();
      }
    });
  }

  /** Moves keyboard focus to the chat field. */
  open(): void {
    this.#elements.input.focus();
  }

  close(): void {
    this.#elements.input.blur();
    this.#onLeave();
  }

  reset(): void {
    this.#log.reset();
    this.#elements.input.value = "";
    this.#render();
  }

  update(snapshot: ZoneSnapshot): void {
    if (this.#log.receive(snapshot)) {
      this.#render();
    }
  }

  #render(): void {
    const entries = this.#log.entries;
    const last = entries.at(-1)?.key ?? -1;
    if (last === this.#rendered) {
      return;
    }
    this.#rendered = last;
    const log = this.#elements.log;
    log.replaceChildren(...entries.map((entry) => {
      const item = document.createElement("li");
      item.className = `chat-${entry.kind}`;
      item.textContent = entry.text;
      return item;
    }));
    log.scrollTop = log.scrollHeight;
  }
}
