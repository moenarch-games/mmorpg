import type { DepthsLootView, WorldCommand, ZoneSnapshot } from "../../generated/mmorpg-wasm/mmorpg_wasm.js";
import { TICK_HZ } from "../../replication";

export type DepthsLootElements = {
  panel: HTMLElement;
  list: HTMLElement;
};

/**
 * Captain Thorn's shared loot (#404): a Need/Greed/Pass prompt for each open roll and a claim
 * button for items everyone passed on. The projection carries the state; every button only
 * names a command the zone validates.
 */
export class DepthsLootPanel {
  readonly #elements: DepthsLootElements;
  readonly #send: (command: WorldCommand) => void;
  #rendered = "";

  constructor(elements: DepthsLootElements, send: (command: WorldCommand) => void) {
    this.#elements = elements;
    this.#send = send;
    elements.list.addEventListener("click", (event) => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]");
      if (!button) { return; }
      const run = BigInt(button.dataset.run ?? "0");
      const drop = Number(button.dataset.drop ?? 0);
      const action = button.dataset.action;
      if (action === "claim") {
        send({ kind: "claim-depths-drop", run, drop });
      } else if (action === "need" || action === "greed" || action === "pass") {
        send({ kind: "roll-depths-drop", run, drop, choice: action });
      }
    });
  }

  reset(): void {
    this.#rendered = "";
    this.#elements.panel.hidden = true;
    this.#elements.list.replaceChildren();
  }

  update(snapshot: ZoneSnapshot): void {
    const view = snapshot.depthsLoot ?? null;
    if (view === null) {
      if (!this.#elements.panel.hidden) { this.reset(); }
      return;
    }
    // The countdown runs even between sheet updates, so include the whole seconds left.
    const key = JSON.stringify(view, (_key, value) => typeof value === "bigint" ? value.toString() : value)
      + secondsLeft(view, snapshot.tick);
    if (key === this.#rendered) { return; }
    this.#rendered = key;
    this.#elements.panel.hidden = false;
    this.#elements.list.replaceChildren(...view.drops.map((drop) => this.#row(view, drop, snapshot.tick)));
  }

  #row(view: DepthsLootView, drop: DepthsLootView["drops"][number], tick: bigint): HTMLElement {
    const row = document.createElement("li");
    row.className = "depths-loot-row";
    const name = document.createElement("span");
    name.textContent = drop.itemName;
    row.append(name);
    const state = drop.state;
    if (state.kind === "rolling") {
      if (state.answer !== null) {
        row.append(text(`You chose ${state.answer}. Waiting for the others…`));
      } else {
        const remaining = Number(state.deadline - tick) / TICK_HZ;
        row.append(text(`${Math.max(0, Math.ceil(remaining))} s`));
        for (const [action, label, allowed] of [["need", "Need", state.mayNeed], ["greed", "Greed", true], ["pass", "Pass", true]] as const) {
          row.append(button(view.runId, drop.drop, action, label, allowed));
        }
      }
    } else if (state.kind === "held") {
      if (state.claimable) {
        row.append(text("Everyone passed."), button(view.runId, drop.drop, "claim", "Claim", true));
      } else {
        row.append(text("Everyone passed; it stays until the run ends."));
      }
    } else if (state.won) {
      const how = state.roll === 0 ? "claimed" : `${state.need ? "Need" : "Greed"} ${state.roll}`;
      row.append(text(state.pending ? `Yours (${how}) — bag full or arriving…` : `Yours (${how})`));
    } else {
      row.append(text("Taken by a party member."));
    }
    return row;
  }
}

function text(content: string): HTMLElement {
  const span = document.createElement("span");
  span.textContent = content;
  return span;
}

function button(run: bigint, drop: number, action: string, label: string, enabled: boolean): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.textContent = label;
  element.dataset.run = String(run);
  element.dataset.drop = String(drop);
  element.dataset.action = action;
  element.disabled = !enabled;
  return element;
}

function secondsLeft(view: DepthsLootView, tick: bigint): number {
  return view.drops.reduce((seconds, drop) => {
    return drop.state.kind === "rolling" ? Math.ceil(Number(drop.state.deadline - tick) / TICK_HZ) : seconds;
  }, 0);
}
