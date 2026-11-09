//! OpenRaster, the layered format Krita opens and saves natively.
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops;
use image::{DynamicImage, RgbaImage};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::grid::{BOUNDS_PREFIX, Bounds};

pub const PAINT_HEIGHT: &str = "paint height";
pub const PAINT_BIOME: &str = "paint biome";
/// No layer of a document this tool reads may unpack to more than this.
const MAX_ENTRY_BYTES: u64 = 1 << 30;

pub struct Layer {
    pub name: String,
    pub image: DynamicImage,
    pub visible: bool,
    pub opacity: f64,
}

impl Layer {
    pub fn new(name: impl Into<String>, image: impl Into<DynamicImage>) -> Self {
        Self {
            name: name.into(),
            image: image.into(),
            visible: true,
            opacity: 1.,
        }
    }

    #[must_use]
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    #[must_use]
    pub fn opacity(mut self, opacity: f64) -> Self {
        self.opacity = opacity;
        self
    }
}

pub fn png_bytes(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut bytes = Cursor::new(Vec::new());
    let encoder =
        PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::Adaptive);
    image
        .write_with_encoder(encoder)
        .map_err(|error| error.to_string())?;
    Ok(bytes.into_inner())
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Writes a document; `layers` run bottom to top and include one named `beauty`.
pub fn write(path: &Path, bounds: Bounds, layers: &[Layer]) -> Result<(), String> {
    let temporary = path.with_extension("ora.tmp");
    write_to(&temporary, bounds, layers)?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn write_to(path: &Path, bounds: Bounds, layers: &[Layer]) -> Result<(), String> {
    let text = |error: zip::result::ZipError| error.to_string();
    let io = |error: std::io::Error| error.to_string();
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut archive = ZipWriter::new(File::create(path).map_err(io)?);
    archive.start_file("mimetype", stored).map_err(text)?;
    archive.write_all(b"image/openraster").map_err(io)?;
    let mut entries = String::new();
    for (index, layer) in layers.iter().rev().enumerate() {
        let source = format!("data/layer{index}.png");
        archive.start_file(source.as_str(), stored).map_err(text)?;
        archive.write_all(&png_bytes(&layer.image)?).map_err(io)?;
        entries.push_str(&format!(
            "<layer name=\"{}\" src=\"{source}\" x=\"0\" y=\"0\" opacity=\"{}\" visibility=\"{}\"/>",
            escaped(&layer.name),
            layer.opacity,
            if layer.visible { "visible" } else { "hidden" },
        ));
    }
    let (width, height) = bounds.size();
    archive.start_file("stack.xml", stored).map_err(text)?;
    write!(
        archive,
        "<?xml version='1.0' encoding='UTF-8'?>\n<image w=\"{width}\" h=\"{height}\" version=\"0.0.3\"><stack>{entries}</stack></image>"
    )
    .map_err(io)?;
    let merged = layers
        .iter()
        .find(|layer| layer.name == "beauty")
        .ok_or("a document needs a beauty layer")?;
    archive
        .start_file("mergedimage.png", stored)
        .map_err(text)?;
    archive.write_all(&png_bytes(&merged.image)?).map_err(io)?;
    archive
        .start_file("Thumbnails/thumbnail.png", stored)
        .map_err(text)?;
    archive
        .write_all(&png_bytes(&merged.image.thumbnail(256, 256))?)
        .map_err(io)?;
    archive.finish().map_err(text)?;
    Ok(())
}

/// The two paint layers of a saved document, each composed onto a full-size image.
pub struct PaintLayers {
    pub bounds: Bounds,
    pub height: RgbaImage,
    pub biome: RgbaImage,
}

fn entry(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|error| format!("{name}: {error}"))?;
    if file.size() > MAX_ENTRY_BYTES {
        return Err(format!("{name} is too large"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_ENTRY_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{name}: {error}"))?;
    Ok(bytes)
}

/// Reads a document saved by Krita, which trims layers and may reorder them.
pub fn read(path: &Path) -> Result<PaintLayers, String> {
    let show = path.display();
    let file = File::open(path).map_err(|error| format!("{show}: {error}"))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("{show} is not an OpenRaster file: {error}"))?;
    let stack = entry(&mut archive, "stack.xml")?;
    let stack = String::from_utf8(stack).map_err(|_| "stack.xml is not text".to_owned())?;
    let document =
        roxmltree::Document::parse(&stack).map_err(|error| format!("stack.xml: {error}"))?;
    let root = document.root_element();
    let layers: Vec<_> = root
        .descendants()
        .filter(|node| node.has_tag_name("layer"))
        .collect();
    let bounds = layers
        .iter()
        .filter_map(|layer| layer.attribute("name"))
        .find(|name| name.starts_with(BOUNDS_PREFIX))
        .and_then(Bounds::parse)
        .ok_or_else(|| format!("{show} has no {BOUNDS_PREFIX} layer: export it with this tool"))?;
    let dimension = |name: &str| {
        root.attribute(name)
            .and_then(|value| value.parse::<u32>().ok())
    };
    if (dimension("w"), dimension("h")) != (Some(bounds.size().0), Some(bounds.size().1)) {
        return Err(format!(
            "{show} was resized or cropped: its bounds expect {:?}",
            bounds.size()
        ));
    }
    let mut compose = |name: &str| -> Result<RgbaImage, String> {
        let layer = layers
            .iter()
            .find(|layer| layer.attribute("name") == Some(name))
            .ok_or_else(|| format!("{show} lacks the layer {name}"))?;
        let source = layer
            .attribute("src")
            .ok_or_else(|| format!("{name} has no image"))?;
        let bytes = entry(&mut archive, source)?;
        let image = image::load_from_memory(&bytes).map_err(|error| format!("{name}: {error}"))?;
        let offset = |axis: &str| {
            layer
                .attribute(axis)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
        };
        let (width, height) = bounds.size();
        let mut canvas = RgbaImage::new(width, height);
        imageops::replace(&mut canvas, &image.to_rgba8(), offset("x"), offset("y"));
        Ok(canvas)
    };
    Ok(PaintLayers {
        height: compose(PAINT_HEIGHT)?,
        biome: compose(PAINT_BIOME)?,
        bounds,
    })
}
