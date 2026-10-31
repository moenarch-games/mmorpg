import type { WorldCommand } from "../../command-wire";
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
  readonly #log = new wasm.ChatLog();
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
