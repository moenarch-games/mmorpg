//! The circular minimap, as in the browser's `minimap.ts`: roads, water and
//! buildings from presentation scenery, projected units as dots and quest
//! markers from the received sheet, rotated so the camera heading points up.
//! Its rules (zoom radii, dot classes, the heading projection, the raid
//! disturbance and the landmark range) are `mmorpg_client_model::minimap`'s (#1280).
use egui::{Color32, Pos2, Shape, Stroke, Vec2, epaint::CircleShape};
use mmorpg_client_model::minimap::{self as rules, DotClass};
use mmorpg_core::{QuestMarker, QuestSheet, ZoneContent, ZoneSnapshot};
use mmorpg_scenery::Scenery;

pub use rules::RADII_METRES;
const UNITS_PER_METRE: f32 = 100.0;

/// Each dot class's colour, as the browser's `DOT_COLORS`.
const fn dot_color(class: DotClass) -> Color32 {
    match class {
        DotClass::Player => Color32::from_rgb(0x4e, 0xa8, 0xff),
        DotClass::Friendly => Color32::from_rgb(0x5c, 0xcf, 0x6c),
        DotClass::Hostile => Color32::from_rgb(0xe5, 0x48, 0x3d),
        DotClass::Neutral => Color32::from_rgb(0xf0, 0xcf, 0x4a),
        DotClass::Minion => Color32::from_rgb(0xd8, 0xd0, 0xb4),
        DotClass::Dead => Color32::from_rgb(0x6e, 0x6e, 0x6e),
    }
}

/// Static map layers in metres, built once from scenery.
pub struct MinimapLayers {
    roads: Vec<(Vec<[f32; 2]>, f32)>,
    water: Vec<([f32; 2], [f32; 2])>,
    buildings: Vec<[[f32; 2]; 4]>,
}

impl MinimapLayers {
    #[must_use]
    pub fn new(scenery: &Scenery) -> Self {
        let metres = |units: i32| units as f32 / UNITS_PER_METRE;
        let roads = scenery
            .roads
            .iter()
            .map(|road| {
                (
                    road.points
                        .iter()
                        .map(|p| [metres(p[0]), metres(p[1])])
                        .collect(),
                    metres(road.half_width) * 2.0,
                )
            })
            .collect();
        let water = scenery
            .water
            .iter()
            .map(|w| {
                (
                    [metres(w.centre[0]), metres(w.centre[1])],
                    [metres(w.radii[0]), metres(w.radii[1])],
                )
            })
            .collect();
        let buildings = scenery
            .props
            .iter()
            .filter(|prop| prop.kind.on_minimap())
            .map(|prop| {
                let [x, _, z] = prop.position.map(metres);
                let [hx, _, hz] = prop.half_extents.map(metres);
                let yaw = f32::from(prop.yaw) / 65_536.0 * std::f32::consts::TAU;
                let (sin, cos) = yaw.sin_cos();
                [[-hx, -hz], [hx, -hz], [hx, hz], [-hx, hz]]
                    .map(|[lx, lz]| [x + lx * cos + lz * sin, z - lx * sin + lz * cos])
            })
            .collect();
        Self {
            roads,
            water,
            buildings,
        }
    }

    /// Paints the map of `diameter` points: `forward` is the camera's XZ
    /// heading, `zoom` an index into [`RADII_METRES`].
    pub fn paint(
        &self,
        ui: &mut egui::Ui,
        diameter: f32,
        snapshot: &ZoneSnapshot,
        quests: Option<&QuestSheet>,
        content: &ZoneContent,
        forward: [f32; 2],
        zoom: usize,
    ) {
        let (response, painter) = ui.allocate_painter(Vec2::splat(diameter), egui::Sense::hover());
        let rect = response.rect;
        let centre = rect.center();
        let radius = diameter / 2.0;
        let range = RADII_METRES[zoom.min(RADII_METRES.len() - 1)];
        let scale = radius / range;
        let Some(viewer) = snapshot
            .entities
            .iter()
            .find(|e| rules::is_viewer(snapshot, e))
        else {
            return;
        };
        let origin = [
            viewer.position[0] as f32 / UNITS_PER_METRE,
            viewer.position[2] as f32 / UNITS_PER_METRE,
        ];
        let heading = rules::heading_of(forward);
        let to_offset =
            |p: [f32; 2]| rules::offset(p[0] - origin[0], p[1] - origin[1], heading, true, scale);
        let to_screen = |p: [f32; 2]| {
            let [x, y] = to_offset(p);
            centre + Vec2::new(x, y)
        };
        let on_rim = |p: [f32; 2], inset: f32| {
            let [x, y] = rules::clamp_to_rim(to_offset(p), radius - inset);
            centre + Vec2::new(x, y)
        };
        let inside = |p: Pos2| (p - centre).length() <= radius;
        painter.add(CircleShape::filled(
            centre,
            radius,
            Color32::from_rgb(74, 98, 58),
        ));
        for (centre_m, radii) in &self.water {
            let steps = 24;
            let points: Vec<Pos2> = (0..steps)
                .map(|i| {
                    let a = i as f32 / steps as f32 * std::f32::consts::TAU;
                    to_screen([
                        centre_m[0] + radii[0] * a.cos(),
                        centre_m[1] + radii[1] * a.sin(),
                    ])
                })
                .collect();
            if points.iter().any(|p| inside(*p)) {
                painter.add(Shape::convex_polygon(
                    points,
                    Color32::from_rgb(70, 120, 170),
                    Stroke::NONE,
                ));
            }
        }
        for (points, width) in &self.roads {
            let line: Vec<Pos2> = points.iter().map(|p| to_screen(*p)).collect();
            painter.add(Shape::line(
                line,
                Stroke::new((width * scale).max(1.5), Color32::from_rgb(170, 150, 110)),
            ));
        }
        for corners in &self.buildings {
            let points: Vec<Pos2> = corners.iter().map(|p| to_screen(*p)).collect();
            if points.iter().any(|p| inside(*p)) {
                painter.add(Shape::convex_polygon(
                    points,
                    Color32::from_rgb(120, 70, 55),
                    Stroke::NONE,
                ));
            }
        }
        // A night raid's approximate disturbance area while its assault runs (#530): a soft
        // circle, or a pip on the rim pointing at it while it is off the map.
        if let Some(area) = rules::disturbance(snapshot, content) {
            let colour = Color32::from_rgb(255, 122, 61);
            let at = to_screen([area.x, area.z]);
            let size = area.radius * scale;
            if (at - centre).length() - size < radius {
                painter.add(CircleShape {
                    center: at,
                    radius: size,
                    fill: Color32::from_rgba_unmultiplied(255, 122, 61, 56),
                    stroke: Stroke::new(1.5, colour),
                });
            } else {
                let pip = centre + (at - centre).normalized() * (radius - 7.0);
                painter.circle(pip, 4.5, colour, Stroke::new(1.0, Color32::BLACK));
            }
        }
        // The exploration map (#983): the active quests' approximate areas as dashed circles,
        // or a rim pip while off the map (Atlas 5.1.13), then nearby discovered landmarks.
        let exploration = mmorpg_client_model::exploration::exploration_map(
            content,
            &snapshot.viewer.discoveries,
            quests,
        );
        let gold = Color32::from_rgb(255, 211, 77);
        for hint in &exploration.hints {
            let at = to_screen(hint.center);
            let size = hint.radius * scale;
            if (at - centre).length() - size < radius {
                painter.add(CircleShape {
                    center: at,
                    radius: size,
                    fill: Color32::from_rgba_unmultiplied(255, 211, 77, 36),
                    stroke: Stroke::NONE,
                });
                let ring: Vec<Pos2> = (0..=48)
                    .map(|i| {
                        let a = i as f32 / 48.0 * std::f32::consts::TAU;
                        at + Vec2::new(a.cos(), a.sin()) * size
                    })
                    .collect();
                painter.extend(Shape::dashed_line(&ring, Stroke::new(1.5, gold), 5.0, 4.0));
            } else {
                let pip = centre + (at - centre).normalized() * (radius - 7.0);
                painter.circle(pip, 4.0, gold, Stroke::new(1.0, Color32::BLACK));
            }
        }
        for area in exploration
            .areas
            .iter()
            .filter(|area| area.discovered && rules::landmark_in_range(area.center(), origin))
        {
            let at = to_screen(area.center());
            if (at - centre).length() > radius - 6.0 {
                continue;
            }
            let s = 3.5;
            painter.add(Shape::convex_polygon(
                vec![
                    at + Vec2::new(0.0, -s),
                    at + Vec2::new(s, 0.0),
                    at + Vec2::new(0.0, s),
                    at + Vec2::new(-s, 0.0),
                ],
                Color32::from_rgb(243, 230, 184),
                Stroke::new(1.0, Color32::from_rgb(58, 42, 20)),
            ));
            if diameter >= 150.0 {
                painter.text(
                    at + Vec2::new(0.0, s + 1.0),
                    egui::Align2::CENTER_TOP,
                    &area.name,
                    egui::FontId::proportional(10.0),
                    Color32::from_rgb(243, 230, 184),
                );
            }
        }
        // Units beyond the rim sit on it, pointing at them.
        for entity in &snapshot.entities {
            if rules::is_viewer(snapshot, entity) {
                continue;
            }
            let at = on_rim(
                [
                    entity.position[0] as f32 / UNITS_PER_METRE,
                    entity.position[2] as f32 / UNITS_PER_METRE,
                ],
                5.0,
            );
            painter.circle(
                at,
                3.2,
                dot_color(rules::dot_class(entity)),
                Stroke::new(1.0, Color32::from_black_alpha(150)),
            );
        }
        if let Some(sheet) = quests {
            for marker in &sheet.markers {
                let Some(npc) = content.npc(marker.npc) else {
                    continue;
                };
                let at = on_rim(
                    [
                        npc.position[0] as f32 / UNITS_PER_METRE,
                        npc.position[1] as f32 / UNITS_PER_METRE,
                    ],
                    6.0,
                );
                let color = match marker.marker {
                    QuestMarker::Available | QuestMarker::Complete => {
                        Color32::from_rgb(255, 210, 0)
                    }
                    QuestMarker::InProgress => Color32::from_gray(170),
                };
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    mmorpg_client_model::quest::marker_glyph(marker.marker),
                    egui::FontId::proportional(15.0),
                    color,
                );
            }
        }
        // The player's arrow points along their facing relative to the camera.
        let facing = f32::from(viewer.facing) / 65_536.0 * std::f32::consts::TAU;
        let pointing = [facing.sin(), facing.cos()];
        let tip = to_screen([origin[0] + pointing[0], origin[1] + pointing[1]]) - centre;
        let dir = if tip.length() > 0.0 {
            tip.normalized()
        } else {
            Vec2::new(0.0, -1.0)
        };
        let side = Vec2::new(-dir.y, dir.x);
        painter.add(Shape::convex_polygon(
            vec![
                centre + dir * 8.0,
                centre - dir * 5.0 + side * 5.0,
                centre - dir * 5.0 - side * 5.0,
            ],
            Color32::WHITE,
            Stroke::new(1.0, Color32::BLACK),
        ));
        painter.circle_stroke(centre, radius, Stroke::new(2.0, Color32::from_gray(30)));
        // North (−Z) marked on the rim, as in the browser.
        let [nx, ny] =
            rules::clamp_to_rim(rules::offset(0.0, -1e6, heading, true, 1.0), radius - 9.0);
        painter.text(
            centre + Vec2::new(nx, ny),
            egui::Align2::CENTER_CENTER,
            "N",
            egui::FontId::proportional(11.0),
            Color32::from_rgb(243, 230, 184),
        );
    }
}
