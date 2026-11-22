//! Named places of zone content that clients mark on their maps (#1063): caves and
//! dungeons, and landmarks and ruins. Clients never invent one; waystones, services and
//! quest givers come from their own tables. Landmarks never affect gameplay.

/// What kind of place a landmark is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LandmarkKind {
    /// A cave, a mine or a bandits' hold.
    Dungeon,
    /// A tower, a ruin, a bridge or another sight.
    Sight,
}

/// One named place, at its XZ centre in units.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Landmark {
    pub name: String,
    pub kind: LandmarkKind,
    pub at: [i32; 2],
}
