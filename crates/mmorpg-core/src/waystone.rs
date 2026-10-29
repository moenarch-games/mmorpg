//! Exploration-unlocked waystones (#145, Atlas 5.1.9).
//!
//! The waystone network is one global catalog, [`WAYSTONES`]: every stone has
//! a stable [`WaystoneId`], a name, its logical zone and where a traveller
//! arrives. A zone's content binds the stones that stand in it
//! ([`crate::ZoneContent::with_waystones`]).
//!
//! A living player who comes within [`WAYSTONE_DISCOVERY_UNITS`] of a bound
//! stone unlocks it for good: the unlocks are a durable part of the character
//! ([`WaystoneSet`] in [`crate::CharacterRecord`]). Standing within the same
//! reach of a stone, a player may travel to any unlocked stone
//! ([`crate::ZoneCommand::UseWaystone`]) at once, with no cast or cooldown.
//! Travel is refused while in combat ([`crate::ErrorCode::InCombat`]), in a
//! handoff corridor ([`crate::ErrorCode::HandoffCorridor`]) and toward a
//! locked, unknown or current stone ([`crate::ErrorCode::InvalidWaystone`]).
//!
//! A stone in this zone moves the player there. A stone in another logical
//! zone uses the same fenced transfer as walking through a seam
//! ([`crate::CharacterTransfer`]); until a host drives that transfer, the
//! zone refuses such travel with [`crate::ErrorCode::InvalidWaystone`].

use crate::{
    CharacterLocation, ErrorCode, PLAYER_HALF_EXTENTS_UNITS, PlayerId, ZoneError, ZoneId,
    ZoneSimulation,
};

/// How close a player's body centre must come to a stone, in XZ units (8 m), to
/// unlock it or travel from it.
pub const WAYSTONE_DISCOVERY_UNITS: i64 = 800;

/// Upper bound on stones in the network: one bit each of a [`WaystoneSet`]; a player
/// projection carries them in six spare flag bits.
pub const MAX_WAYSTONES: usize = 6;

/// A stone of the global network, `1..=MAX_WAYSTONES`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WaystoneId(u8);

impl WaystoneId {
    #[must_use]
    pub const fn new(value: u8) -> Option<Self> {
        if value == 0 || value as usize > MAX_WAYSTONES {
            None
        } else {
            Some(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    const fn bit(self) -> u32 {
        1 << (self.0 - 1)
    }
}

/// The stones a character has unlocked, one bit per [`WaystoneId`].
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct WaystoneSet(u32);

impl WaystoneSet {
    pub const EMPTY: Self = Self(0);

    /// The set of these bits; `None` when one names no stone of [`WAYSTONES`].
    #[must_use]
    pub fn from_bits(bits: u32) -> Option<Self> {
        let known = WAYSTONES
            .iter()
            .fold(0, |known, site| known | site.id.bit());
        (bits & !known == 0).then_some(Self(bits))
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, id: WaystoneId) -> bool {
        self.0 & id.bit() != 0
    }

    #[must_use]
    pub const fn with(self, id: WaystoneId) -> Self {
        Self(self.0 | id.bit())
    }

    /// Unlocked stones in ID order.
    pub fn iter(self) -> impl Iterator<Item = WaystoneId> {
        (1..=MAX_WAYSTONES as u8)
            .filter_map(WaystoneId::new)
            .filter(move |id| self.contains(*id))
    }
}

/// One stone of the network.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaystoneSite {
    pub id: WaystoneId,
    pub name: &'static str,
    /// The logical zone the stone stands in.
    pub zone: ZoneId,
    /// The stone's XZ centre in its zone, units.
    pub stone: [i32; 2],
    /// Where a traveller's feet arrive (XZ, units) and the facing on arrival.
    pub arrival: [i32; 2],
    pub facing: u16,
}

const fn id(value: u8) -> WaystoneId {
    match WaystoneId::new(value) {
        Some(id) => id,
        None => panic!("waystone IDs are 1..=MAX_WAYSTONES"),
    }
}

/// The Greyhaven Outpost stone, beside the hub's well.
pub const GREYHAVEN_OUTPOST: WaystoneId = id(1);
/// The stone of the Seam Test West zone (#145).
pub const SEAM_TEST_WEST: WaystoneId = id(2);
/// The stone of the Seam Test East zone (#145).
pub const SEAM_TEST_EAST: WaystoneId = id(3);

/// The logical zone Greyhaven Vale runs as in the waystone network.
pub const GREYHAVEN_ZONE: ZoneId = ZoneId::new(1);

/// The whole network, in ID order.
pub const WAYSTONES: [WaystoneSite; 3] = [
    WaystoneSite {
        id: GREYHAVEN_OUTPOST,
        name: "Greyhaven Outpost",
        zone: GREYHAVEN_ZONE,
        stone: [-550, 3_050],
        arrival: [-550, 2_800],
        facing: 0,
    },
    WaystoneSite {
        id: SEAM_TEST_WEST,
        name: "Seam Test West",
        zone: crate::seam_test::WEST_ZONE,
        stone: crate::seam_test::STONE,
        arrival: crate::seam_test::ARRIVAL,
        facing: 0,
    },
    WaystoneSite {
        id: SEAM_TEST_EAST,
        name: "Seam Test East",
        zone: crate::seam_test::EAST_ZONE,
        stone: crate::seam_test::STONE,
        arrival: crate::seam_test::ARRIVAL,
        facing: 0,
    },
];

/// The network's stone of `id`.
#[must_use]
pub fn waystone(id: WaystoneId) -> Option<&'static WaystoneSite> {
    WAYSTONES.iter().find(|site| site.id == id)
}

fn xz_distance_squared(a: [i32; 2], b: [i32; 2]) -> i64 {
    let dx = i64::from(a[0]) - i64::from(b[0]);
    let dz = i64::from(a[1]) - i64::from(b[1]);
    dx * dx + dz * dz
}

/// The stone bound in `content` within reach of the XZ point `at`, if any. Clients use it
/// to offer the waystone panel; the zone decides.
#[must_use]
pub fn waystone_within_reach(content: &crate::ZoneContent, at: [i32; 2]) -> Option<WaystoneId> {
    let reach = WAYSTONE_DISCOVERY_UNITS * WAYSTONE_DISCOVERY_UNITS;
    content
        .waystones()
        .iter()
        .copied()
        .find(|&id| waystone(id).is_some_and(|site| xz_distance_squared(site.stone, at) <= reach))
}

impl ZoneSimulation {
    /// The player's unlocked stones.
    pub fn unlocked_waystones(&self, player_id: PlayerId) -> Result<WaystoneSet, ZoneError> {
        self.players
            .get(&player_id)
            .map(|player| player.waystones)
            .ok_or_else(|| ZoneError::new("unknown player"))
    }

    /// The bound stone within reach of the player's body centre, if any.
    fn waystone_in_reach(&self, player_id: PlayerId) -> Option<WaystoneId> {
        let at = self.player_position(player_id).ok()?;
        waystone_within_reach(&self.content, [at.x, at.z])
    }

    /// Tick step: living players unlock the bound stones they reach.
    pub(crate) fn discover_waystones(&mut self) {
        if self.content.waystones().is_empty() {
            return;
        }
        let found: Vec<_> = self
            .players
            .iter()
            .filter(|(_, player)| player.is_alive())
            .filter_map(|(&player_id, _)| Some((player_id, self.waystone_in_reach(player_id)?)))
            .collect();
        for (player_id, id) in found {
            if let Some(player) = self.players.get_mut(&player_id) {
                player.waystones = player.waystones.with(id);
            }
        }
    }

    /// Resolves a [`crate::ZoneCommand::UseWaystone`] intent: the refusal, if any.
    pub(crate) fn use_waystone(
        &mut self,
        player_id: PlayerId,
        target: WaystoneId,
    ) -> Result<Option<(ErrorCode, Option<crate::EntityRef>)>, ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(None);
        };
        if !player.is_alive() {
            return Ok(Some((ErrorCode::YouAreDead, None)));
        }
        if self.in_handoff_corridor(player_id) {
            return Ok(Some((ErrorCode::HandoffCorridor, None)));
        }
        if player.combat_timer > 0 || self.threatened_players().contains(&player_id) {
            return Ok(Some((ErrorCode::InCombat, None)));
        }
        let unlocked = player.waystones;
        let Some(origin) = self.waystone_in_reach(player_id) else {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        };
        let Some(site) = waystone(target) else {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        };
        if origin == target || !unlocked.contains(target) {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        }
        if !self.content.waystones().contains(&target) {
            // Another logical zone: the fenced transfer carries the player once a
            // host drives it (#145 part 3b).
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        }
        let destination = self.waystone_arrival(site);
        self.relocate_player(player_id, destination)?;
        Ok(None)
    }

    /// Where a traveller to `site` stands, body centre on the ground, in this
    /// zone's coordinates when the stone is bound here.
    #[must_use]
    pub fn waystone_arrival(&self, site: &WaystoneSite) -> CharacterLocation {
        let ground = if self.content.waystones().contains(&site.id) {
            self.definition()
                .ground_at(site.arrival[0], site.arrival[1])
        } else {
            0
        };
        CharacterLocation {
            zone_id: site.zone,
            position: [
                site.arrival[0],
                ground + PLAYER_HALF_EXTENTS_UNITS[1],
                site.arrival[1],
            ],
            facing: site.facing,
        }
    }

    /// Moves a player within this zone: body, facing and interest, with the
    /// selection, any cast and held movement ended.
    fn relocate_player(
        &mut self,
        player_id: PlayerId,
        destination: CharacterLocation,
    ) -> Result<(), ZoneError> {
        use crate::entity::body_id;
        use crate::physics::Vec3i;
        let body = body_id(crate::EntityRef::Player(player_id));
        let position = Vec3i::new(
            destination.position[0],
            destination.position[1],
            destination.position[2],
        );
        self.world
            .set_position(body, position)
            .map_err(crate::zone::physics_error)?;
        self.world
            .set_velocity(body, Vec3i::ZERO)
            .map_err(crate::zone::physics_error)?;
        self.cancel_cast(player_id);
        self.update_player(player_id, |player| {
            player.facing = destination.facing;
            player.forward = crate::zone::Axis::Zero;
            player.strafe = crate::zone::Axis::Zero;
            player.target = None;
            player.swing_queued = false;
            player.repairing = None;
        });
        self.place_in_interest(crate::EntityRef::Player(player_id), position.x, position.z);
        Ok(())
    }
}
