import type { RendererSceneNode } from "@moritzbrantner/three-d-renderer";
import { entityKey, sameEntity, type EntityRef } from "../../entity-ref";
import type { StrikeAim, StrikeHeight, ZoneSnapshot } from "../../replication";
import { guardDisplay, type GuardDisplay } from "./shield-guard";
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
  visual: SpellVisual | "impact" | "sweep" | "guard" | "block";
  critical?: boolean;
  ability: number;
  source: EntityRef;
  target: EntityRef | null;
  /** A ground point fixed when the effect started (Blizzard's target point). */
  ground: Vec3 | null;
  startedAt: number;
  /** Seconds a timed effect lives; null while a bound effect follows a cast or an aura. */
  duration: number | null;
  /** A directed strike's aim and whether the zone said it struck a creature. */
  sweep?: { aim: StrikeAim; hit: boolean };
  /** The viewer's raised shield as the zone holds it (#156); bound, refreshed every projection. */
  guard?: NonNullable<GuardDisplay>;
  /** A blow met a guard in `lane`: blocked, or overpowering it. Source is the defender. */
  block?: { lane: StrikeHeight; broken: boolean };
};

export const EFFECT_SECONDS = { projectile: 0.4, arrow: 0.22, ring: 0.65, swing: 0.28, sweep: 0.34, block: 0.45 } as const;
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
type ActorAction = { pose: CasterPose; startedAt: number; duration: number; hold: boolean; ability: number; release: boolean; projectedProgress?: number };

export class SpellEffects {
  #effects: SpellEffect[] = [];
  #nextId = 1;
  readonly #actions = new Map<string, ActorAction>();
  readonly #casts = new Map<string, ActorAction>();
  #lastTick: bigint | null = null;
  #session = "";
  readonly #dead = new Set<string>();

  clear(): void {
    this.#effects = [];
    this.#actions.clear();
    this.#casts.clear();
    this.#dead.clear();
    this.#lastTick = null;
    this.#session = "";
  }

  /** Consume each authoritative projection once; identities never share animation state. */
  spawn(snapshot: ZoneSnapshot, now: number, resolve: ResolveAnchor): void {
    const session = `${snapshot.zoneId}:${snapshot.viewerId}:${snapshot.contentRevision}`;
    if (this.#session !== session || (this.#lastTick !== null && snapshot.tick < this.#lastTick)) {
      this.clear();
      this.#session = session;
    }
    if (snapshot.tick === this.#lastTick) { return; }
    this.#lastTick = snapshot.tick;
    const visible = new Set(snapshot.entities.map((entity) => entityKey({ kind: entity.kind, id: entity.entityId })));
    this.#dead.clear();
    for (const entity of snapshot.entities) {
      if (entity.flags.dead) { this.#dead.add(entityKey({ kind: entity.kind, id: entity.entityId })); }
    }
    for (const actions of [this.#actions, this.#casts]) {
      for (const key of actions.keys()) {
        if (!visible.has(key) || this.#dead.has(key)) { actions.delete(key); }
      }
    }
    this.#effects = this.#effects.filter((effect) => visible.has(entityKey(effect.source)) && (!effect.target || visible.has(entityKey(effect.target))));
    const currentCast = snapshot.viewer.cast;
    const currentPose = currentCast ? presentationOf(currentCast.ability)?.pose : null;
    if (currentCast && currentPose && !snapshot.viewer.dead) {
      this.#casts.set(entityKey({ kind: "player", id: snapshot.viewerId }), {
        pose: currentPose, ability: currentCast.ability, startedAt: now, duration: currentCast.total / 30,
        hold: true, release: false, projectedProgress: currentCast.total > 0 ? currentCast.elapsed / currentCast.total : 1,
      });
    }
    for (const event of snapshot.events) {
      if (event.kind === "attack-performed") {
        this.#actions.set(entityKey(event.source), { pose: "swing", startedAt: now, duration: ACTION_SECONDS, hold: false, ability: 0, release: false });
      } else if (event.kind === "cast-started") {
        const pose = presentationOf(event.ability)?.pose;
        if (pose) {
          this.#casts.set(entityKey(event.source), { pose, startedAt: now, duration: event.ticks / 30, hold: true, ability: event.ability, release: false });
        }
        if (event.ability === BLIZZARD_ABILITY) { this.#startShards(event.source, event.target, event.ability, now, resolve); }
      } else if (event.kind === "ability-used") {
        const presentation = presentationOf(event.ability);
        const key = entityKey(event.source);
        const release = this.#casts.get(key)?.ability === event.ability;
        if (release) { this.#casts.delete(key); }
        if (presentation?.pose) {
          this.#actions.set(key, { pose: presentation.pose, startedAt: now, duration: ACTION_SECONDS, hold: false, ability: event.ability, release });
        }
        const visual = presentation?.visual;
        if (visual === "projectile" || visual === "arrow" || visual === "ring" || visual === "swing") {
          this.#add({ visual, ability: event.ability, source: event.source, target: event.target, ground: null, duration: EFFECT_SECONDS[visual] }, now);
        }
      } else if (event.kind === "strike-swept") {
        // The zone's outcome, not the input, drives the sweep: its aim, and hit or whiff.
        this.#actions.set(entityKey(event.source), { pose: "swing", startedAt: now, duration: EFFECT_SECONDS.sweep, hold: false, ability: event.ability, release: false });
        this.#add({ visual: "sweep", ability: event.ability, source: event.source, target: event.target, ground: null, duration: EFFECT_SECONDS.sweep, sweep: { aim: event.aim, hit: event.target !== null } }, now);
      } else if (event.kind === "blocked" || event.kind === "guard-broken") {
        // The defender's shield meets the blow at the zone's lane, facing the attacker.
        const block = { lane: event.lane, broken: event.kind === "guard-broken" };
        this.#add({ visual: "block", ability: 0, source: event.target, target: event.source, ground: null, duration: EFFECT_SECONDS.block, block }, now);
      } else if (event.kind === "interrupted") {
        const key = entityKey(event.target);
        if (this.#casts.get(key)?.ability === event.ability) { this.#casts.delete(key); }
        if (this.#actions.get(key)?.ability === event.ability) { this.#actions.delete(key); }
      } else if (event.kind === "damage-dealt" || event.kind === "damage-taken" || event.kind === "damage-observed") {
        if (event.amount > 0) {
          this.#add({ visual: "impact", ability: 0, source: event.source, target: event.target, ground: null, duration: 0.32, critical: event.critical }, now);
        }
      } else if (event.kind === "died") {
        this.#dead.add(entityKey(event.entity));
      }
    }
    // Death wins even when the same projection also contains an accepted attempt.
    for (const key of this.#dead) { this.#actions.delete(key); this.#casts.delete(key); }
  }

  /** Only the viewer's continuously projected cast/auras may reconstruct bound effects. */
  sync(snapshot: ZoneSnapshot, now: number, resolve: ResolveAnchor): void {
    const viewer: EntityRef = { kind: "player", id: snapshot.viewerId };
    const key = entityKey(viewer);
    const cast = snapshot.viewer.cast;
    const pose = cast ? presentationOf(cast.ability)?.pose : null;
    if (!snapshot.viewer.dead && cast && pose) {
      this.#casts.set(key, { pose, ability: cast.ability, startedAt: now - cast.elapsed / 30, duration: cast.total / 30, hold: true, release: false, projectedProgress: cast.total > 0 ? cast.elapsed / cast.total : 1 });
    } else { this.#casts.delete(key); }
    const barrier = !snapshot.viewer.dead && snapshot.auras.some((aura) => aura.ability === BARRIER_ABILITY && aura.kind === "absorb");
    this.#effects = this.#effects.filter((effect) => {
      if (effect.duration !== null || effect.visual === "guard") { return true; }
      if (effect.visual === "shards") { return this.#casts.get(entityKey(effect.source))?.ability === BLIZZARD_ABILITY; }
      return sameEntity(effect.source, viewer) && barrier;
    });
    if (cast?.channel && cast.ability === BLIZZARD_ABILITY && !snapshot.viewer.dead) {
      this.#startShards(viewer, snapshot.viewer.target, BLIZZARD_ABILITY, now, resolve);
    }
    // The raised shield follows the zone's guard: its lane, and protecting, suspended or broken.
    const shown = snapshot.viewer.dead ? null : guardDisplay(snapshot.viewer.guard);
    const raised = this.#effects.find((effect) => effect.visual === "guard");
    if (shown === null) {
      this.#effects = this.#effects.filter((effect) => effect.visual !== "guard");
    } else if (raised) {
      raised.guard = shown;
    } else {
      this.#add({ visual: "guard", ability: 0, source: viewer, target: null, ground: null, duration: null, guard: shown }, now);
    }
    if (barrier && !this.#effects.some((effect) => effect.visual === "bubble" && sameEntity(effect.source, viewer))) {
      this.#add({ visual: "bubble", ability: BARRIER_ABILITY, source: viewer, target: null, ground: null, duration: null }, now);
    }
  }

  active(now: number): readonly SpellEffect[] {
    for (const actions of [this.#actions, this.#casts]) {
      for (const [key, action] of actions) {
        if (action.projectedProgress === undefined && now - action.startedAt >= action.duration) { actions.delete(key); }
      }
    }
    this.#effects = this.#effects.filter((effect) => effect.duration === null
      ? this.#casts.get(entityKey(effect.source))?.ability === effect.ability || effect.visual === "bubble" || effect.visual === "guard"
      : now - effect.startedAt < effect.duration);
    return this.#effects;
  }

  actionFor(entity: EntityRef, now: number): ViewerAction | null {
    const key = entityKey(entity);
    if (entity.kind === "npc" || this.#dead.has(key)) { return null; }
    for (const actions of [this.#actions, this.#casts]) {
      const action = actions.get(key);
      if (!action) { continue; }
      const elapsed = now - action.startedAt;
      if (action.projectedProgress !== undefined) { return { pose: action.pose, hold: true, progress: Math.min(1, action.projectedProgress) }; }
      if (elapsed >= action.duration) { actions.delete(key); continue; }
      const progress = Math.min(1, Math.max(0, elapsed / Math.max(0.001, action.duration)));
      return { pose: action.pose, hold: action.hold, progress: action.release ? 0.5 + progress / 2 : progress };
    }
    return null;
  }

  viewerAction(snapshot: ZoneSnapshot, now: number): ViewerAction | null {
    if (snapshot.viewer.dead) { return null; }
    const cast = snapshot.viewer.cast;
    const pose = cast ? presentationOf(cast.ability)?.pose : null;
    if (cast && pose) { return { pose, progress: cast.total > 0 ? Math.min(1, cast.elapsed / cast.total) : 1, hold: true }; }
    return this.actionFor({ kind: "player", id: snapshot.viewerId }, now);
  }

  #startShards(source: EntityRef, target: EntityRef | null, ability: number, now: number, resolve: ResolveAnchor): void {
    const point = target ? resolve(target) : null;
    if (!point || this.#effects.some((effect) => effect.visual === "shards" && sameEntity(effect.source, source))) { return; }
    this.#add({ visual: "shards", ability, source, target, ground: [point.x, point.feetY, point.z], duration: null }, now);
  }

  #add(effect: Omit<SpellEffect, "id" | "startedAt">, now: number): void {
    this.#effects.push({ ...effect, id: this.#nextId++, startedAt: now });
    if (this.#effects.length > MAX_EFFECTS) { this.#effects.splice(0, this.#effects.length - MAX_EFFECTS); }
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
/** Blade height above the feet (metres) per aimed band; the zone's bands are 0–0.6, 0.6–1.3, 1.3–2 m. */
export const SWEEP_HEIGHT_METRES = { low: 0.35, middle: 0.95, high: 1.65 } as const;
/** The aimed side turns the arc 45° from the facing (left is toward +yaw); it spans ±30°. */
export const SWEEP_SIDE_RADIANS = { left: Math.PI / 4, centre: 0, right: -Math.PI / 4 } as const;
const SWEEP_HALF_WIDTH = Math.PI / 6;
/** The zone's guard coverage is centred 30° toward the shield (left) side of the facing. */
const GUARD_SIDE_RADIANS = Math.PI / 6;
const SWEEP_COUNT = 8;
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
    case "impact": {
      if (!target) { return []; }
      const glow = effect.critical ? "#ffe6a3" : "#ffb376";
      const nodes: RendererSceneNode[] = [{ id: `${prefix}-flash`, geometry: { kind: "sphere", radius: 0.08 + 0.2 * t }, color: glow, unlit: true, opacity: 0.85 * (1 - t), transform: { translation: [target.x, target.centreY, target.z] } }];
      for (let index = 0; index < 6; index += 1) {
        const angle = index / 6 * Math.PI * 2;
        const radius = 0.12 + 0.85 * t;
        nodes.push({ id: `${prefix}-spark-${index}`, geometry: { kind: "box", size: [0.06, 0.06, 0.18] }, color: glow, unlit: true, opacity: 0.9 * (1 - t), transform: { translation: [target.x + Math.cos(angle) * radius, target.centreY + Math.sin(angle) * radius, target.z + Math.sin(angle * 2) * radius * 0.3], rotationQuaternion: yawQuaternion(angle) } });
      }
      return nodes;
    }
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
    case "sweep": {
      const sweep = effect.sweep;
      if (!source || !sweep) {
        return [];
      }
      const centre = source.yaw + SWEEP_SIDE_RADIANS[sweep.aim.side];
      const height = source.feetY + SWEEP_HEIGHT_METRES[sweep.aim.height];
      const tint = sweep.hit ? "#ffd36b" : "#9fb4c8";
      // The blade crosses the covered arc from the left edge to the right edge; the trail lags.
      return Array.from({ length: SWEEP_COUNT }, (_, index) => {
        const lag = index / SWEEP_COUNT;
        const reach = Math.max(0, Math.min(1, t * 1.25 - lag * 0.35));
        const angle = centre + SWEEP_HALF_WIDTH - 2 * SWEEP_HALF_WIDTH * reach;
        return {
          id: `${prefix}-sweep-${index}`,
          geometry: { kind: "box" as const, size: [0.9, 0.05, 0.12] as [number, number, number] },
          color: index === 0 ? "#ffffff" : tint,
          unlit: true,
          opacity: 0.85 * (1 - t) * (1 - lag * 0.6),
          transform: {
            translation: [source.x + Math.sin(angle) * 1.6, height, source.z + Math.cos(angle) * 1.6] as Vec3,
            rotationQuaternion: yawQuaternion(angle + Math.PI / 2),
          },
        };
      });
    }
    case "guard": {
      const guard = effect.guard;
      if (!source || !guard) {
        return [];
      }
      // The shield covers the front and the shield (left) side: it sits 30° toward it.
      const angle = source.yaw + GUARD_SIDE_RADIANS;
      const tint = guard.state === "protecting" ? "#9cc8ff" : guard.state === "broken" ? "#ff6a4d" : "#9fb4c8";
      return [{
        id: `${prefix}-shield`,
        geometry: { kind: "box" as const, size: [0.62, 0.5, 0.07] as [number, number, number] },
        color: tint,
        unlit: true,
        opacity: guard.state === "protecting" ? 0.55 : 0.25,
        transform: {
          translation: [source.x + Math.sin(angle) * 0.55, source.feetY + SWEEP_HEIGHT_METRES[guard.lane], source.z + Math.cos(angle) * 0.55] as Vec3,
          rotationQuaternion: yawQuaternion(angle),
        },
      }];
    }
    case "block": {
      const block = effect.block;
      if (!source || !target || !block) {
        return [];
      }
      const angle = Math.atan2(target.x - source.x, target.z - source.z);
      // A block flashes and holds; a broken guard flares red and drops away.
      const drop = block.broken ? 0.5 * t * t : 0;
      return [{
        id: `${prefix}-plate`,
        geometry: { kind: "box" as const, size: [0.7 + 0.25 * t, 0.55 + 0.2 * t, 0.06] as [number, number, number] },
        color: block.broken ? "#ff5a3c" : "#e8f3ff",
        unlit: true,
        opacity: 0.9 * (1 - t),
        transform: {
          translation: [source.x + Math.sin(angle) * 0.7, source.feetY + SWEEP_HEIGHT_METRES[block.lane] - drop, source.z + Math.cos(angle) * 0.7] as Vec3,
          rotationQuaternion: yawQuaternion(angle),
        },
      }];
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
