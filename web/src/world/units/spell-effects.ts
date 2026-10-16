import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { sameEntity, type EntityRef } from "../../entity-ref";
import type { ZoneSnapshot } from "../../replication";
import { ABILITY_PRESENTATION, presentationOf, type CasterPose, type SpellVisual } from "./ability-presentation";

/**
 * Spell visuals built from the renderer's existing box and sphere nodes. An effect starts from a
 * projection event (or from the viewer's own cast or aura state), lives for a fixed time or while
 * its cast or aura lasts, and expires on its own. Effects never feed back into the simulation.
 */
export type Anchor = { x: number; feetY: number; centreY: number; z: number; yaw: number };
export type ResolveAnchor = (entity: EntityRef) => Anchor | null;

type Vec3 = [number, number, number];
type Quaternion = [number, number, number, number];

export type SpellEffect = {
  id: number;
  visual: SpellVisual;
  ability: number;
  source: EntityRef;
  target: EntityRef | null;
  /** A ground point fixed when the effect started (Blizzard's target point). */
  ground: Vec3 | null;
  startedAt: number;
  /** Seconds a timed effect lives; null while a bound effect follows a cast or an aura. */
  duration: number | null;
};

export const EFFECT_SECONDS = { projectile: 0.4, arrow: 0.22, ring: 0.65, swing: 0.28 } as const;
const MAX_EFFECTS = 24;
/** Frost Nova's radius (800 units) in metres; Blizzard's area (600 units). */
const NOVA_RADIUS = 8;
const BLIZZARD_RADIUS = 6;
export const BLIZZARD_ABILITY = 12;
export const BARRIER_ABILITY = 11;
const ACTION_SECONDS = 0.3;

const accentOf = (ability: number): `#${string}` => ABILITY_PRESENTATION.get(ability)?.accent ?? "#ffffff";

/** `hold`: a cast in progress keeps its pose raised; a finished instant ability plays through and releases. */
export type ViewerAction = { pose: CasterPose; progress: number; hold: boolean };

/** The live effects of one session. */
export class SpellEffects {
  #effects: SpellEffect[] = [];
  #nextId = 1;
  #action: { pose: CasterPose; startedAt: number; release: boolean } | null = null;
  /** The ability the viewer was casting in the last received projection. */
  #casting: number | null = null;

  clear(): void {
    this.#effects = [];
    this.#action = null;
    this.#casting = null;
  }

  /** Starts the effects that one received projection's events call for. */
  spawn(snapshot: ZoneSnapshot, now: number, resolve: ResolveAnchor): void {
    const viewer: EntityRef = { kind: "player", id: snapshot.viewerId };
    for (const event of snapshot.events) {
      if (event.kind === "ability-used") {
        const presentation = presentationOf(event.ability);
        if (sameEntity(event.source, viewer) && presentation?.pose) {
          // A finished cast releases its held pose; an instant ability swells and releases.
          this.#action = { pose: presentation.pose, startedAt: now, release: this.#casting === event.ability };
        }
        const visual = presentation?.visual;
        if (visual === "projectile" || visual === "arrow" || visual === "ring" || visual === "swing") {
          this.#add({ visual, ability: event.ability, source: event.source, target: event.target, ground: null, duration: EFFECT_SECONDS[visual] }, now);
        }
      } else if (event.kind === "cast-started" && event.ability === BLIZZARD_ABILITY) {
        this.#startShards(event.source, event.target, event.ability, now, resolve);
      }
    }
    this.#casting = snapshot.viewer.cast?.ability ?? null;
  }

  /** Starts and ends the effects bound to the viewer's channel and auras. */
  sync(snapshot: ZoneSnapshot, now: number, resolve: ResolveAnchor): void {
    const viewer: EntityRef = { kind: "player", id: snapshot.viewerId };
    const channel = snapshot.viewer.cast;
    const channelling = channel !== null && channel.channel && channel.ability === BLIZZARD_ABILITY;
    const barrier = snapshot.auras.some((aura) => aura.ability === BARRIER_ABILITY && aura.kind === "absorb");
    this.#effects = this.#effects.filter((effect) => {
      if (effect.duration !== null) {
        return true;
      }
      return effect.visual === "shards" ? channelling : barrier;
    });
    if (channelling && !this.#effects.some((effect) => effect.visual === "shards")) {
      this.#startShards(viewer, snapshot.viewer.target, BLIZZARD_ABILITY, now, resolve);
    }
    if (barrier && !this.#effects.some((effect) => effect.visual === "bubble")) {
      this.#add({ visual: "bubble", ability: BARRIER_ABILITY, source: viewer, target: null, ground: null, duration: null }, now);
    }
  }

  /** Drops expired timed effects and returns the rest. */
  active(now: number): readonly SpellEffect[] {
    this.#effects = this.#effects.filter((effect) => effect.duration === null || now - effect.startedAt < effect.duration);
    return this.#effects;
  }

  /** The viewer's current cast, draw or swing pose with its progress 0–1, or null at rest. */
  viewerAction(snapshot: ZoneSnapshot, now: number): ViewerAction | null {
    const cast = snapshot.viewer.cast;
    const pose = cast ? presentationOf(cast.ability)?.pose : null;
    if (cast && pose) {
      return { pose, progress: cast.total > 0 ? Math.min(1, cast.elapsed / cast.total) : 1, hold: true };
    }
    const action = this.#action;
    if (action && now - action.startedAt < ACTION_SECONDS) {
      const elapsed = (now - action.startedAt) / ACTION_SECONDS;
      // The release starts at the peak of the swell, where a held cast pose ends.
      return { pose: action.pose, progress: action.release ? 0.5 + elapsed / 2 : elapsed, hold: false };
    }
    return null;
  }

  #startShards(source: EntityRef, target: EntityRef | null, ability: number, now: number, resolve: ResolveAnchor): void {
    const point = target ? resolve(target) : null;
    if (!point || this.#effects.some((effect) => effect.visual === "shards")) {
      return;
    }
    this.#add({ visual: "shards", ability, source, target, ground: [point.x, point.feetY, point.z], duration: null }, now);
  }

  #add(effect: Omit<SpellEffect, "id" | "startedAt">, now: number): void {
    this.#effects.push({ ...effect, id: this.#nextId, startedAt: now });
    this.#nextId += 1;
    if (this.#effects.length > MAX_EFFECTS) {
      this.#effects.splice(0, this.#effects.length - MAX_EFFECTS);
    }
  }
}

function lerp(from: number, to: number, t: number): number {
  return from + (to - from) * t;
}

function qMul(a: Quaternion, b: Quaternion): Quaternion {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

const yawQuaternion = (yaw: number): Quaternion => [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
const pitchQuaternion = (pitch: number): Quaternion => [Math.sin(pitch / 2), 0, 0, Math.cos(pitch / 2)];

/** Rotation turning +Z along `direction`. */
function facing(direction: Vec3): Quaternion {
  const [dx, dy, dz] = direction;
  const horizontal = Math.hypot(dx, dz);
  return qMul(yawQuaternion(Math.atan2(dx, dz)), pitchQuaternion(-Math.atan2(dy, horizontal)));
}

/** Pseudo-random but deterministic fraction for shard `index`. */
function scatter(index: number, salt: number): number {
  const value = Math.sin(index * 127.1 + salt * 311.7) * 43_758.5453;
  return value - Math.floor(value);
}

const SHARD_COUNT = 12;
const RING_COUNT = 20;
const SWING_COUNT = 6;

/** Renderer nodes for one effect at `now` seconds; stable IDs, only transforms change. */
export function effectNodes(effect: SpellEffect, now: number, resolve: ResolveAnchor): RendererSceneNode[] {
  const color = accentOf(effect.ability);
  const prefix = `fx-${effect.id}`;
  const age = now - effect.startedAt;
  const t = effect.duration === null ? 0 : Math.min(1, Math.max(0, age / effect.duration));
  const source = resolve(effect.source);
  const target = effect.target ? resolve(effect.target) : null;
  switch (effect.visual) {
    case "projectile": {
      if (!source || !target) {
        return [];
      }
      const x = lerp(source.x, target.x, t);
      const z = lerp(source.z, target.z, t);
      const y = lerp(source.centreY, target.centreY, t) + Math.sin(Math.PI * t) * 0.5;
      const trail = Math.max(0, t - 0.12);
      return [
        { id: `${prefix}-core`, geometry: { kind: "sphere", radius: 0.2 }, color, transform: { translation: [x, y, z] } },
        {
          id: `${prefix}-trail`,
          geometry: { kind: "sphere", radius: 0.12 },
          color: "#fff2c4",
          opacity: 0.7,
          transform: {
            translation: [lerp(source.x, target.x, trail), lerp(source.centreY, target.centreY, trail) + Math.sin(Math.PI * trail) * 0.5, lerp(source.z, target.z, trail)],
          },
        },
      ];
    }
    case "arrow": {
      if (!source || !target) {
        return [];
      }
      const from: Vec3 = [source.x, source.centreY + 0.2, source.z];
      const to: Vec3 = [target.x, target.centreY, target.z];
      const direction: Vec3 = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
      return [{
        id: `${prefix}-shaft`,
        geometry: { kind: "box", size: [0.05, 0.05, 0.9] },
        color,
        transform: {
          translation: [lerp(from[0], to[0], t), lerp(from[1], to[1], t), lerp(from[2], to[2], t)],
          rotationQuaternion: facing(direction),
        },
      }];
    }
    case "ring": {
      if (!source) {
        return [];
      }
      const radius = Math.max(0.3, NOVA_RADIUS * (1 - (1 - t) * (1 - t)));
      return Array.from({ length: RING_COUNT }, (_, index) => {
        const angle = (index / RING_COUNT) * Math.PI * 2;
        return {
          id: `${prefix}-ring-${index}`,
          geometry: { kind: "box" as const, size: [0.5, 0.14, 0.18] as [number, number, number] },
          color,
          opacity: 0.8,
          transform: {
            translation: [source.x + Math.sin(angle) * radius, source.feetY + 0.2, source.z + Math.cos(angle) * radius] as Vec3,
            rotationQuaternion: yawQuaternion(angle + Math.PI / 2),
          },
        };
      });
    }
    case "bubble": {
      if (!source) {
        return [];
      }
      // A slow swell so the shield reads as alive.
      const swell = 1 + 0.03 * Math.sin(age * 3);
      return [{
        id: `${prefix}-bubble`,
        geometry: { kind: "sphere", radius: 1.25 * swell },
        color,
        opacity: 0.28,
        transform: { translation: [source.x, source.centreY, source.z] },
      }];
    }
    case "shards": {
      const ground = effect.ground;
      if (!ground) {
        return [];
      }
      return Array.from({ length: SHARD_COUNT }, (_, index) => {
        const angle = scatter(index, 1) * Math.PI * 2;
        const radius = Math.sqrt(scatter(index, 2)) * BLIZZARD_RADIUS;
        // Each shard falls 6 m at its own speed and starts over.
        const fall = (age * (0.9 + scatter(index, 3) * 0.7) + scatter(index, 4)) % 1;
        return {
          id: `${prefix}-shard-${index}`,
          geometry: { kind: "box" as const, size: [0.09, 0.6, 0.09] as [number, number, number] },
          color,
          opacity: 0.85,
          transform: {
            translation: [ground[0] + Math.sin(angle) * radius, ground[1] + 6.5 - fall * 6.4, ground[2] + Math.cos(angle) * radius] as Vec3,
          },
        };
      });
    }
    case "swing": {
      if (!source) {
        return [];
      }
      const sweep = Math.PI * 0.9;
      return Array.from({ length: SWING_COUNT }, (_, index) => {
        // A blade trail: the leading segment is at the sweep angle, the others lag behind it.
        const angle = source.yaw + sweep / 2 - sweep * t * ((SWING_COUNT - index) / SWING_COUNT);
        return {
          id: `${prefix}-arc-${index}`,
          geometry: { kind: "box" as const, size: [0.7, 0.06, 0.16] as [number, number, number] },
          color: index === 0 ? "#ffffff" : color,
          opacity: 0.75,
          transform: {
            translation: [source.x + Math.sin(angle) * 1.5, source.centreY + 0.25, source.z + Math.cos(angle) * 1.5] as Vec3,
            rotationQuaternion: yawQuaternion(angle + Math.PI / 2),
          },
        };
      });
    }
  }
}

/** Nodes for every active effect. */
export function allEffectNodes(effects: readonly SpellEffect[], now: number, resolve: ResolveAnchor): RendererSceneNode[] {
  return effects.flatMap((effect) => effectNodes(effect, now, resolve));
}
