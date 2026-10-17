import { LocalCharacters, type EntryStorage, type ExclusiveLocks } from "../../src/world/local-characters";

/** `localStorage` in memory. */
export class MemoryStorage implements EntryStorage {
  readonly items = new Map<string, string>();

  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.items.set(key, value);
  }
}

/** Web Locks for tabs sharing one browser: `ifAvailable` requests see held names. */
export class MemoryLocks implements ExclusiveLocks {
  readonly held = new Set<string>();

  request(name: string, options: LockOptions, callback: LockGrantedCallback<unknown>): Promise<unknown>;
  request(...args: unknown[]): Promise<unknown> {
    const [name, , callback] = args as [string, LockOptions, LockGrantedCallback<unknown>];
    if (this.held.has(name)) {
      return Promise.resolve(callback(null));
    }
    this.held.add(name);
    return Promise.resolve(callback({ name, mode: "exclusive" } as Lock)).finally(() => this.held.delete(name));
  }
}

/** One browser's character storage; pass the same storage and locks for several tabs. */
export function testCharacters(storage = new MemoryStorage(), locks = new MemoryLocks()): LocalCharacters {
  return new LocalCharacters(storage, locks);
}
