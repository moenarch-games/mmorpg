import * as THREE from "three";
import type { RaidSignal } from "../replication";
import type { RaidPrepSpots } from "./raid-prep";

/**
 * What a night raid's forewarning leaves in the world (#532, Atlas 5.1.60, 5.1.61): on a
 * forewarned evening a short trail of boot prints on the Rim Trail at the scouts' tracks, and once
 * a brace is accepted a timber leaning against that barn wall until the raid it served resolves.
 * Purely cosmetic: it follows the projection's raid signal and never feeds back.
 */

/** Boot prints along the trail: count, spacing and size in metres. */
const PRINTS = 6;
const STRIDE = 0.7;
const PRINT = [0.14, 0.3] as const;
/** A brace timber: length and thickness in metres, leaning this far off vertical. */
const TIMBER = [2.6, 0.18] as const;
const LEAN = 0.45;

export class RaidSigns {
  readonly root = new THREE.Group();
  readonly #tracks = new THREE.Group();
  readonly #braces: THREE.Mesh[] = [];
  #placed = false;

  constructor() {
    this.root.name = "raid-signs";
    this.#tracks.name = "raid-tracks";
    this.#tracks.visible = false;
    this.root.add(this.#tracks);
  }

  /**
   * Shows the tracks and the accepted braces for `signal`. `groundAt` gives the ground height in
   * metres at `(x, z)` metres; `unitsPerMetre` converts the spots' units.
   */
  update(signal: RaidSignal | null, spots: RaidPrepSpots | null, groundAt: (x: number, z: number) => number, unitsPerMetre: number): void {
    if (!spots) {
      this.root.visible = false;
      return;
    }
    if (!this.#placed) {
      this.#place(spots, groundAt, unitsPerMetre);
      this.#placed = true;
    }
    this.root.visible = true;
    this.#tracks.visible = signal?.signs === true;
    this.#braces.forEach((mesh, index) => {
      mesh.visible = ((signal?.readiness ?? 0) & (1 << (index + 1))) !== 0;
    });
  }

  #place(spots: RaidPrepSpots, groundAt: (x: number, z: number) => number, unitsPerMetre: number): void {
    const print = new THREE.MeshBasicMaterial({ color: 0x2b2118, transparent: true, opacity: 0.75, depthWrite: false });
    const shape = new THREE.CircleGeometry(0.5, 12);
    const [tx, tz] = [spots.tracks.x / unitsPerMetre, spots.tracks.z / unitsPerMetre];
    for (let index = 0; index < PRINTS; index += 1) {
      // A trail heading south toward the farm, alternating feet.
      const side = index % 2 === 0 ? -0.15 : 0.15;
      const [x, z] = [tx + side, tz + (index - PRINTS / 2) * STRIDE];
      const mesh = new THREE.Mesh(shape, print);
      mesh.scale.set(PRINT[0] * 2, PRINT[1] * 2, 1);
      mesh.rotation.x = -Math.PI / 2;
      mesh.position.set(x, groundAt(x, z) + 0.03, z);
      mesh.name = `raid-track-${index}`;
      this.#tracks.add(mesh);
    }
    const wood = new THREE.MeshLambertMaterial({ color: 0x7a5a36 });
    const beam = new THREE.BoxGeometry(TIMBER[1], TIMBER[0], TIMBER[1]);
    spots.braces.forEach((spot, index) => {
      const [x, z] = [spot.x / unitsPerMetre, spot.z / unitsPerMetre];
      const mesh = new THREE.Mesh(beam, wood);
      // Leaning from the brace spot toward the barn's middle (east for a west wall, and back).
      const toward = index % 2 === 0 ? 1 : -1;
      mesh.position.set(x + toward * 0.5, groundAt(x, z) + TIMBER[0] * 0.45, z);
      mesh.rotation.z = -toward * LEAN;
      mesh.visible = false;
      mesh.name = `raid-brace-${index}`;
      this.#braces.push(mesh);
      this.root.add(mesh);
    });
  }

  /** Forgets the placement for a new world. */
  clear(): void {
    this.#tracks.clear();
    for (const mesh of this.#braces) {
      this.root.remove(mesh);
    }
    this.#braces.length = 0;
    this.#placed = false;
  }
}
