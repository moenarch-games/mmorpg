// Flat HUD rectangles in normalised window coordinates (0,0 top left).
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex fn vs_main(
    @builtin(vertex_index) index: u32,
    @location(0) low: vec2<f32>,
    @location(1) high: vec2<f32>,
    @location(2) color: vec3<f32>,
) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let point = mix(low, high, corners[index]);
    var output: VertexOutput;
    output.clip_position = vec4<f32>(point.x * 2.0 - 1.0, 1.0 - point.y * 2.0, 0.0, 1.0);
    output.color = color;
    return output;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
