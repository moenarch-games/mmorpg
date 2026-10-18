import { createElement, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Profile } from "@moritzbrantner/input-bindings";
import { InputBindingsWorkbench, type InputBindingsContextScenario } from "@moritzbrantner/input-bindings-react/workbench";
import "@moritzbrantner/input-bindings-react/workbench.css";
import { contextStack, GAME_ACTION_REGISTRY } from "../input/game-controls";
import type { CharacterBindings } from "./settings-panel";

/**
 * Embeds the shared `input-bindings` workbench (#41 Q17): it owns capture, conflicts and repair.
 * A confirmed profile is saved first and only then shown as current, so a failed save keeps the
 * previous profile in both the editor and the running controls (Q19).
 */
const SCENARIOS: readonly InputBindingsContextScenario[] = [
  { id: "gameplay", label: "In the world", stack: contextStack({ phase: "world", panelOpen: false }), defaultKeyboardMode: "physical" },
  { id: "panels", label: "Panel open", stack: contextStack({ phase: "world", panelOpen: true }), defaultKeyboardMode: "physical" },
  { id: "casting", label: "Casting", stack: contextStack({ phase: "world", panelOpen: false, casting: true }), defaultKeyboardMode: "physical" },
  { id: "selection", label: "Character selection", stack: contextStack({ phase: "selection", creating: false }), defaultKeyboardMode: "physical" },
];

function BindingsEditor({ bindings }: { bindings: CharacterBindings }) {
  const [profile, setProfile] = useState(bindings.profile);
  const [problem, setProblem] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const onProfileChange = (next: Profile) => {
    const failure = bindings.save(next);
    setProblem(failure);
    if (failure === null) {
      setProfile(next);
    }
  };
  return createElement(
    "div",
    { className: `settings-bindings-editor${advanced ? " is-advanced" : ""}` },
    problem ? createElement("p", { className: "settings-error", role: "alert" }, problem) : null,
    createElement(
      "div",
      { className: "settings-bindings-toolbar" },
      createElement("span", null, `Bindings for ${bindings.characterName}`),
      createElement("button", {
        type: "button",
        "aria-pressed": advanced,
        onClick: () => setAdvanced((value) => !value),
      }, advanced ? "Basic view" : "Advanced"),
    ),
    createElement(InputBindingsWorkbench, {
      key: advanced ? "advanced" : "basic",
      registry: GAME_ACTION_REGISTRY,
      profile,
      onProfileChange,
      contextScenarios: SCENARIOS,
      title: "Controls",
      description: `Bindings for ${bindings.characterName}.`,
      initialPresentation: "list",
    }),
  );
}

export function mountBindingsEditor(host: HTMLElement, bindings: CharacterBindings): () => void {
  const root = createRoot(host);
  root.render(createElement(BindingsEditor, { bindings }));
  return () => root.unmount();
}
