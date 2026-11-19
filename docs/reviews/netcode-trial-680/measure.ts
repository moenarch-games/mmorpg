// Automated supplement to the #680 netcode trial: drives `NetcodeTrialSource` (the Practice
// route's own source) over the real `mmorpg-wasm` arena zone with one fixed scripted input
// sequence per option × simulated round trip, and prints per-cell metrics as JSON.
//
//   cd web && bun ../docs/reviews/netcode-trial-680/measure.ts > measure.json
//
// It never replaces the owner's playtest (Atlas 5.9.i680, #680 decision A).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { buildWasm } from "../../../web/scripts/build-wasm";

const directory = buildWasm();
const generated = (await import(join(directory, "mmorpg_wasm.js"))) as { initSync(options: { module: BufferSource }): unknown };
generated.initSync({ module: readFileSync(join(directory, "mmorpg_wasm_bg.wasm")) });

const { wasm } = await import("../../../web/src/wasm");
const { decodeSnapshot, TICK_HZ } = await import("../../../web/src/replication");
const { NetcodeTrialSource } = await import("../../../web/src/world/battle-royale/netcode-trial");
type Trial = import("../../../web/src/world/battle-royale/netcode-trial").NetcodeOption;
type Handle = import("../../../web/src/world/battle-royale/netcode-trial").PredictionHandle;
type Zone = import("../../../web/src/world/battle-royale/netcode-trial").TrialZoneHandle;
type Snapshot = import("../../../web/src/replication").ZoneSnapshot;
type Entity = import("../../../web/src/replication").EntityState;
type Command = import("../../../web/src/command-wire").WorldCommand;

const TICK = 1 / TICK_HZ;
const WARDEN_STANDARD = new Uint16Array(6);
const TOTAL_TICKS = 3000; // 50 s
const CYCLE = 600;
const YAW_STEPS = 65_536;
// Timed swing (Heroic Strike): PLAYER_REACH_UNITS and STRIKE_ARC_HALF_WIDTH (30°).
const TIMED = { reach: 250, half: Math.PI / 6 };
// Basic strike (#885), approximately the sword chain's profile: 2.55 m, 50° either side.
const BASIC = { reach: 255, half: (50 / 180) * Math.PI };

type Vec = [number, number, number];

function yawToward(dx: number, dz: number): number {
  const yaw = Math.round((Math.atan2(dx, dz) / (2 * Math.PI)) * YAW_STEPS);
  return ((yaw % YAW_STEPS) + YAW_STEPS) % YAW_STEPS;
}

/** Whether `target` stands inside a blow from `from` with heading `facing`. */
function covers(shape: { reach: number; half: number }, from: Vec, facing: number, target: Vec): boolean {
  const dx = target[0] - from[0];
  const dz = target[2] - from[2];
  if (Math.hypot(dx, dz) > shape.reach) { return false; }
  let delta = Math.atan2(dx, dz) - (facing / YAW_STEPS) * 2 * Math.PI;
  delta = Math.atan2(Math.sin(delta), Math.cos(delta));
  return Math.abs(delta) <= shape.half;
}

const quantile = (values: number[], q: number) => {
  if (values.length === 0) { return 0; }
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;
};
const mean = (values: number[]) => values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length;
const round = (value: number, places = 1) => Math.round(value * 10 ** places) / 10 ** places;
const isPlayer = (ref: { kind: string; id: number } | null | undefined, id: number) => ref?.kind === "player" && ref.id === id;

async function runCell(option: Trial, roundTripMs: number) {
  // The zone's own projection for the trial player every tick (the source requests it anyway;
  // decoding has no side effects): authoritative positions and every event, undelayed.
  const authority: Snapshot[] = [];
  const corrections: number[] = [];
  const source = new NetcodeTrialSource(
    () => {
      const zone = wasm.LocalZone.forArenaTrial(0, 1, WARDEN_STANDARD, option === "rewind") as unknown as Zone;
      const projection = zone.projection.bind(zone);
      const player = zone.scenarioPlayer;
      return new Proxy(zone, {
        get(target, key) {
          if (key === "projection") {
            return (id: number) => {
              const bytes = projection(id);
              if (id === player) { authority.push(decodeSnapshot(bytes)); }
              return bytes;
            };
          }
          const value = Reflect.get(target, key, target);
          return typeof value === "function" ? value.bind(target) : value;
        },
      });
    },
    () => {
      const inner = new wasm.ArenaMovementPrediction() as unknown as Handle;
      return {
        sent: (sequence, command) => inner.sent(sequence, command),
        step: () => inner.step(),
        acknowledge: (tick, acknowledged) => inner.acknowledge(tick, acknowledged),
        // How far each reconciliation moves the predicted body: the correction a player sees.
        reconcile: (tick, x, y, z, vx, vy, vz, facing) => {
          const before = inner.position();
          inner.reconcile(tick, x, y, z, vx, vy, vz, facing);
          const after = inner.position();
          if (before.length === 4 && after.length === 4) {
            corrections.push(Math.hypot(after[0]! - before[0]!, after[1]! - before[1]!, after[2]! - before[2]!));
          }
        },
        position: () => inner.position(),
        free: () => inner.free(),
      };
    },
    { option, roundTripMs },
  );
  const viewer = await source.join();
  const partnerId = viewer + 1;
  const zoneAt = (tick: bigint) => authority.find((snapshot) => snapshot.tick === tick);
  const body = (entities: readonly Entity[], id: number) => entities.find((e) => e.kind === "player" && e.entityId === id);

  const drawn = () => {
    const entities = source.sample();
    return { own: body(entities, viewer), partner: body(entities, partnerId) };
  };

  const drawnTrack: Vec[] = [];
  const directionChanges: number[] = [];
  const leads: number[] = [];
  const sent: { tick: number; kind: "basic" | "timed"; purpose: string }[] = [];
  // Screen checks: for each zone tick the attacker's blow resolves on, whether the attacker's
  // screen showed the partner inside it when that tick's projection arrived.
  const screenAt = new Map<bigint, Vec[]>();
  let lastMove = "";
  let lastBasic = -1_000;
  let lastTimed = -1_000;

  for (let tick = 1; tick <= TOTAL_TICKS; tick += 1) {
    const { own, partner } = drawn();
    const cycle = tick % CYCLE;
    const commands: Command[] = [];
    if (own && partner) {
      const dx = partner.position[0] - own.position[0];
      const dz = partner.position[2] - own.position[2];
      const distance = Math.hypot(dx, dz);
      const facing = yawToward(dx, dz);
      if (tick % 6 === 0) {
        const flip = Math.floor(tick / 45) % 2 === 0 ? 1 : -1;
        let forward: -1 | 0 | 1 = 0;
        let strafe: -1 | 0 | 1 = 0;
        if (cycle < 150) { strafe = flip; } // running strafes, a new direction every 0.75 s
        else if (cycle < 270) { forward = distance > 180 ? 1 : 0; strafe = flip; } // close while strafing
        else if (cycle < 390) { /* stand and trade */ }
        else if (cycle < 450) { forward = -1; } // back off
        else if (distance > 245) { forward = 1; } // close in to the edge of reach
        else { strafe = flip; }
        const key = `${forward},${strafe}`;
        if (key !== lastMove) { directionChanges.push(tick); lastMove = key; }
        commands.push({ kind: "move", forward, strafe, facing });
      }
      // Basic strikes (Q) at the moving partner build Rage; Heroic Strikes (hold G) are the timed
      // swings that clash and, in option 3, rewind.
      if (cycle < 290 && cycle % 45 === 20 && tick - lastBasic >= 40) {
        commands.push({ kind: "swing", facing });
        sent.push({ tick, kind: "basic", purpose: "moving" });
        lastBasic = tick;
      }
      // The partner swings on ticks divisible by 150; the trade presses on the same tick.
      const edge = cycle >= 450 && cycle < 590 && distance >= 215 && distance <= 255;
      if ((cycle === 300 || edge) && tick - lastTimed >= 130) {
        commands.push({ kind: "directed-strike", ability: 1, facing, aim: { side: "centre", height: "middle" } });
        sent.push({ tick, kind: "timed", purpose: cycle === 300 ? "trade" : "edge" });
        lastTimed = tick;
      }
    }
    for (const command of commands) { source.sendCommand(command); }
    const received = source.advance(TICK);
    const after = drawn();
    if (after.own) { drawnTrack[tick] = after.own.position as Vec; }
    for (const snapshot of received) {
      if (after.own && after.partner) {
        screenAt.set(snapshot.tick, [after.own.position as Vec, [after.own.facing, 0, 0], after.partner.position as Vec]);
      }
    }
    const present = authority.at(-1);
    const zoneOwn = present && body(present.entities, viewer);
    if (zoneOwn && after.own && cycle < 270) {
      leads.push(Math.hypot(after.own.position[0] - zoneOwn.position[0], after.own.position[2] - zoneOwn.position[2]));
    }
  }

  // The zone tick before the first local tick (baseline never asks for the welcome projection).
  const joinTick = authority.at(-1)!.tick - BigInt(TOTAL_TICKS);

  // Movement responsiveness: after every change of held direction, local ticks until the drawn
  // own body moves the new way at half the speed the zone eventually gives it.
  const latencies: number[] = [];
  const zonePos = (local: number) => {
    const snapshot = zoneAt(joinTick + BigInt(local));
    return snapshot && (body(snapshot.entities, viewer)?.position as Vec | undefined);
  };
  for (const tick of directionChanges) {
    const a = zonePos(tick + 12);
    const b = zonePos(tick + 18);
    if (!a || !b) { continue; }
    const dir = [(b[0] - a[0]) / 6, (b[2] - a[2]) / 6];
    const speed = Math.hypot(dir[0]!, dir[1]!);
    if (speed < 1) { continue; }
    for (let t = tick + 1; t < tick + 40; t += 1) {
      const p = drawnTrack[t - 1];
      const q = drawnTrack[t];
      if (!p || !q) { continue; }
      if (((q[0] - p[0]) * dir[0]! + (q[2] - p[2]) * dir[1]!) / speed >= speed / 2) { latencies.push(t - tick); break; }
    }
  }

  // Authoritative outcomes, from every zone tick's events.
  type Blow = { begin: bigint; resolves: bigint[]; kind: "basic" | "timed"; facing: number; hit: boolean; outsidePresent: boolean };
  const blows: Blow[] = [];
  const clashes: bigint[] = [];
  const partnerSwings: bigint[] = [];
  const partnerHits: bigint[] = [];
  const refusals = new Map<string, number>();
  for (const snapshot of authority) {
    for (const event of snapshot.events) {
      const ev = event as { kind: string; source?: { kind: string; id: number }; target?: { kind: string; id: number } | null } & Record<string, unknown>;
      if (ev.kind === "error") { refusals.set(String(ev.code), (refusals.get(String(ev.code)) ?? 0) + 1); }
      if (ev.kind === "strike-began" && isPlayer(ev.source, viewer)) {
        const contact = snapshot.tick + BigInt(ev.contact as number);
        blows.push({ begin: snapshot.tick, resolves: [contact], kind: "basic", facing: -1, hit: false, outsidePresent: false });
      }
      if (ev.kind === "swing-began" && isPlayer(ev.source, viewer)) {
        const first = snapshot.tick + BigInt(ev.startup as number) + 1n;
        const resolves = Array.from({ length: ev.active as number }, (_, i) => first + BigInt(i));
        blows.push({ begin: snapshot.tick, resolves, kind: "timed", facing: ev.facing as number, hit: false, outsidePresent: false });
      }
      if (ev.kind === "swing-began" && isPlayer(ev.source, partnerId)) { partnerSwings.push(snapshot.tick); }
      if (ev.kind === "strike-swept" && isPlayer(ev.source, partnerId) && isPlayer(ev.target, viewer)) { partnerHits.push(snapshot.tick); }
      if (ev.kind === "clashed") { clashes.push(snapshot.tick); }
      const ownHit = (ev.kind === "strike-swept" || ev.kind === "attack-performed") && isPlayer(ev.source, viewer) && isPlayer(ev.target, partnerId);
      if (ownHit) {
        const blow = [...blows].reverse().find((b) => b.kind === (ev.kind === "strike-swept" ? "timed" : "basic")
          && snapshot.tick >= b.resolves[0]! - 2n && snapshot.tick <= b.resolves.at(-1)! + 2n);
        if (blow && !blow.hit) {
          blow.hit = true;
          // Present geometry on the resolving tick: outside the blow means rewind admitted it.
          const at = zoneAt(snapshot.tick);
          const o = at && body(at.entities, viewer);
          const p = at && body(at.entities, partnerId);
          const facing = blow.kind === "timed" ? blow.facing : o?.facing ?? 0;
          if (o && p && !covers(blow.kind === "timed" ? TIMED : BASIC, o.position as Vec, facing, p.position as Vec)) {
            blow.outsidePresent = true;
          }
        }
      }
    }
  }
  const looked = (blow: Blow) => blow.resolves.some((tick) => {
    const screen = screenAt.get(tick);
    if (!screen) { return false; }
    const [own, [ownFacing], partner] = screen;
    return covers(blow.kind === "timed" ? TIMED : BASIC, own, blow.kind === "timed" ? blow.facing : ownFacing!, partner);
  });
  const tally = (kind: "basic" | "timed") => {
    const list = blows.filter((b) => b.kind === kind);
    const looks = list.map(looked);
    return {
      pressed: sent.filter((s) => s.kind === kind).length,
      began: list.length,
      lookedLikeHit: looks.filter(Boolean).length,
      confirmed: list.filter((b) => b.hit).length,
      lookedButMissed: list.filter((b, i) => looks[i] && !b.hit).length,
      hitButLookedMissed: list.filter((b, i) => !looks[i] && b.hit).length,
      confirmedOutsidePresent: list.filter((b) => b.outsidePresent).length,
    };
  };

  // Each scripted trade: what happened around the press.
  const trades = sent.filter((s) => s.purpose === "trade").map((s) => {
    const pressed = joinTick + BigInt(s.tick);
    const window = (t: bigint) => t >= pressed && t <= pressed + 60n;
    const own = blows.find((b) => b.kind === "timed" && window(b.begin));
    if (!own) { return "refused"; }
    if (clashes.some(window)) { return "clash"; }
    const theirs = partnerHits.some(window);
    return own.hit && theirs ? "trade" : own.hit ? "own-hit" : theirs ? "partner-hit" : partnerSwings.some(window) ? "both-miss" : "no-partner-swing";
  });
  source.leave();

  const tickMs = 1000 / TICK_HZ;
  return {
    option,
    roundTripMs,
    movement: {
      directionChanges: latencies.length,
      inputToMotionMs: { median: round(quantile(latencies, 0.5) * tickMs, 0), p90: round(quantile(latencies, 0.9) * tickMs, 0) },
      ownOffsetFromZoneCm: { mean: round(mean(leads)), p95: round(quantile(leads, 0.95)) },
      corrections: {
        reconciles: corrections.length,
        over1cm: corrections.filter((c) => c > 1).length,
        over10cm: corrections.filter((c) => c > 10).length,
        p95Cm: round(quantile(corrections, 0.95)),
        maxCm: round(Math.max(0, ...corrections)),
      },
    },
    hits: { basic: tally("basic"), timed: tally("timed") },
    clashes: {
      trades,
      clashEvents: clashes.length,
      partnerSwings: partnerSwings.length,
      partnerHitsOnViewer: partnerHits.length,
    },
    refusals: Object.fromEntries([...refusals].sort()),
    viewerDead: authority.at(-1)!.viewer.dead,
  };
}

const results = [];
for (const option of (process.env.ONLY ? [process.env.ONLY] : ["baseline", "predicted", "rewind"]) as Trial[]) {
  for (const delay of [50, 100, 150]) {
    const first = await runCell(option, delay);
    const second = await runCell(option, delay);
    results.push({ ...first, repeatable: JSON.stringify(first) === JSON.stringify(second) });
  }
}
console.log(JSON.stringify(results, null, 1));
