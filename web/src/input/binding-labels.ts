import { applyProfile, isKeyStroke, resolveWithContextStack, type Binding, type InputStroke, type Profile } from "@moritzbrantner/input-bindings";
import { contextStack, GAME_ACTION_REGISTRY, type GameAction } from "./game-controls";

/**
 * Short labels of the bindings that actually trigger `action` in the world: the character's
 * profile applied over the MMORPG defaults, kept only when the stroke resolves to this action in
 * the gameplay context (an unbound, removed or conflicting binding yields no label). Presentation
 * only: `input-bindings` owns the bindings themselves.
 */
export function actionBindingLabels(action: GameAction, profile: Profile): string[] {
  const defaults: Binding[] = GAME_ACTION_REGISTRY.actions.flatMap((entry) => entry.defaults ?? []);
  const { bindings } = applyProfile(defaults, profile);
  const stack = contextStack({ phase: "world", panelOpen: false });
  const labels: string[] = [];
  for (const binding of bindings) {
    const [stroke] = binding.sequence;
    if (binding.action !== action || binding.sequence.length !== 1 || !stroke) {
      continue;
    }
    const resolution = resolveWithContextStack(bindings, binding.sequence, new Set(), stack);
    if (resolution.kind !== "resolved" || resolution.action !== action) {
      continue;
    }
    const label = strokeLabel(stroke);
    if (label && !labels.includes(label)) {
      labels.push(label);
    }
  }
  return labels;
}

/** A compact key name: `KeyG` → `G`, `Digit1` → `1`, `ShiftLeft` → `Shift`; gamepad buttons as `Pad N`. */
export function strokeLabel(stroke: InputStroke): string | null {
  if (isKeyStroke(stroke)) {
    const value = stroke.key.value;
    const name = value.replace(/^Key(?=[A-Z]$)/, "").replace(/^Digit(?=\d$)/, "").replace(/(Left|Right)$/, "") || value;
    const modifiers = stroke.modifiers ?? {};
    const prefix = [modifiers.ctrl && "Ctrl", modifiers.alt && "Alt", modifiers.shift && "Shift", modifiers.meta && "Meta"].filter(Boolean);
    return [...prefix, name].join("+");
  }
  if ("device" in stroke && stroke.device === "gamepadButton") {
    return `Pad ${stroke.button}`;
  }
  if ("device" in stroke && stroke.device === "mouseButton") {
    return `Mouse ${stroke.button + 1}`;
  }
  return null;
}
