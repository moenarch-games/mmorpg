import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { applyOutpostWear } from "../src/world/outpost-wear";

// The Outpost's wear is cosmetic: don't simulate missing plaster and roof geometry with black pixels.
describe("Outpost wear shader", () => {
  test("keeps surface ageing without pixel-only holes", () => {
    const material = new THREE.ShaderMaterial();
    material.name = "outpost-plaster";
    const root = new THREE.Mesh(new THREE.PlaneGeometry(), material);
    const mask = new THREE.Texture();
    expect(applyOutpostWear([root], new Map([["plaster", mask]]))).toBe(1);
    expect(applyOutpostWear([root], new Map([["plaster", mask]]))).toBe(0);
    expect(material.uniforms.wearMask?.value).toBe(mask);

    const shader = {
      vertexShader: "out vec4 vColor;\nvoid main() {\n    vColor = color;\n}",
      fragmentShader: "MaterialSample sampleMaterial() { return m; }\nvec3 litRadiance() { return vec3(1.0); }",
      uniforms: {},
    };
    material.onBeforeCompile(shader as Parameters<typeof material.onBeforeCompile>[0], {} as THREE.WebGLRenderer);
    expect(shader.vertexShader).toContain("vWear = getBatchingColor");
    expect(shader.vertexShader).not.toContain("vPlacement");
    expect(shader.fragmentShader).toContain("0.45 * reveal");
    expect(shader.fragmentShader).not.toContain("MISSING_SHARE");
    expect(shader.fragmentShader).not.toContain("texelFetch");
  });
});
