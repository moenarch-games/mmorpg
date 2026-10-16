struct Environment {
    camera: mat4x4<f32>, eye: vec4<f32>, direction: vec4<f32>,
    sky: vec4<f32>, ground: vec4<f32>, sun_direction: vec4<f32>, sun: vec4<f32>,
    fog: vec4<f32>, settings: vec4<f32>,
    zenith: vec4<f32>, upper: vec4<f32>, horizon: vec4<f32>, sky_ground: vec4<f32>, clouds: vec4<f32>,
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
fn linear_channel(c:f32)->f32{if c<=0.04045{return c/12.92;}return pow((c+0.055)/1.055,2.4);}
fn cloud(p:vec2<f32>,centre:vec2<f32>,radii:vec2<f32>,opacity:f32)->f32{return opacity*(1.0-clamp(length((p-centre)/radii)/0.7,0.0,1.0));}
@fragment fn sky_fragment(@builtin(position) pixel:vec4<f32>)->@location(0) vec4<f32>{
    let h=environment.settings.w;let y=pixel.y/environment.settings.z;
    let upper=max(0.0,h*0.62);let horizon=max(upper,h-0.005);let ground=max(horizon,h+0.015);
    var c=environment.sky_ground.rgb;
    if y<upper{c=mix(environment.zenith.rgb,environment.upper.rgb,clamp(y/max(upper,0.00001),0.0,1.0));}
    else if y<horizon{c=mix(environment.upper.rgb,environment.horizon.rgb,clamp((y-upper)/max(horizon-upper,0.00001),0.0,1.0));}
    else if y<ground{c=mix(environment.horizon.rgb,environment.sky_ground.rgb,clamp((y-horizon)/max(ground-horizon,0.00001),0.0,1.0));}
    let height=environment.settings.z*0.42;
    let p=vec2<f32>(((pixel.x-environment.clouds.x)%1400.0+1400.0)%1400.0,pixel.y-environment.settings.z*(h-0.48));
    var transmittance=1.0;
    transmittance*=1.0-cloud(p,vec2<f32>(168.0,height*0.62),vec2<f32>(160.0,34.0),0.55);
    transmittance*=1.0-cloud(p,vec2<f32>(252.0,height*0.55),vec2<f32>(120.0,26.0),0.45);
    transmittance*=1.0-cloud(p,vec2<f32>(658.0,height*0.75),vec2<f32>(220.0,40.0),0.45);
    transmittance*=1.0-cloud(p,vec2<f32>(994.0,height*0.48),vec2<f32>(140.0,30.0),0.50);
    transmittance*=1.0-cloud(p,vec2<f32>(1232.0,height*0.70),vec2<f32>(190.0,36.0),0.40);
    if p.y>=0.0&&p.y<=height{c=mix(c,vec3<f32>(1.0),(1.0-transmittance)*0.75);}
    return vec4<f32>(linear_channel(c.r),linear_channel(c.g),linear_channel(c.b),1.0);
}
