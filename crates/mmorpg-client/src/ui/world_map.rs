//! The world map (M), as in the browser's `world-map.ts`: the continent atlas from
//! `mmorpg_client_model::world_map`, drawn with the Maps library's Rust core. Its Web
//! Mercator `MapCamera` projects and unprojects every point, its MapLibre line tessellation
//! (`LineMesh`) draws rivers, roads and borders with proper joins, and region fills are
//! triangulated once with earcut, as Maps does. egui only receives the triangles.
use earcut::Earcut;
use egui::{Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2, epaint::Vertex};
use maps_core::{
    LineLayout, LineMesh, MapCamera, ScreenCoordinate, ViewportSize, project_web_mercator,
};
use mmorpg_client_model::world_map::{self as model, LonLat, WorldMap};
use mmorpg_core::{EntityKind, ZoneSnapshot};

const SEA: Color32 = Color32::from_rgb(47, 93, 124);
const LAKE: Color32 = Color32::from_rgb(79, 134, 168);
const OPEN_ZOOM: f64 = 13.2;
const MIN_ZOOM: f64 = 10.0;
const MAX_ZOOM: f64 = 17.0;
const UNITS_PER_METRE: f64 = 100.0;

/// A filled area triangulated in normalized Web Mercator coordinates, ready to scale to the screen.
struct Fill {
    world: Vec<[f64; 2]>,
    triangles: Vec<u32>,
    color: Color32,
}

/// The open map's camera and its prepared fills; built when the map opens.
pub struct WorldMapWindow {
    center: LonLat,
    zoom: f64,
    fills: Vec<Fill>,
    centred: bool,
}

fn rgb(hex: &str) -> Color32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x88_88_88);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn triangulate(rings: &[Vec<LonLat>], color: Color32) -> Option<Fill> {
    let mut world = Vec::new();
    let mut holes = Vec::new();
    for (n, ring) in rings.iter().enumerate() {
        if n > 0 {
            holes.push(world.len() as u32);
        }
        // earcut wants open rings
        let open = if ring.first() == ring.last() {
            &ring[..ring.len() - 1]
        } else {
            &ring[..]
        };
        for &[lon, lat] in open {
            let w = project_web_mercator(lon, lat)?;
            world.push([w.x, w.y]);
        }
    }
    let mut triangles = Vec::new();
    Earcut::new().earcut(world.iter().copied(), &holes, &mut triangles);
    Some(Fill {
        world,
        triangles,
        color,
    })
}

impl WorldMapWindow {
    /// Prepares the map; it centres on the viewer on its first frame.
    #[must_use]
    pub fn new() -> Self {
        let map = model::world_map();
        let mut fills: Vec<Fill> = map
            .regions
            .iter()
            .filter_map(|r| {
                let color = map
                    .kingdom(&r.kingdom)
                    .map_or(Color32::GRAY, |k| rgb(&k.color));
                triangulate(&r.area.rings, color.gamma_multiply(0.9))
            })
            .collect();
        fills.extend(map.lakes.iter().filter_map(|l| triangulate(&l.rings, LAKE)));
        fills.extend(map.zones.iter().filter_map(|z| {
            triangulate(
                &z.area.rings,
                Color32::from_rgba_unmultiplied(210, 60, 45, 30),
            )
        }));
        Self {
            center: [0.0, 0.0],
            zoom: OPEN_ZOOM,
            fills,
            centred: false,
        }
    }

    fn camera(&self, rect: Rect) -> Option<MapCamera> {
        let viewport = ViewportSize::new(f64::from(rect.width()), f64::from(rect.height()))?;
        MapCamera::new(
            self.center[0],
            self.center[1],
            self.zoom,
            0.0,
            0.0,
            viewport,
        )
    }

    /// Draws the map window; returns `false` once the player closes it.
    pub fn show(&mut self, ctx: &egui::Context, snapshot: Option<&ZoneSnapshot>) -> bool {
        let mut open = true;
        if !self.centred {
            if let Some(at) = viewer_point(snapshot) {
                self.center = at;
            }
            self.centred = true;
        }
        let screen = ctx.content_rect();
        let size = (screen.size() * 0.88).max(Vec2::new(320.0, 240.0));
        egui::Window::new("World map")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.small("M or Esc to close · drag to pan · wheel to zoom");
                    if ui.button("Centre on me").clicked()
                        && let Some(at) = viewer(snapshot).map(|(at, _)| at)
                    {
                        self.center = at;
                    }
                });
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                self.paint(ui, &response, &painter, snapshot);
            });
        open
    }

    fn paint(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        snapshot: Option<&ZoneSnapshot>,
    ) {
        let rect = response.rect;
        let Some(camera) = self.camera(rect) else {
            return;
        };
        // pan by dragging; zoom about the pointer with the wheel
        if response.dragged() {
            let d = response.drag_delta();
            let mid = ScreenCoordinate {
                x: f64::from(rect.width()) / 2.0,
                y: f64::from(rect.height()) / 2.0,
            };
            let moved = ScreenCoordinate {
                x: mid.x - f64::from(d.x),
                y: mid.y - f64::from(d.y),
            };
            if let Some(at) = camera.unproject_screen(moved) {
                self.center = [at.longitude, at.latitude];
            }
        }
        let scroll = if response.hovered() {
            ui.input(|i| i.smooth_scroll_delta.y)
        } else {
            0.0
        };
        if scroll != 0.0 {
            let pointer = response.hover_pos().unwrap_or(rect.center()) - rect.min;
            let local = ScreenCoordinate {
                x: f64::from(pointer.x),
                y: f64::from(pointer.y),
            };
            let before = camera.unproject_screen(local);
            self.zoom = (self.zoom + f64::from(scroll) / 240.0).clamp(MIN_ZOOM, MAX_ZOOM);
            if let (Some(before), Some(after)) = (
                before,
                self.camera(rect).and_then(|c| c.unproject_screen(local)),
            ) {
                self.center = [
                    self.center[0] + before.longitude - after.longitude,
                    self.center[1] + before.latitude - after.latitude,
                ];
            }
        }
        let Some(camera) = self.camera(rect) else {
            return;
        };
        let Some(size) = camera.world_size() else {
            return;
        };
        let Some(centre) = project_web_mercator(camera.longitude, camera.latitude) else {
            return;
        };
        let to_screen = |w: [f64; 2]| {
            Pos2::new(
                rect.min.x + (f64::from(rect.width()) / 2.0 + (w[0] - centre.x) * size) as f32,
                rect.min.y + (f64::from(rect.height()) / 2.0 + (w[1] - centre.y) * size) as f32,
            )
        };
        let project = |p: LonLat| {
            camera
                .project_screen(p[0], p[1])
                .map(|s| Pos2::new(rect.min.x + s.x as f32, rect.min.y + s.y as f32))
        };

        let painter = painter.with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, SEA);
        let mut mesh = Mesh::default();
        for fill in &self.fills {
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend(fill.world.iter().map(|&w| Vertex {
                pos: to_screen(w),
                uv: egui::epaint::WHITE_UV,
                color: fill.color,
            }));
            mesh.indices.extend(fill.triangles.iter().map(|i| base + i));
        }
        painter.add(Shape::mesh(mesh));

        let map = model::world_map();
        let outline = |pts: &[LonLat]| pts.iter().filter_map(|&p| project(p)).collect::<Vec<_>>();
        let mut lines = Mesh::default();
        for region in &map.regions {
            for ring in &region.area.rings {
                stroke(
                    &mut lines,
                    &outline(ring),
                    1.0,
                    Color32::from_rgba_unmultiplied(59, 43, 38, 150),
                    true,
                );
            }
        }
        for river in &map.rivers {
            for part in &river.parts {
                stroke(
                    &mut lines,
                    &outline(part),
                    2.0,
                    Color32::from_rgb(52, 98, 160),
                    false,
                );
            }
        }
        for road in &map.roads {
            for part in &road.parts {
                stroke(
                    &mut lines,
                    &outline(part),
                    1.6,
                    Color32::from_rgb(122, 74, 42),
                    false,
                );
            }
        }
        for zone in &map.zones {
            for ring in &zone.area.rings {
                stroke(
                    &mut lines,
                    &outline(ring),
                    2.5,
                    Color32::from_rgb(210, 60, 45),
                    true,
                );
            }
        }
        painter.add(Shape::mesh(lines));

        let label = |at: Pos2, text: &str, size: f32, color: Color32| {
            painter.text(
                at + Vec2::new(1.0, 1.0),
                Align2::CENTER_BOTTOM,
                text,
                FontId::proportional(size),
                Color32::from_black_alpha(160),
            );
            painter.text(
                at,
                Align2::CENTER_BOTTOM,
                text,
                FontId::proportional(size),
                color,
            );
        };
        for place in map.settlements.iter().chain(&map.gates) {
            let Some(at) = project(place.at) else {
                continue;
            };
            let capital = place.kind == "capital";
            painter.circle(
                at,
                if capital { 5.5 } else { 3.5 },
                if capital {
                    Color32::from_rgb(140, 30, 30)
                } else {
                    Color32::from_rgb(42, 30, 30)
                },
                Stroke::new(1.5, Color32::from_rgb(250, 246, 236)),
            );
            if self.zoom >= 11.5 || capital {
                label(
                    at - Vec2::new(0.0, 6.0),
                    &place.name,
                    if capital { 15.0 } else { 12.5 },
                    Color32::from_rgb(250, 246, 236),
                );
            }
        }
        if self.zoom >= 12.5 {
            for mark in &map.landmarks {
                if let Some(at) = project(mark.at) {
                    label(at, &mark.name, 12.0, Color32::from_rgb(235, 220, 190));
                }
            }
        }

        // you: a dot and an arrow along your facing (bearing clockwise from north; screen y points south)
        if let Some((at, bearing)) = viewer(snapshot)
            && let Some(p) = project(at)
        {
            let b = bearing.to_radians() as f32;
            let dir = Vec2::new(b.sin(), -b.cos());
            let side = Vec2::new(-dir.y, dir.x);
            let tip = p + dir * 14.0;
            let arrow = vec![
                tip,
                p - dir * 6.0 + side * 7.0,
                p - dir * 3.0,
                p - dir * 6.0 - side * 7.0,
            ];
            painter.add(Shape::convex_polygon(
                arrow,
                Color32::from_rgb(255, 211, 77),
                Stroke::new(1.5, Color32::from_rgb(29, 26, 20)),
            ));
        }

        // the region under the pointer, from the same rule the browser uses
        if let Some(pointer) = response.hover_pos() {
            let local = pointer - rect.min;
            if let Some(at) = camera.unproject_screen(ScreenCoordinate {
                x: f64::from(local.x),
                y: f64::from(local.y),
            }) && let Some(region) = map.region_at([at.longitude, at.latitude])
            {
                let mut text = format!(
                    "{}\n{}\n{}",
                    region
                        .name
                        .as_deref()
                        .map_or_else(|| format!("Unnamed region ({})", region.id), str::to_owned),
                    region.kingdom,
                    region.biome
                );
                if let Some(zone) = &region.zone {
                    text.push_str(&format!("\n{zone}: playable zone"));
                }
                let galley = painter.layout_no_wrap(
                    text,
                    FontId::proportional(13.0),
                    Color32::from_rgb(30, 26, 22),
                );
                let tip = Rect::from_min_size(
                    pointer + Vec2::new(14.0, 14.0),
                    galley.size() + Vec2::splat(12.0),
                );
                painter.rect_filled(tip, 5.0, Color32::from_rgb(250, 246, 236));
                painter.galley(
                    tip.min + Vec2::splat(6.0),
                    galley,
                    Color32::from_rgb(30, 26, 22),
                );
            }
        }
        legend(&painter, rect, map);
    }
}

/// A line through screen points, tessellated by the Maps library's MapLibre line bucket with
/// round joins and caps, `width` px wide.
fn stroke(mesh: &mut Mesh, points: &[Pos2], width: f32, color: Color32, closed: bool) {
    if points.len() < 2 {
        return;
    }
    let path: Vec<[f64; 2]> = points
        .iter()
        .map(|p| [f64::from(p.x), f64::from(p.y)])
        .collect();
    // screen px at about the tile zoom: 16 MapLibre tile units each, as in maps-wasm
    let mut line = LineMesh::with_units(16.0, 1.0);
    line.add_line(&path, &LineLayout::ROUND, closed);
    let half = f64::from(width) / 2.0;
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend(line.vertices.iter().map(|v| Vertex {
        pos: Pos2::new(
            (v.point[0] + v.extrude[0] * half) as f32,
            (v.point[1] + v.extrude[1] * half) as f32,
        ),
        uv: egui::epaint::WHITE_UV,
        color,
    }));
    mesh.indices.extend(line.indices.iter().map(|i| base + i));
}

fn legend(painter: &egui::Painter, rect: Rect, map: &WorldMap) {
    let row = 17.0;
    let size = Vec2::new(190.0, row * map.kingdoms.len() as f32 + 10.0);
    let area = Rect::from_min_size(
        Pos2::new(rect.min.x + 10.0, rect.max.y - size.y - 10.0),
        size,
    );
    painter.rect_filled(area, 6.0, Color32::from_rgba_unmultiplied(12, 18, 21, 220));
    for (n, kingdom) in map.kingdoms.iter().enumerate() {
        let y = area.min.y + 5.0 + row * n as f32;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(area.min.x + 8.0, y + 3.0), Vec2::splat(11.0)),
            2.0,
            rgb(&kingdom.color),
        );
        painter.text(
            Pos2::new(area.min.x + 26.0, y + 8.5),
            Align2::LEFT_CENTER,
            &kingdom.name,
            FontId::proportional(12.5),
            Color32::from_rgb(237, 244, 238),
        );
    }
}

/// The viewer's map point and compass bearing.
fn viewer(snapshot: Option<&ZoneSnapshot>) -> Option<(LonLat, f64)> {
    let snapshot = snapshot?;
    let me = snapshot
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Player && e.id == snapshot.viewer_id)?;
    let at = model::zone_to_map(
        f64::from(me.position[0]) / UNITS_PER_METRE,
        f64::from(me.position[2]) / UNITS_PER_METRE,
    );
    let yaw = f64::from(me.facing) / 65_536.0 * std::f64::consts::TAU;
    Some((at, model::bearing(yaw)))
}

/// Where to open the map: on the viewer.
#[must_use]
pub fn viewer_point(snapshot: Option<&ZoneSnapshot>) -> Option<LonLat> {
    viewer(snapshot).map(|(at, _)| at)
}

impl Default for WorldMapWindow {
    fn default() -> Self {
        Self::new()
    }
}
