struct Environment {
    camera: mat4x4<f32>, eye: vec4<f32>, direction: vec4<f32>,
    sky: vec4<f32>, ground: vec4<f32>, sun_direction: vec4<f32>, sun: vec4<f32>,
    fog: vec4<f32>, settings: vec4<f32>,
    background: vec4<f32>,
};
@group(0) @binding(0) var<uniform> environment: Environment;
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>, @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>, @location(3) emissive: vec4<f32>,
};
@vertex fn vs_main(
    @location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) vertex_color: vec3<f32>,
    @location(3) m0:vec4<f32>, @location(4) m1:vec4<f32>, @location(5) m2:vec4<f32>, @location(6) m3:vec4<f32>,
    @location(7) n0:vec4<f32>, @location(8) n1:vec4<f32>, @location(9) n2:vec4<f32>,
    @location(10) color:vec4<f32>, @location(11) emissive:vec4<f32>,
)->Output {
    var out: Output;
    let world=mat4x4<f32>(m0,m1,m2,m3)*vec4<f32>(position,1.0);
    out.position=environment.camera*world;out.world=world.xyz;
    out.normal=mat3x3<f32>(n0.xyz,n1.xyz,n2.xyz)*normal;
    // Material and vertex colors were decoded from sRGB once during CPU upload.
    out.color=vec4<f32>(color.rgb*vertex_color,color.a);out.emissive=emissive;return out;
}
@fragment fn fs_main(input:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32>{
    var normal=normalize(input.normal);if !front{normal=-normal;}
    let base=input.color.rgb;
    var result=base;
    if input.emissive.a<0.5 {
        let light=normalize(environment.sun_direction.xyz);
        let view=normalize(environment.eye.xyz-input.world);
        let half_vector=normalize(light+view);
        let nl=max(dot(normal,light),0.0);let nv=max(dot(normal,view),0.00001);
        let nh=max(dot(normal,half_vector),0.0);let vh=max(dot(view,half_vector),0.0);
        // Current Pages MeshStandardMaterial: roughness .86, metalness .02.
        let alpha=0.86*0.86;let a2=alpha*alpha;
        let denom=nh*nh*(a2-1.0)+1.0;
        let distribution=a2/(3.141592653589793*denom*denom);
        let visibility=0.5/max(nl*sqrt(a2+(1.0-a2)*nv*nv)+nv*sqrt(a2+(1.0-a2)*nl*nl),0.00001);
        let f0=mix(vec3<f32>(0.04),base,0.02);
        let fresnel=f0+(vec3<f32>(1.0)-f0)*pow(1.0-vh,5.0);
        let diffuse=base*(0.98/3.141592653589793);
        let hemi=mix(environment.ground.rgb,environment.sky.rgb,normal.y*0.5+0.5)*environment.sky.a;
        result=hemi*diffuse+environment.sun.rgb*environment.sun_direction.a*nl*(diffuse+distribution*visibility*fresnel)+input.emissive.rgb;
    }
    // Three.js linear fog uses view depth rather than radial distance.
    let depth=dot(input.world-environment.eye.xyz,environment.direction.xyz);
    let fog=smoothstep(environment.fog.a,environment.settings.x,depth);
    return vec4<f32>(mix(result,environment.fog.rgb,fog),input.color.a);
}
@vertex fn sky_vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{
    let points=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    return vec4<f32>(points[index],1.0,1.0);
}
// The current browser renderer consumes an opaque environment background.
@fragment fn sky_fragment()->@location(0) vec4<f32>{
    return environment.background;
}
