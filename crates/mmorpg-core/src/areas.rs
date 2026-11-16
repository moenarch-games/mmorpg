//! Named areas of zone content. Clients read area names from here instead of
//! inventing their own, and later "explore" objectives decide membership with
//! the same table. Areas never affect movement or collision. Each content
//! revision publishes its table, such as `greyhaven_vale::areas()`.

use crate::ZoneError;

/// Longest area name in bytes.
pub const MAX_AREA_NAME_BYTES: usize = 64;

/// Stable identifier of a named area within one content revision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AreaId(u32);

impl AreaId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A named XZ rectangle in simulation units. Bounds are inclusive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Area {
    id: AreaId,
    name: String,
    min_xz: [i32; 2],
    max_xz: [i32; 2],
}

impl Area {
    pub fn new(
        id: AreaId,
        name: impl Into<String>,
        min_xz: [i32; 2],
        max_xz: [i32; 2],
    ) -> Result<Self, ZoneError> {
        let name = name.into();
        if name.trim().is_empty() || name.len() > MAX_AREA_NAME_BYTES {
            return Err(ZoneError::new("area name must be 1..=64 bytes of text"));
        }
        if min_xz[0] > max_xz[0] || min_xz[1] > max_xz[1] {
            return Err(ZoneError::new("area bounds must not be inverted"));
        }
        Ok(Self {
            id,
            name,
            min_xz,
            max_xz,
        })
    }

    #[must_use]
    pub const fn id(&self) -> AreaId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn min_xz(&self) -> [i32; 2] {
        self.min_xz
    }

    #[must_use]
    pub const fn max_xz(&self) -> [i32; 2] {
        self.max_xz
    }

    #[must_use]
    pub const fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.min_xz[0] && x <= self.max_xz[0] && z >= self.min_xz[1] && z <= self.max_xz[1]
    }
}

/// The ordered area table of one content revision.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneAreas {
    areas: Vec<Area>,
}

impl ZoneAreas {
    /// Validates unique IDs and names and orders the table by ID.
    pub fn new(mut areas: Vec<Area>) -> Result<Self, ZoneError> {
        areas.sort_by_key(Area::id);
        if areas.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ZoneError::new("duplicate area id"));
        }
        let mut names = areas.iter().map(Area::name).collect::<Vec<_>>();
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ZoneError::new("duplicate area name"));
        }
        Ok(Self { areas })
    }

    #[must_use]
    pub fn areas(&self) -> &[Area] {
        &self.areas
    }

    /// The area containing `(x, z)`. Where areas overlap, the lowest ID wins.
    #[must_use]
    pub fn area_at(&self, x: i32, z: i32) -> Option<&Area> {
        self.areas.iter().find(|area| area.contains(x, z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(id: u32, name: &str, min_xz: [i32; 2], max_xz: [i32; 2]) -> Area {
        Area::new(AreaId::new(id), name, min_xz, max_xz).unwrap()
    }

    #[test]
    fn lookup_is_inclusive_and_prefers_the_lowest_id() {
        let areas = ZoneAreas::new(vec![
            area(2, "Inner", [0, 0], [10, 10]),
            area(1, "Outer", [-5, -5], [5, 5]),
        ])
        .unwrap();
        assert_eq!(areas.areas()[0].name(), "Outer", "ordered by id");
        assert_eq!(areas.area_at(5, 5).map(Area::name), Some("Outer"));
        assert_eq!(areas.area_at(6, 10).map(Area::name), Some("Inner"));
        assert_eq!(areas.area_at(11, 0), None);
        assert_eq!(areas.area_at(-6, 0), None);
    }

    #[test]
    fn invalid_areas_fail_closed() {
        assert!(Area::new(AreaId::new(1), " ", [0, 0], [1, 1]).is_err());
        assert!(Area::new(AreaId::new(1), "x".repeat(65), [0, 0], [1, 1]).is_err());
        assert!(Area::new(AreaId::new(1), "Inverted", [2, 0], [1, 1]).is_err());
        assert!(
            ZoneAreas::new(vec![
                area(1, "A", [0, 0], [1, 1]),
                area(1, "B", [0, 0], [1, 1])
            ])
            .is_err()
        );
        assert!(
            ZoneAreas::new(vec![
                area(1, "A", [0, 0], [1, 1]),
                area(2, "A", [0, 0], [1, 1])
            ])
            .is_err()
        );
    }
}
