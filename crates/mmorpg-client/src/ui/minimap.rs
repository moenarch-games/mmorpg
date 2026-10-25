//! The circular minimap, as in the browser's `minimap.ts`: roads, water and
//! buildings from presentation scenery, projected units as dots and quest
//! markers from the received sheet, rotated so the camera heading points up.
use egui::{Color32, Pos2, Shape, Stroke, Vec2, epaint::CircleShape};
use mmorpg_core::{EntityKind, QuestMarker, QuestSheet, ZoneContent, ZoneSnapshot};
use mmorpg_scenery::{PropKind, Scenery};

/// Zoom levels: metres from the centre to the rim, as in the browser.
pub const RADII_METRES: [f32; 4] = [30.0, 45.0, 70.0, 100.0];
const UNITS_PER_METRE: f32 = 100.0;

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
            .filter(|prop| {
                matches!(
                    prop.kind,
                    PropKind::Keep
                        | PropKind::Inn
                        | PropKind::House
                        | PropKind::Smithy
                        | PropKind::Barn
                        | PropKind::Farmhouse
                        | PropKind::Windmill
                        | PropKind::Well
                        | PropKind::PalisadeSegment
                        | PropKind::GatePost
                        | PropKind::MineEntrance
                        | PropKind::Waystone
                        | PropKind::Fence
                        | PropKind::Tent
                        | PropKind::Cliff
                )
            })
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
            .find(|e| e.kind == EntityKind::Player && e.id == snapshot.viewer_id)
        else {
            return;
        };
        let origin = [
            viewer.position[0] as f32 / UNITS_PER_METRE,
            viewer.position[2] as f32 / UNITS_PER_METRE,
        ];
        let length = forward[0].hypot(forward[1]).max(1e-3);
        let (fx, fz) = (forward[0] / length, forward[1] / length);
        // Screen right is the heading turned clockwise seen from above.
        let to_screen = |p: [f32; 2]| {
            let (dx, dz) = (p[0] - origin[0], p[1] - origin[1]);
            let right = dx * -fz + dz * fx;
            let up = dx * fx + dz * fz;
            centre + Vec2::new(right, -up) * scale
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
        for entity in &snapshot.entities {
            if entity.kind == EntityKind::Player && entity.id == snapshot.viewer_id {
                continue;
            }
            let at = to_screen([
                entity.position[0] as f32 / UNITS_PER_METRE,
                entity.position[2] as f32 / UNITS_PER_METRE,
            ]);
            if !inside(at) {
                continue;
            }
            let color = match entity.kind {
                _ if entity.flags.dead => Color32::from_gray(110),
                EntityKind::Player => Color32::from_rgb(90, 160, 255),
                EntityKind::Npc => Color32::from_rgb(240, 220, 120),
                EntityKind::Creature if entity.flags.hostile => Color32::from_rgb(230, 70, 60),
                EntityKind::Creature => Color32::from_rgb(235, 200, 60),
                // Summoned skeletons (#295) in bone white.
                EntityKind::Minion => Color32::from_rgb(216, 208, 180),
            };
            painter.circle_filled(at, 3.0, color);
        }
        if let Some(sheet) = quests {
            for marker in &sheet.markers {
                let Some(npc) = content.npc(marker.npc) else {
                    continue;
                };
                let at = to_screen([
                    npc.position[0] as f32 / UNITS_PER_METRE,
                    npc.position[1] as f32 / UNITS_PER_METRE,
                ]);
                let at = if inside(at) {
                    at
                } else {
                    centre + (at - centre).normalized() * (radius - 6.0)
                };
                let (text, color) = match marker.marker {
                    QuestMarker::Available => ("!", Color32::from_rgb(255, 210, 0)),
                    QuestMarker::Complete => ("?", Color32::from_rgb(255, 210, 0)),
                    QuestMarker::InProgress => ("?", Color32::from_gray(170)),
                };
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(15.0),
                    color,
                );
            }
        }
        // The player's arrow points along their facing relative to the camera.
        let facing = f32::from(viewer.facing) / 65_536.0 * std::f32::consts::TAU;
        let heading = [facing.sin(), facing.cos()];
        let tip = to_screen([origin[0] + heading[0], origin[1] + heading[1]]) - centre;
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
        painter.text(
            centre + Vec2::new(0.0, -radius + 8.0),
            egui::Align2::CENTER_CENTER,
            if fz.abs() >= fx.abs() {
                if fz < 0.0 { "N" } else { "S" }
            } else if fx > 0.0 {
                "E"
            } else {
                "W"
            },
            egui::FontId::proportional(11.0),
            Color32::from_gray(230),
        );
    }
}
