import type { AuraKind, AuraState } from "../../replication";
import type { ContentCatalog } from "../catalog";
import { formatRemaining } from "./ability-state";
import { auraPolarity } from "./ability-presentation";

/** Buffs help their bearer; the frame shows them above and debuffs below. */
export type SortedAuras = { buffs: readonly AuraState[]; debuffs: readonly AuraState[] };

/** Longest remaining first, then ability ID, so equal times keep a stable order between frames. */
export function sortAuras(auras: readonly AuraState[]): SortedAuras {
  const ordered = [...auras].sort((left, right) => right.remaining - left.remaining || left.ability - right.ability);
  return {
    buffs: ordered.filter((aura) => auraPolarity(aura.kind) === "buff"),
    debuffs: ordered.filter((aura) => auraPolarity(aura.kind) === "debuff"),
  };
}

const KIND_TEXT: Record<AuraKind, (amount: number) => string> = {
  "damage-over-time": (amount) => `${amount} damage over time`,
  "heal-over-time": (amount) => `${amount} healing over time`,
  absorb: (amount) => `Absorbs ${amount} damage`,
  root: () => "Rooted",
  snare: (amount) => `Slowed by ${amount}%`,
  stun: () => "Stunned",
  haste: (amount) => `Attacks ${amount}% faster`,
};

/** What an aura does, from its kind and amount. */
export function auraEffectText(aura: AuraState): string {
  return KIND_TEXT[aura.kind](aura.amount);
}

/** The icon's tooltip: the ability name, its effect and the time left. */
export function auraLabel(aura: AuraState, catalog: ContentCatalog): string {
  const name = catalog.abilities.get(aura.ability)?.name ?? `ability ${aura.ability}`;
  return `${name}: ${auraEffectText(aura)} (${formatRemaining(aura.remaining)})`;
}

/** The corner timer an aura icon shows. */
export function auraTimer(aura: AuraState): string {
  return formatRemaining(aura.remaining);
}
