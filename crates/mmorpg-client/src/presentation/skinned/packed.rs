//! The packed glTF asset-tooling publishes file by file (the Ranger figures and their equipment,
//! #866), read as published: `EXT_meshopt_compression` buffer views are decoded by meshoptimizer's
//! own decoder (the `meshopt` crate) and `KHR_mesh_quantization` attributes become floats, so the
//! ordinary `gltf` import reads them. Their KTX2 textures are transcoded with every bundle's
//! (`bundle_glb`).

use serde_json::Value;
use std::collections::BTreeSet;

/// The glTF extensions [`decode_views`] and [`dequantize`] resolve.
pub(super) const EXTENSIONS: [&str; 2] = ["EXT_meshopt_compression", "KHR_mesh_quantization"];

const FLOAT: u64 = 5126;

/// Replaces every compressed view of `json` with a view of its decoded bytes, appended to
/// `buffers` as a buffer of its own.
pub(super) fn decode_views(json: &mut Value, buffers: &mut Vec<Vec<u8>>) -> Result<(), String> {
    for view in json
        .get_mut("bufferViews")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        let Some(packed) = view
            .get_mut("extensions")
            .and_then(Value::as_object_mut)
            .and_then(|extensions| extensions.remove("EXT_meshopt_compression"))
        else {
            continue;
        };
        if view["extensions"]
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
        {
            view.as_object_mut().map(|view| view.remove("extensions"));
        }
        let size = |name: &str| packed[name].as_u64().unwrap_or(0) as usize;
        let (offset, length) = (size("byteOffset"), size("byteLength"));
        let source = buffers
            .get(size("buffer"))
            .and_then(|buffer| buffer.get(offset..offset + length))
            .ok_or("a compressed view lies outside its buffer")?;
        let data = decode(
            source,
            packed["mode"].as_str().unwrap_or_default(),
            packed["filter"].as_str().unwrap_or("NONE"),
            size("count"),
            size("byteStride"),
        )?;
        view["buffer"] = buffers.len().into();
        view["byteOffset"] = 0.into();
        view["byteLength"] = data.len().into();
        buffers.push(data);
    }
    Ok(())
}

/// Calls `$body` with `$element` the `[u32; K]` vertex of a `$stride`-byte stride (4 to 128).
macro_rules! by_stride {
    ($stride:expr, $element:ident => $body:expr, $($k:literal)*) => {
        match $stride {
            $(stride if stride == $k * 4 => {
                type $element = [u32; $k];
                $body
            })*
            stride => Err(format!("unsupported compressed vertex stride {stride}")),
        }
    };
}

/// One compressed view's `count` elements of `stride` bytes, decoded and filtered.
fn decode(
    source: &[u8],
    mode: &str,
    filter: &str,
    count: usize,
    stride: usize,
) -> Result<Vec<u8>, String> {
    let failed = |error: meshopt::Error| format!("meshopt {mode} decode failed: {error}");
    match (mode, filter, stride) {
        ("TRIANGLES", "NONE", 2) => meshopt::decode_index_buffer::<u16>(source, count)
            .map(|data| bytemuck::cast_slice(&data).to_vec())
            .map_err(failed),
        ("TRIANGLES", "NONE", 4) => meshopt::decode_index_buffer::<u32>(source, count)
            .map(|data| bytemuck::cast_slice(&data).to_vec())
            .map_err(failed),
        ("INDICES", "NONE", 2) => meshopt::decode_index_sequence::<u16>(source, count)
            .map(|data| bytemuck::cast_slice(&data).to_vec())
            .map_err(failed),
        ("INDICES", "NONE", 4) => meshopt::decode_index_sequence::<u32>(source, count)
            .map(|data| bytemuck::cast_slice(&data).to_vec())
            .map_err(failed),
        ("ATTRIBUTES", "OCTAHEDRAL", 4) => {
            let mut data =
                meshopt::decode_vertex_buffer::<[i8; 4]>(source, count).map_err(failed)?;
            meshopt::decode_filter_oct(&mut data);
            Ok(bytemuck::cast_slice(&data).to_vec())
        }
        ("ATTRIBUTES", "OCTAHEDRAL", 8) => {
            let mut data =
                meshopt::decode_vertex_buffer::<[i16; 4]>(source, count).map_err(failed)?;
            meshopt::decode_filter_oct(&mut data);
            Ok(bytemuck::cast_slice(&data).to_vec())
        }
        ("ATTRIBUTES", "QUATERNION", 8) => {
            let mut data =
                meshopt::decode_vertex_buffer::<[i16; 4]>(source, count).map_err(failed)?;
            meshopt::decode_filter_quat(&mut data);
            Ok(bytemuck::cast_slice(&data).to_vec())
        }
        ("ATTRIBUTES", "NONE" | "EXPONENTIAL", stride) => by_stride!(stride, Vertex => {
            let mut data = meshopt::decode_vertex_buffer::<Vertex>(source, count).map_err(failed)?;
            if filter == "EXPONENTIAL" {
                meshopt::decode_filter_exp(&mut data);
            }
            Ok(bytemuck::cast_slice(&data).to_vec())
        }, 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32),
        _ => Err(format!(
            "unsupported compressed view: mode {mode}, filter {filter}, stride {stride}"
        )),
    }
}

/// Rewrites every quantized vertex attribute, morph target and animation output of `json` as
/// floats, each appended to `buffers` as a buffer of its own; joints and indices stay integers.
pub(super) fn dequantize(json: &mut Value, buffers: &mut Vec<Vec<u8>>) -> Result<(), String> {
    let mut quantized = BTreeSet::new();
    let index = |value: &Value| value.as_u64().map(|index| index as usize);
    for primitive in list(&json["meshes"])
        .iter()
        .flat_map(|mesh| list(&mesh["primitives"]))
    {
        for (name, accessor) in primitive["attributes"].as_object().into_iter().flatten() {
            if !name.starts_with("JOINTS_") {
                quantized.extend(index(accessor));
            }
        }
        for target in list(&primitive["targets"]) {
            quantized.extend(
                target
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(_, a)| index(a)),
            );
        }
    }
    for sampler in list(&json["animations"])
        .iter()
        .flat_map(|animation| list(&animation["samplers"]))
    {
        quantized.extend(index(&sampler["output"]));
    }
    for accessor_index in quantized {
        let accessor = &json["accessors"][accessor_index];
        let component = accessor["componentType"].as_u64().unwrap_or(FLOAT);
        if component == FLOAT {
            continue;
        }
        if accessor.get("sparse").is_some() {
            return Err(format!("quantized accessor {accessor_index} is sparse"));
        }
        let normalized = accessor["normalized"].as_bool().unwrap_or(false);
        let convert = |value: f64| to_float(component, normalized, value);
        let width = components(accessor["type"].as_str().unwrap_or("SCALAR"));
        let count = accessor["count"].as_u64().unwrap_or(0) as usize;
        let view = index(&accessor["bufferView"]);
        let floats = view
            .map(|view| {
                read(
                    &json["bufferViews"][view],
                    accessor,
                    buffers,
                    component,
                    count * width,
                    width,
                )
                .map(|values| values.into_iter().map(convert).collect::<Vec<f32>>())
            })
            .transpose()?;
        let bounds: Vec<(&str, Value)> = ["min", "max"]
            .into_iter()
            .filter_map(|name| {
                let values = accessor[name].as_array()?;
                let floats = values
                    .iter()
                    .map(|value| Value::from(f64::from(convert(value.as_f64().unwrap_or(0.)))))
                    .collect();
                Some((name, Value::Array(floats)))
            })
            .collect();
        let new_view = floats.map(|floats| {
            let views = json["bufferViews"].as_array().map_or(0, Vec::len);
            let bytes: Vec<u8> = bytemuck::cast_slice(&floats).to_vec();
            json["bufferViews"]
                .as_array_mut()
                .expect("a quantized accessor's document has views")
                .push(serde_json::json!({
                    "buffer": buffers.len(), "byteOffset": 0, "byteLength": bytes.len()
                }));
            buffers.push(bytes);
            views
        });
        let accessor = &mut json["accessors"][accessor_index];
        if let Some(object) = accessor.as_object_mut() {
            object.remove("normalized");
            object.remove("byteOffset");
            object.insert("componentType".into(), FLOAT.into());
            if let Some(view) = new_view {
                object.insert("bufferView".into(), view.into());
            }
            for (name, values) in bounds {
                object.insert(name.into(), values);
            }
        }
    }
    Ok(())
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn components(kind: &str) -> usize {
    match kind {
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => 1,
    }
}

/// The byte width of glTF component type `component`.
fn width(component: u64) -> usize {
    match component {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    }
}

/// A component value as a float: normalized integers per the glTF specification, other values as
/// they are.
fn to_float(component: u64, normalized: bool, value: f64) -> f32 {
    let value = if normalized {
        match component {
            5120 => (value / 127.).max(-1.),
            5121 => value / 255.,
            5122 => (value / 32767.).max(-1.),
            5123 => value / 65535.,
            _ => value,
        }
    } else {
        value
    };
    value as f32
}

/// The `values` components of an integer accessor in its view, `width` per element.
fn read(
    view: &Value,
    accessor: &Value,
    buffers: &[Vec<u8>],
    component: u64,
    values: usize,
    per_element: usize,
) -> Result<Vec<f64>, String> {
    let size = width(component);
    let buffer = view["buffer"]
        .as_u64()
        .and_then(|buffer| buffers.get(buffer as usize))
        .ok_or("a quantized accessor's view names no buffer")?;
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"]
        .as_u64()
        .map_or(size * per_element, |stride| stride as usize);
    (0..values)
        .map(|value| {
            let at = start + value / per_element * stride + value % per_element * size;
            let bytes = buffer
                .get(at..at + size)
                .ok_or("a quantized accessor lies outside its buffer")?;
            Ok(match component {
                5120 => f64::from(bytes[0] as i8),
                5121 => f64::from(bytes[0]),
                5122 => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
                5123 => f64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
                _ => f64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            })
        })
        .collect()
}
