import type { WorldCommand } from "../../command-wire";
import type { PartyChatInput } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import type { ZoneSnapshot } from "../../replication";
import { wasm } from "../../wasm";

export type ChatElements = {
  frame: HTMLElement;
  log: HTMLElement;
  form: HTMLFormElement;
  input: HTMLInputElement;
};

/**
 * The browser chat frame: a polite live log of received lines and a text field. Focusing the field
 * hands the keyboard to it (held movement is released by the controls), Enter sends, Escape returns
 * to the world. Parsing and the log are the shared client model's (`mmorpg-client-model`, #543);
 * this frame only renders them.
 */
export class ChatFrame {
  // Created on first use: the page builds its world view before `mmorpg-wasm` has loaded.
  #chatLog: InstanceType<typeof wasm.ChatLog> | null = null;
  readonly #elements: ChatElements;
  readonly #send: (command: WorldCommand) => void;
  readonly #onLeave: () => void;
  readonly #party: (parsed: PartyChatInput) => void;
  #rendered = -1;

  constructor(elements: ChatElements, send: (command: WorldCommand) => void, onLeave: () => void, party: (parsed: PartyChatInput) => void) {
    this.#elements = elements;
    this.#send = send;
    this.#onLeave = onLeave;
    this.#party = party;
    elements.form.addEventListener("submit", (event) => {
      event.preventDefault();
      const raw = elements.input.value;
      if (raw.trim().length > 0) {
        const party = wasm.parsePartyChat(raw);
        if (party !== null) {
          this.#party(party);
          elements.input.value = "";
          this.close();
          return;
        }
        const parsed = wasm.parseChatInput(raw);
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

  get #log(): InstanceType<typeof wasm.ChatLog> {
    this.#chatLog ??= new wasm.ChatLog();
    return this.#chatLog;
  }

  /** Adds a local line to the log, such as a party refusal. */
  note(text: string): void {
    this.#log.note(text);
    this.#render();
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
    const added = this.#log.added();
    if (added === this.#rendered) {
      return;
    }
    this.#rendered = added;
    const log = this.#elements.log;
    log.replaceChildren(...this.#log.entries().map((entry) => {
      const item = document.createElement("li");
      item.className = `chat-${entry.kind}`;
      item.textContent = entry.text;
      return item;
    }));
    log.scrollTop = log.scrollHeight;
  }
}
