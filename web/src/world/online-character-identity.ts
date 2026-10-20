/** Client identity metadata only; progression remains in the host's leased record. */
const KEY = "mmorpg.online-character-ids.v1";
type IdentityStorage = Pick<Storage, "getItem" | "setItem">;

function identities(storage: IdentityStorage): Map<string, string> {
  const raw = storage.getItem(KEY);
  if (raw === null) { return new Map(); }
  if (raw.length > 131_072) { throw new Error("Online character identities are too large."); }
  const value: unknown = JSON.parse(raw);
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Online character identities are invalid.");
  }
  const entries = Object.entries(value);
  if (entries.length > 1024 || entries.some(([slot, id]) =>
    !/^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/.test(slot) || typeof id !== "string" || !/^online-[0-9a-f]{32}$/.test(id))) {
    throw new Error("Online character identities are invalid.");
  }
  return new Map(entries.map(([slot, id]) => [slot, String(id)]));
}

/** Call under the browser's identity Web Lock to serialize first entry across tabs. */
export function onlineCharacterId(storage: IdentityStorage, slot: string, create = () => crypto.randomUUID().replaceAll("-", "")): string {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/.test(slot)) { throw new Error("Invalid local character slot."); }
  const ids = identities(storage);
  const existing = ids.get(slot);
  if (existing) { return existing; }
  if (ids.size >= 1024) { throw new Error("Online character identity limit reached."); }
  const id = `online-${create()}`;
  if (!/^online-[0-9a-f]{32}$/.test(id)) { throw new Error("Invalid generated character identity."); }
  ids.set(slot, id);
  storage.setItem(KEY, JSON.stringify(Object.fromEntries(ids)));
  return id;
}

/** A deleted roster row must not reissue a slot that still owns an online identity. */
export function hasOnlineCharacterId(storage: IdentityStorage, slot: string): boolean {
  return identities(storage).has(slot);
}
