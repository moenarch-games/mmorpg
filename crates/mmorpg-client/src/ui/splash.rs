//! The opening splash (`content/ui/logo.json`), as the browser's: the logo
//! draws itself on over the shared timeline, the gem lights, the title
//! settles, then the splash fades into the selection stage beneath it. A
//! click skips it; reduced motion shows the finished logo, then fades.
use super::theme;
use egui::{Align2, Color32, FontId, Pos2, Shape, Stroke, Vec2, epaint::PathShape};
use mmorpg_client_model::logo::{self, Logo};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// Reduced motion: the finished logo holds this long, then fades.
const STILL_HOLD_MS: u32 = 900;
const STILL_FADE_MS: u32 = 400;

fn shared() -> &'static Logo {
    static LOGO: OnceLock<Logo> = OnceLock::new();
    LOGO.get_or_init(Logo::shared)
}

#[derive(Default)]
pub struct Splash {
    started: Option<Instant>,
    done: bool,
}

impl Splash {
    /// Paints the splash at `now`. Returns whether it still covers the whole
    /// window, so nothing beneath it needs drawing or can take a click.
    pub fn show(&mut self, ctx: &egui::Context, now: Instant, reduced_motion: bool) -> bool {
        if self.done {
            return false;
        }
        let started = *self.started.get_or_insert(now);
        let ms =
            u32::try_from(now.saturating_duration_since(started).as_millis()).unwrap_or(u32::MAX);
        let timeline = shared().timeline_ms;
        let (hold, end) = if reduced_motion {
            (STILL_HOLD_MS, STILL_HOLD_MS + STILL_FADE_MS)
        } else {
            (timeline.hold, timeline.end())
        };
        if ms >= end || ctx.input(|input| input.pointer.any_pressed()) {
            self.done = true;
            return false;
        }
        let opacity = if ms <= hold {
            1.0
        } else {
            1.0 - (ms - hold) as f32 / (end - hold) as f32
        };
        // Reduced motion draws the finished logo.
        let at = if reduced_motion { u32::MAX } else { ms };
        paint(ctx, at, opacity);
        ctx.request_repaint_after(Duration::from_millis(16));
        ms < hold
    }
}

fn faded(color: Color32, opacity: f32) -> Color32 {
    color.gamma_multiply(opacity.clamp(0.0, 1.0))
}

fn paint(ctx: &egui::Context, ms: u32, opacity: f32) {
    let theme = theme::shared();
    let logo = shared();
    let timeline = logo.timeline_ms;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("opening-splash"),
    ));
    let screen = ctx.content_rect();
    painter.rect_filled(screen, 0.0, faded(theme::color(theme.color.ink), opacity));
    let size = (screen.width().min(screen.height()) * 0.36).min(216.0);
    let centre = screen.center() - Vec2::new(0.0, size * 0.25);
    // A soft gold glow behind the mark.
    for step in 1..=8 {
        let radius = size * (0.45 + step as f32 * 0.16);
        painter.circle_filled(
            centre,
            radius,
            faded(theme::color(theme.color.gold), opacity * 0.012),
        );
    }
    let point = |[x, y]: [f32; 2]| centre + Vec2::new(x - 50.0, y - 50.0) * (size / 100.0);
    let colour = |name: &str| {
        faded(
            theme.color.named(name).map_or(Color32::WHITE, theme::color),
            opacity,
        )
    };
    for shape in &logo.shapes {
        let progress = timeline.progress(shape.phase(), ms);
        if progress <= 0.0 {
            continue;
        }
        match shape {
            logo::Shape::Circle {
                centre: middle,
                radius,
                width,
                color,
                ..
            } => {
                // Drawn on clockwise from the top, as the browser's rotated circle.
                let segments = 96;
                let drawn = (segments as f32 * progress).ceil() as usize;
                let points = (0..=drawn)
                    .map(|i| {
                        let angle = -std::f32::consts::FRAC_PI_2
                            + std::f32::consts::TAU * progress * i as f32 / drawn.max(1) as f32;
                        point([
                            middle[0] + radius * angle.cos(),
                            middle[1] + radius * angle.sin(),
                        ])
                    })
                    .collect();
                painter.add(PathShape::line(
                    points,
                    Stroke::new(width * size / 100.0, colour(color)),
                ));
            }
            logo::Shape::Line {
                points,
                width,
                color,
                ..
            }
            | logo::Shape::Outline {
                points,
                width,
                color,
                ..
            } => {
                let drawn: Vec<Pos2> = logo::partial(points, progress)
                    .into_iter()
                    .map(point)
                    .collect();
                if drawn.len() > 1 {
                    painter.add(PathShape::line(
                        drawn,
                        Stroke::new(width * size / 100.0, colour(color)),
                    ));
                }
            }
            logo::Shape::Fill { points, color, .. } => {
                // The gem pops: it overshoots, then settles, as the browser's.
                let middle = points.iter().fold([0.0, 0.0], |sum, p| {
                    [
                        sum[0] + p[0] / points.len() as f32,
                        sum[1] + p[1] / points.len() as f32,
                    ]
                });
                let pop = 0.3 + 0.7 * progress + 0.25 * (progress * std::f32::consts::PI).sin();
                let gem: Vec<Pos2> = points
                    .iter()
                    .map(|p| {
                        point([
                            middle[0] + (p[0] - middle[0]) * pop,
                            middle[1] + (p[1] - middle[1]) * pop,
                        ])
                    })
                    .collect();
                // A soft light around the gem: layered faint discs, densest at its heart.
                for ring in 1..=5 {
                    painter.circle_filled(
                        point(middle),
                        size * 0.022 * ring as f32 * pop,
                        faded(
                            theme::color(theme.color.gold_bright),
                            opacity * progress * 0.05,
                        ),
                    );
                }
                painter.add(Shape::convex_polygon(
                    gem,
                    faded(colour(color), progress),
                    Stroke::NONE,
                ));
            }
        }
    }
    let title = if ms == u32::MAX {
        1.0
    } else {
        timeline.title(ms)
    };
    if title > 0.0 {
        let rise = (1.0 - title) * 7.0;
        let spaced =
            |text: &str, gap: &str| text.chars().map(String::from).collect::<Vec<_>>().join(gap);
        painter.text(
            centre + Vec2::new(0.0, size * 0.62 + rise),
            Align2::CENTER_TOP,
            spaced(&logo.title, "  "),
            FontId::proportional(theme.text.display),
            faded(theme::color(theme.color.text_heading), opacity * title),
        );
        painter.text(
            centre + Vec2::new(0.0, size * 0.62 + theme.text.display + 14.0 + rise),
            Align2::CENTER_TOP,
            spaced(&logo.subtitle, " "),
            FontId::proportional(theme.text.small - 1.0),
            faded(theme::color(theme.color.text_muted), opacity * title),
        );
    }
}
