//! Bounded one-time PNG decode and egui upload of the approved Warden illustrations.
use egui::{ColorImage, TextureHandle, TextureOptions, Vec2};
use std::{collections::BTreeMap, io::Cursor};

const IMAGES: [(u8, &[u8]); 4] = [
    (
        1,
        include_bytes!("../../../../web/assets/warden-icons/warden-1-512.png"),
    ),
    (
        2,
        include_bytes!("../../../../web/assets/warden-icons/warden-2-512.png"),
    ),
    (
        3,
        include_bytes!("../../../../web/assets/warden-icons/warden-3-512.png"),
    ),
    (
        4,
        include_bytes!("../../../../web/assets/warden-icons/warden-4-512.png"),
    ),
];
struct Illustration {
    color: TextureHandle,
    locked: TextureHandle,
}
#[derive(Default)]
enum State {
    #[default]
    Unloaded,
    Ready(BTreeMap<u8, Illustration>),
    Failed(String),
}
#[derive(Default)]
pub(super) struct AbilityArt {
    state: State,
}
impl AbilityArt {
    fn load(ctx: &egui::Context) -> Result<BTreeMap<u8, Illustration>, String> {
        let mut icons = BTreeMap::new();
        for (ability, bytes) in IMAGES {
            let mut reader =
                image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(512);
            limits.max_image_height = Some(512);
            limits.max_alloc = Some(4 * 1024 * 1024);
            reader.limits(limits);
            let decoded = reader
                .decode()
                .map_err(|cause| format!("Warden icon {ability}: {cause}"))?;
            if decoded.width() != 512 || decoded.height() != 512 {
                return Err(format!("Warden icon {ability} has unexpected dimensions"));
            }
            let rgba = decoded.to_rgba8();
            let gray = decoded.grayscale().to_rgba8();
            let texture = |name, pixels: &[u8]| {
                ctx.load_texture(
                    format!("approved-warden-{ability}-{name}"),
                    ColorImage::from_rgba_unmultiplied([512, 512], pixels),
                    TextureOptions::LINEAR,
                )
            };
            icons.insert(
                ability,
                Illustration {
                    color: texture("color", rgba.as_raw()),
                    locked: texture("locked", gray.as_raw()),
                },
            );
        }
        Ok(icons)
    }

    pub(super) fn image(
        &mut self,
        ctx: &egui::Context,
        ability: u8,
        locked: bool,
    ) -> Option<egui::Image<'static>> {
        if !(1..=4).contains(&ability) {
            return None;
        }
        if matches!(self.state, State::Unloaded) {
            self.state = match Self::load(ctx) {
                Ok(icons) => State::Ready(icons),
                Err(cause) => State::Failed(cause),
            };
        }
        let State::Ready(icons) = &self.state else {
            return None;
        };
        let icon = icons.get(&ability)?;
        let texture = if locked { &icon.locked } else { &icon.color };
        Some(egui::Image::new((texture.id(), Vec2::splat(44.0))))
    }

    pub(super) fn problem(&self) -> Option<&str> {
        match &self.state {
            State::Failed(cause) => Some(cause),
            State::Ready(_) | State::Unloaded => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_art_decodes_once_and_does_not_upload_again_for_repeated_or_locked_frames() {
        let context = egui::Context::default();
        context.tex_manager().write().take_delta().clear();
        let mut art = AbilityArt::default();
        assert!(art.image(&context, 1, false).is_some());
        let mut first = context.tex_manager().write().take_delta();
        let uploads = first.set.len();
        first.clear();
        assert_eq!(uploads, 8, "four color and four locked textures");
        for ability in 1..=4 {
            assert!(art.image(&context, ability, false).is_some());
            assert!(art.image(&context, ability, true).is_some());
        }
        assert!(art.image(&context, 17, false).is_none());
        assert!(art.problem().is_none());
        assert!(context.tex_manager().write().take_delta().set.is_empty());
    }
}
