/**
 * Browser storage for the local zone host's durable characters (#30).
 *
 * The record and lease rules live in Rust (`mmorpg-characters`); this page
 * supplies what they need: entry bytes in `localStorage`, an epoch for each
 * local zone it starts, and a Web Lock per character so that only one tab
 * plays a character at a time. The lock is held for the whole visit and the
 * browser drops it when the tab closes, which is how a crashed or closed tab
 * hands the character back; the next tab's newer epoch then supersedes the
 * lease the old one left behind.
 */

const ENTRY_PREFIX = "mmorpg.character.v1.";
const EPOCH_KEY = "mmorpg.local-zone-epoch.v1";
const LOCK_PREFIX = "mmorpg.character.";

export type EntryStorage = Pick<Storage, "getItem" | "setItem">;

/** The wasm-bindgen `CharacterEntries` shape. */
export type CharacterEntries = {
  read(key: string): Uint8Array | undefined;
  write(key: string, entry: Uint8Array): void;
};

export type ExclusiveLocks = Pick<LockManager, "request">;

/** Storage for a page whose browser denies `localStorage`: characters last for the visit only. */
export class VisitStorage implements EntryStorage {
  readonly #items = new Map<string, string>();

  getItem(key: string): string | null {
    return this.#items.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.#items.set(key, value);
  }
}

export function characterEntryKey(characterId: string): string {
  return `${ENTRY_PREFIX}${characterId}`;
}

export function storageEntries(storage: EntryStorage): CharacterEntries {
  return {
    read(key) {
      const stored = storage.getItem(characterEntryKey(key));
      return stored === null ? undefined : decodeBase64(stored);
    },
    write(key, entry) {
      storage.setItem(characterEntryKey(key), encodeBase64(entry));
    },
  };
}

export class LocalCharacters {
  readonly entries: CharacterEntries;
  readonly #storage: EntryStorage;
  readonly #locks: ExclusiveLocks | undefined;
  /** Characters this page released whose lock the browser has not dropped yet. */
  readonly #releasing = new Map<string, Promise<void>>();

  constructor(storage: EntryStorage, locks: ExclusiveLocks | undefined) {
    this.#storage = storage;
    this.entries = storageEntries(storage);
    this.#locks = locks;
  }

  /** A local zone epoch above every earlier one in this browser. */
  nextZoneEpoch(): bigint {
    const stored = this.#storage.getItem(EPOCH_KEY);
    if (stored !== null && !/^[0-9]{1,19}$/.test(stored)) {
      throw new Error("The local zone epoch is corrupt.");
    }
    const next = (stored === null ? 0n : BigInt(stored)) + 1n;
    this.#storage.setItem(EPOCH_KEY, next.toString());
    return next;
  }

  /**
   * Holds the character for this tab until the returned release is called.
   * Rejects while another tab plays it. A character this page just released
   * is held again once the browser has let go of it.
   */
  async hold(characterId: string): Promise<() => void> {
    const locks = this.#locks;
    if (!locks) {
      throw new Error("This browser cannot keep a character to one tab (Web Locks are unavailable).");
    }
    await this.#releasing.get(characterId);
    let settled: Promise<unknown> = Promise.resolve();
    const release = await new Promise<(() => void) | null>((resolve, reject) => {
      settled = locks.request(`${LOCK_PREFIX}${characterId}`, { ifAvailable: true }, (lock) => {
        if (!lock) {
          resolve(null);
          return undefined;
        }
        return new Promise<void>((done) => resolve(() => done()));
      });
      settled.catch(reject);
    });
    if (!release) {
      throw new Error("This character is already in play in another tab.");
    }
    return () => {
      const released = settled.then(() => undefined, () => undefined).finally(() => {
        if (this.#releasing.get(characterId) === released) {
          this.#releasing.delete(characterId);
        }
      });
      this.#releasing.set(characterId, released);
      release();
    };
  }
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
