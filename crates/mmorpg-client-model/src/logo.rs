//! The game's logo and its opening animation (`content/ui/logo.json`): a
//! double ring, the vale's two peaks, the keep between them and the waystone
//! gem above, in a 100 × 100 box, drawn on in phases over the first seconds.
//! The browser turns the file into inline SVG at build time; the native client
//! paints these shapes with egui. Colours name `theme` colours.
use serde::Deserialize;

/// The shared logo file.
pub const LOGO_JSON: &str = include_str!("../../../content/ui/logo.json");
const SCHEMA_VERSION: u64 = 1;

/// When each part of the logo draws on, in milliseconds from the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Ring,
    Land,
    Gem,
}

/// One drawn part of the logo.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Shape {
    /// A stroked circle.
    Circle {
        centre: [f32; 2],
        radius: f32,
        width: f32,
        color: String,
        phase: Phase,
    },
    /// An open stroked polyline.
    Line {
        points: Vec<[f32; 2]>,
        width: f32,
        color: String,
        phase: Phase,
    },
    /// A stroked polyline whose ends close at the ground.
    Outline {
        points: Vec<[f32; 2]>,
        width: f32,
        color: String,
        phase: Phase,
    },
    /// A filled polygon that fades and grows in.
    Fill {
        points: Vec<[f32; 2]>,
        color: String,
        phase: Phase,
    },
}

impl Shape {
    #[must_use]
    pub const fn phase(&self) -> Phase {
        match self {
            Self::Circle { phase, .. }
            | Self::Line { phase, .. }
            | Self::Outline { phase, .. }
            | Self::Fill { phase, .. } => *phase,
        }
    }
}

/// The timeline in milliseconds: each phase's `[start, end]`, then the hold
/// until the splash fades out over `fade`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub ring: [u32; 2],
    pub land: [u32; 2],
    pub gem: [u32; 2],
    pub title: [u32; 2],
    pub hold: u32,
    pub fade: u32,
}

impl Timeline {
    /// How far `phase` has drawn at `ms`, 0 to 1, eased out.
    #[must_use]
    pub fn progress(&self, phase: Phase, ms: u32) -> f32 {
        let [start, end] = match phase {
            Phase::Ring => self.ring,
            Phase::Land => self.land,
            Phase::Gem => self.gem,
        };
        ease(span(start, end, ms))
    }

    /// How far the title has faded in at `ms`, 0 to 1, eased out.
    #[must_use]
    pub fn title(&self, ms: u32) -> f32 {
        ease(span(self.title[0], self.title[1], ms))
    }

    /// The splash's opacity at `ms`: whole until the hold ends, then fading to 0.
    #[must_use]
    pub fn opacity(&self, ms: u32) -> f32 {
        1.0 - span(self.hold, self.hold + self.fade, ms)
    }

    /// When the splash is gone.
    #[must_use]
    pub const fn end(&self) -> u32 {
        self.hold + self.fade
    }
}

fn span(start: u32, end: u32, ms: u32) -> f32 {
    if ms <= start {
        0.0
    } else if ms >= end || end <= start {
        1.0
    } else {
        (ms - start) as f32 / (end - start) as f32
    }
}

/// Cubic ease-out, as the browser's `cubic-bezier(.33, 1, .68, 1)`.
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    schema_version: u64,
    pub title: String,
    pub subtitle: String,
    pub timeline_ms: Timeline,
    pub shapes: Vec<Shape>,
}

impl Logo {
    /// Parses a logo file of the supported schema.
    ///
    /// # Errors
    /// The file is not a logo of this schema.
    pub fn parse(json: &str) -> Result<Self, String> {
        let logo: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if logo.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "logo schema {} is not {SCHEMA_VERSION}",
                logo.schema_version
            ));
        }
        Ok(logo)
    }

    /// The shared logo.
    #[must_use]
    pub fn shared() -> Self {
        Self::parse(LOGO_JSON).expect("content/ui/logo.json is a valid logo")
    }
}

/// The first `progress` (0 to 1) of a polyline by length, for drawing it on.
#[must_use]
pub fn partial(points: &[[f32; 2]], progress: f32) -> Vec<[f32; 2]> {
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
        .collect();
    let mut remaining = lengths.iter().sum::<f32>() * progress.clamp(0.0, 1.0);
    let mut drawn = points.first().copied().into_iter().collect::<Vec<_>>();
    for (pair, length) in points.windows(2).zip(lengths) {
        if remaining >= length {
            drawn.push(pair[1]);
            remaining -= length;
        } else {
            if length > 0.0 && remaining > 0.0 {
                let t = remaining / length;
                drawn.push([
                    pair[0][0] + (pair[1][0] - pair[0][0]) * t,
                    pair[0][1] + (pair[1][1] - pair[0][1]) * t,
                ]);
            }
            break;
        }
    }
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn the_shared_logo_parses_and_names_theme_colours() {
        let logo = Logo::shared();
        let theme = Theme::shared();
        assert_eq!(logo.title, "GREYHAVEN");
        assert!(logo.shapes.iter().any(|shape| shape.phase() == Phase::Gem));
        for shape in &logo.shapes {
            let (Shape::Circle { color, .. }
            | Shape::Line { color, .. }
            | Shape::Outline { color, .. }
            | Shape::Fill { color, .. }) = shape;
            assert!(theme.color.named(color).is_some(), "{color}");
        }
    }

    #[test]
    fn the_timeline_draws_then_holds_then_fades() {
        let timeline = Logo::shared().timeline_ms;
        assert_eq!(timeline.progress(Phase::Ring, 0), 0.0);
        assert_eq!(timeline.progress(Phase::Ring, timeline.ring[1]), 1.0);
        assert!(timeline.progress(Phase::Gem, timeline.land[0]) == 0.0);
        assert_eq!(timeline.opacity(timeline.hold), 1.0);
        assert_eq!(timeline.opacity(timeline.end()), 0.0);
    }

    #[test]
    fn a_polyline_draws_on_by_length() {
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert_eq!(partial(&line, 0.0), vec![[0.0, 0.0]]);
        assert_eq!(partial(&line, 0.25), vec![[0.0, 0.0], [5.0, 0.0]]);
        assert_eq!(
            partial(&line, 0.75),
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0]]
        );
        assert_eq!(partial(&line, 1.0), line.to_vec());
    }
}
