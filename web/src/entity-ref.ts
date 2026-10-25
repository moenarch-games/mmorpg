/**
 * Unit identity shared by the command and snapshot wire formats
 * (docs/PROTOCOL.md): a kind code then a `u32` ID, kind 0 meaning none.
 */
export type EntityKind = "player" | "creature" | "npc" | "minion";
export type EntityRef = { readonly kind: EntityKind; readonly id: number };

const KIND_CODES: Record<EntityKind, number> = { player: 1, creature: 2, npc: 3, minion: 4 };

export function entityKindCode(kind: EntityKind): number {
  return KIND_CODES[kind];
}

/** The kind for a wire code; `null` for 0 (none); throws for unknown codes. */
export function entityKindFromCode(code: number): EntityKind | null {
  switch (code) {
    case 0:
      return null;
    case 1:
      return "player";
    case 2:
      return "creature";
    case 3:
      return "npc";
    // A Necromancer's summoned minion (#295).
    case 4:
      return "minion";
    default:
      throw new Error("Unknown entity kind");
  }
}

export function isU32(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff;
}

export function sameEntity(left: EntityRef | null, right: EntityRef | null): boolean {
  return left !== null && right !== null && left.kind === right.kind && left.id === right.id;
}

/** A stable map key such as `creature:108`. */
export function entityKey(entity: EntityRef): string {
  return `${entity.kind}:${entity.id}`;
}
