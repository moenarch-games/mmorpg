//! The interface theme both clients draw with (`content/ui/theme.json`):
//! colours, corner radii, spacing, text sizes and motion timings. The browser
//! turns the file into CSS custom properties at build time; the native client
//! turns this parse into egui visuals. One file decides the look of both.
use serde::Deserialize;

/// The shared theme file.
pub const THEME_JSON: &str = include_str!("../../../content/ui/theme.json");
const SCHEMA_VERSION: u64 = 1;

/// An sRGB colour with straight alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("`{text}` is not #rrggbb or #rrggbbaa"))
        })
    }
}

impl Rgba {
    fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if !matches!(hex.len(), 6 | 8) || !hex.is_ascii() {
            return None;
        }
        let byte = |index: usize| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok();
        let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
        Some(Self([byte(0)?, byte(2)?, byte(4)?, alpha]))
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    /// The screen behind every panel.
    pub ink: Rgba,
    pub panel: Rgba,
    /// A control or card on a panel.
    pub panel_raised: Rgba,
    pub border: Rgba,
    /// The border of the selected or primary element.
    pub border_strong: Rgba,
    pub gold: Rgba,
    pub gold_bright: Rgba,
    /// Text on gold.
    pub gold_ink: Rgba,
    pub text: Rgba,
    pub text_heading: Rgba,
    pub text_muted: Rgba,
    pub accent: Rgba,
    pub accent_fill: Rgba,
    pub danger: Rgba,
    pub focus: Rgba,
}

/// Corner radii in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Radius {
    pub panel: f32,
    pub control: f32,
}

/// Spacing steps in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

/// Text sizes in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSizes {
    pub display: f32,
    pub heading: f32,
    pub body: f32,
    pub small: f32,
}

/// Transition lengths in milliseconds; reduced motion skips them.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub fast: u32,
    pub normal: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    schema_version: u64,
    pub color: Colors,
    pub radius: Radius,
    pub space: Space,
    pub text: TextSizes,
    pub motion_ms: Motion,
}

impl Theme {
    /// Parses a theme file of the supported schema.
    ///
    /// # Errors
    /// The file is not a theme of this schema.
    pub fn parse(json: &str) -> Result<Self, String> {
        let theme: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if theme.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "theme schema {} is not {SCHEMA_VERSION}",
                theme.schema_version
            ));
        }
        Ok(theme)
    }

    /// The shared theme.
    #[must_use]
    pub fn shared() -> Self {
        Self::parse(THEME_JSON).expect("content/ui/theme.json is a valid theme")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_theme_parses() {
        let theme = Theme::shared();
        assert_eq!(theme.color.gold, Rgba([0xc6, 0xa8, 0x5f, 0xff]));
        assert_eq!(theme.color.panel.0[3], 0xdc);
        assert!(theme.text.display > theme.text.heading);
    }

    #[test]
    fn malformed_colours_and_schemas_fail_closed() {
        assert_eq!(Rgba::parse("#abc"), None);
        assert_eq!(Rgba::parse("c6a85f"), None);
        assert_eq!(Rgba::parse("#gg0000"), None);
        let newer = THEME_JSON.replace("\"schema_version\": 1", "\"schema_version\": 2");
        assert!(Theme::parse(&newer).is_err());
    }
}
