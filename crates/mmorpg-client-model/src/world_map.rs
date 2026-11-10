//! The world map both clients open with M: the continent atlas's committed map
//! (`content/world/export/world-map.json`, made by `atlas.py map-export`), where a
//! zone position lies on it, and what lies under a point.
//!
//! The atlas uses the zone's own frame: metres, x east, z south (−z north), with
//! Greyhaven Outpost at the origin. The map places that frame at 0° N 0° E in
//! WGS84 through inverse Web Mercator, where the scale is 1 to within two parts
//! per million, so a metre in the zone is a metre on the map.
use std::sync::OnceLock;

use serde_json::Value;

/// The map as both clients receive it: GeoJSON layers in WGS84.
pub const BUNDLE: &str = include_str!("../../../content/world/export/world-map.json");

const EARTH_RADIUS: f64 = 6_378_137.0; // the Web Mercator sphere

/// `[longitude, latitude]` in degrees.
pub type LonLat = [f64; 2];

/// Where zone position (`x` east, `z` south, in metres) lies on the map.
#[must_use]
pub fn zone_to_map(x: f64, z: f64) -> LonLat {
    [
        (x / EARTH_RADIUS).to_degrees(),
        (2.0 * (-z / EARTH_RADIUS).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    ]
}

/// The zone position (`x`, `z` in metres) at a map point.
#[must_use]
pub fn map_to_zone(point: LonLat) -> (f64, f64) {
    let [lon, lat] = point;
    (
        lon.to_radians() * EARTH_RADIUS,
        -EARTH_RADIUS
            * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
                .tan()
                .ln(),
    )
}

/// A facing in radians (the zone convention of `mmorpg-core`: 0 faces +z, south, and a
/// quarter turn faces +x, east) as a compass bearing on the map: degrees clockwise from north.
#[must_use]
pub fn bearing(yaw: f64) -> f64 {
    (180.0 - yaw.to_degrees()).rem_euclid(360.0)
}

/// A kingdom, or the unclaimed Crown.
#[derive(Clone, Debug, PartialEq)]
pub struct Kingdom {
    pub id: String,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

/// An area: a polygon's outer ring followed by its holes, each closed.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub rings: Vec<Vec<LonLat>>,
    min: LonLat,
    max: LonLat,
}

impl Area {
    fn new(rings: Vec<Vec<LonLat>>) -> Self {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for p in rings.iter().flatten() {
            for axis in 0..2 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        Self { rings, min, max }
    }

    /// Whether `point` lies inside the outer ring and outside every hole.
    #[must_use]
    pub fn contains(&self, point: LonLat) -> bool {
        if point[0] < self.min[0]
            || point[0] > self.max[0]
            || point[1] < self.min[1]
            || point[1] > self.max[1]
        {
            return false;
        }
        self.rings
            .iter()
            .filter(|ring| crosses(ring, point))
            .count()
            % 2
            == 1
    }
}

fn crosses(ring: &[LonLat], [x, y]: LonLat) -> bool {
    let mut inside = false;
    for pair in ring.windows(2) {
        let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) / (y1 - y0) * (x1 - x0) {
            inside = !inside;
        }
    }
    inside
}

/// A natural region: the unit of play and of kingdom claims.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub id: String,
    pub name: Option<String>,
    pub kingdom: String,
    pub biome: String,
    pub area: Area,
}

/// A named point: a settlement, a Crown gate or a landmark.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// Settlement kind (`capital`, `town`, `fort`, `ruin`, `outpost`), `gate`, or the
    /// landmark kind (`range`, `hills`, `massif`).
    pub kind: String,
    pub at: LonLat,
}

/// A named line: a river course or a road (which may have several legs).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub name: String,
    pub parts: Vec<Vec<LonLat>>,
}

/// A playable zone's outline on the map.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub area: Area,
}

/// What the map says about one point, for the hover label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionInfo {
    pub id: String,
    /// The region's name, or `None` for the many regions not yet named.
    pub name: Option<String>,
    pub kingdom: String,
    pub biome: String,
    /// The playable zone the point lies in, if any.
    pub zone: Option<String>,
}

/// The parsed map.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldMap {
    pub kingdoms: Vec<Kingdom>,
    pub regions: Vec<Region>,
    pub lakes: Vec<Area>,
    pub rivers: Vec<Line>,
    pub roads: Vec<Line>,
    pub settlements: Vec<Place>,
    pub gates: Vec<Place>,
    pub landmarks: Vec<Place>,
    pub zones: Vec<Zone>,
}

/// The committed map, parsed once.
///
/// # Panics
///
/// When the committed bundle is malformed; `atlas.py check` and this crate's tests guard it.
#[must_use]
pub fn world_map() -> &'static WorldMap {
    static MAP: OnceLock<WorldMap> = OnceLock::new();
    MAP.get_or_init(|| {
        WorldMap::parse(BUNDLE).expect("content/world/export/world-map.json is a valid world map")
    })
}

impl WorldMap {
    /// Parses a bundle written by `atlas.py map-export`.
    ///
    /// # Errors
    ///
    /// When the text is not JSON or a layer is missing or misshapen.
    pub fn parse(text: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let layer = |name: &str| -> Result<&Vec<Value>, String> {
            root["layers"][name]
                .as_array()
                .ok_or_else(|| format!("layer {name} is missing"))
        };
        let text_of = |f: &Value, key: &str| f["properties"][key].as_str().map(str::to_owned);
        let kingdoms = root["kingdoms"]
            .as_array()
            .ok_or("kingdoms are missing")?
            .iter()
            .map(|k| Kingdom {
                id: k["id"].as_str().unwrap_or_default().to_owned(),
                name: k["name"].as_str().unwrap_or_default().to_owned(),
                color: k["color"].as_str().unwrap_or("#888888").to_owned(),
            })
            .collect();
        let regions = layer("regions")?
            .iter()
            .map(|f| {
                Ok(Region {
                    id: text_of(f, "id").ok_or("a region has no id")?,
                    name: text_of(f, "name"),
                    kingdom: text_of(f, "kingdom").unwrap_or_default(),
                    biome: text_of(f, "biome").unwrap_or_default(),
                    area: polygon(&f["geometry"])?,
                })
            })
            .collect::<Result<_, String>>()?;
        let places = |name: &str, kind: Option<&str>| -> Result<Vec<Place>, String> {
            layer(name)?
                .iter()
                .map(|f| {
                    Ok(Place {
                        name: text_of(f, "name").unwrap_or_default(),
                        kind: kind
                            .map_or_else(|| text_of(f, "kind").unwrap_or_default(), str::to_owned),
                        at: point(&f["geometry"]["coordinates"])?,
                    })
                })
                .collect()
        };
        let lines = |name: &str| -> Result<Vec<Line>, String> {
            layer(name)?
                .iter()
                .map(|f| {
                    let g = &f["geometry"];
                    let parts = match g["type"].as_str() {
                        Some("LineString") => vec![path(&g["coordinates"])?],
                        Some("MultiLineString") => g["coordinates"]
                            .as_array()
                            .ok_or("bad lines")?
                            .iter()
                            .map(path)
                            .collect::<Result<_, _>>()?,
                        other => return Err(format!("{name}: unexpected {other:?}")),
                    };
                    Ok(Line {
                        name: text_of(f, "name").unwrap_or_default(),
                        parts,
                    })
                })
                .collect()
        };
        Ok(Self {
            kingdoms,
            regions,
            lakes: layer("lakes")?
                .iter()
                .map(|f| polygon(&f["geometry"]))
                .collect::<Result<_, _>>()?,
            rivers: lines("rivers")?,
            roads: lines("roads")?,
            settlements: places("settlements", None)?,
            gates: places("gates", Some("gate"))?,
            landmarks: places("landmarks", None)?,
            zones: layer("zones")?
                .iter()
                .map(|f| {
                    Ok(Zone {
                        id: text_of(f, "id").unwrap_or_default(),
                        name: text_of(f, "name").unwrap_or_default(),
                        area: polygon(&f["geometry"])?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }

    /// The kingdom with this id.
    #[must_use]
    pub fn kingdom(&self, id: &str) -> Option<&Kingdom> {
        self.kingdoms.iter().find(|k| k.id == id)
    }

    /// The region at a map point (land only), with its kingdom's name and the zone there.
    #[must_use]
    pub fn region_at(&self, point: LonLat) -> Option<RegionInfo> {
        let region = self.regions.iter().find(|r| r.area.contains(point))?;
        let zone = self
            .zones
            .iter()
            .find(|z| z.area.contains(point))
            .map(|z| z.name.clone());
        Some(self.info(region, zone))
    }

    /// The region with this id, for a client whose map reports the hovered feature; `zone`
    /// names a playable zone that overlaps the region.
    #[must_use]
    pub fn region(&self, id: &str) -> Option<RegionInfo> {
        let region = self.regions.iter().find(|r| r.id == id)?;
        let overlaps = |zone: &Zone| {
            zone.area
                .rings
                .iter()
                .flatten()
                .any(|p| region.area.contains(*p))
        };
        Some(
            self.info(
                region,
                self.zones
                    .iter()
                    .find(|z| overlaps(z))
                    .map(|z| z.name.clone()),
            ),
        )
    }

    fn info(&self, region: &Region, zone: Option<String>) -> RegionInfo {
        RegionInfo {
            id: region.id.clone(),
            name: region.name.clone(),
            kingdom: self
                .kingdom(&region.kingdom)
                .map_or_else(|| region.kingdom.clone(), |k| k.name.clone()),
            biome: region.biome.clone(),
            zone,
        }
    }

    /// The south-west and north-east corners of all land.
    #[must_use]
    pub fn bounds(&self) -> (LonLat, LonLat) {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for r in &self.regions {
            for axis in 0..2 {
                min[axis] = min[axis].min(r.area.min[axis]);
                max[axis] = max[axis].max(r.area.max[axis]);
            }
        }
        (min, max)
    }
}

fn point(v: &Value) -> Result<LonLat, String> {
    match v.as_array().map(Vec::as_slice) {
        Some([lon, lat, ..]) => Ok([
            lon.as_f64().ok_or("bad longitude")?,
            lat.as_f64().ok_or("bad latitude")?,
        ]),
        _ => Err("bad point".to_owned()),
    }
}

fn path(v: &Value) -> Result<Vec<LonLat>, String> {
    v.as_array().ok_or("bad line")?.iter().map(point).collect()
}

fn polygon(g: &Value) -> Result<Area, String> {
    if g["type"].as_str() != Some("Polygon") {
        return Err("expected a Polygon".to_owned());
    }
    Ok(Area::new(
        g["coordinates"]
            .as_array()
            .ok_or("bad polygon")?
            .iter()
            .map(path)
            .collect::<Result<_, _>>()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_map_parses_with_every_layer() {
        let map = world_map();
        assert!(map.regions.len() > 120, "{} regions", map.regions.len());
        assert_eq!(
            map.kingdoms.len(),
            6,
            "five kingdoms and the unclaimed Crown"
        );
        assert!(
            map.regions
                .iter()
                .all(|r| map.kingdom(&r.kingdom).is_some()),
            "every region names a known kingdom"
        );
        assert!(
            !map.rivers.is_empty()
                && !map.roads.is_empty()
                && !map.settlements.is_empty()
                && !map.lakes.is_empty()
        );
        assert_eq!(map.zones.len(), 1);
    }

    #[test]
    fn a_zone_metre_is_a_map_metre_and_round_trips() {
        for (x, z) in [(0.0, 0.0), (632.0, -208.0), (-11_000.0, 9_000.0)] {
            let (bx, bz) = map_to_zone(zone_to_map(x, z));
            assert!(
                (bx - x).abs() < 1e-6 && (bz - z).abs() < 1e-6,
                "{x},{z} came back as {bx},{bz}"
            );
        }
        let [_, north] = zone_to_map(0.0, -1000.0);
        assert!(north > 0.0, "−z is north");
    }

    #[test]
    fn the_outpost_lies_in_greyhaven_vale_in_carrowmark_and_the_sea_in_no_region() {
        let map = world_map();
        let here = map
            .region_at(zone_to_map(0.0, 20.0))
            .expect("the outpost is on land");
        assert_eq!(here.id, "greyhaven");
        assert_eq!(here.name.as_deref(), Some("Greyhaven Vale"));
        assert_eq!(here.kingdom, "Carrowmark");
        assert_eq!(here.zone.as_deref(), Some("Greyhaven Vale"));
        assert_eq!(map.region_at(zone_to_map(11_900.0, 11_900.0)), None);
        let crown = map
            .region_at(zone_to_map(-700.0, -3800.0))
            .expect("Shardfall is on land");
        assert_eq!(crown.kingdom, "The Shattered Crown");
        assert_eq!(crown.zone, None);
    }

    #[test]
    fn a_facing_becomes_a_compass_bearing() {
        use std::f64::consts::{FRAC_PI_2, PI};
        assert!((bearing(0.0) - 180.0).abs() < 1e-9, "yaw 0 faces +z, south");
        assert!(
            (bearing(FRAC_PI_2) - 90.0).abs() < 1e-9,
            "a quarter turn faces +x, east"
        );
        assert!(bearing(PI).abs() < 1e-9, "half a turn faces -z, north");
        assert!((bearing(3.0 * FRAC_PI_2) - 270.0).abs() < 1e-9, "west");
    }
}
