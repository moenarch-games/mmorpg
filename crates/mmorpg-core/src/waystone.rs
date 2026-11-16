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
//! ([`crate::CharacterTransfer`]): the traveller becomes a departure
//! ([`crate::ZoneSimulation::departures`]) and stays put until its host
//! freezes it toward the stone's arrival. A host that cannot move players
//! between zones leaves that off, and such travel is refused with
//! [`crate::ErrorCode::InvalidWaystone`]
//! ([`crate::ZoneSimulation::enable_waystone_departures`]).

use std::collections::BTreeSet;

use crate::{
    CharacterLocation, ErrorCode, PLAYER_HALF_EXTENTS_UNITS, PlayerId, ZoneError, ZoneId,
    ZoneSimulation,
};

/// How close a player's body centre must come to a stone, in XZ units (8 m), to
/// unlock it or travel from it.
pub const WAYSTONE_DISCOVERY_UNITS: i64 = 800;

/// A stone of the global network; `0` names no stone. Whether a stone exists is the catalog's
/// ([`WAYSTONES`]) to say.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WaystoneId(u32);

impl WaystoneId {
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The stones a character has unlocked, in ID order, as many as the network has (#882).
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct WaystoneSet(BTreeSet<WaystoneId>);

impl WaystoneSet {
    pub const EMPTY: Self = Self(BTreeSet::new());

    /// The set of these stones; `None` when one is not a stone of [`WAYSTONES`].
    #[must_use]
    pub fn from_ids(ids: impl IntoIterator<Item = WaystoneId>) -> Option<Self> {
        let set: BTreeSet<_> = ids.into_iter().collect();
        set.iter()
            .all(|id| waystone(*id).is_some())
            .then_some(Self(set))
    }

    #[must_use]
    pub fn contains(&self, id: WaystoneId) -> bool {
        self.0.contains(&id)
    }

    #[must_use]
    pub fn with(mut self, id: WaystoneId) -> Self {
        self.0.insert(id);
        self
    }

    /// Unlocks `id`; whether it was locked.
    pub fn insert(&mut self, id: WaystoneId) -> bool {
        self.0.insert(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Unlocked stones in ID order.
    pub fn iter(&self) -> impl Iterator<Item = WaystoneId> + '_ {
        self.0.iter().copied()
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

const fn id(value: u32) -> WaystoneId {
    match WaystoneId::new(value) {
        Some(id) => id,
        None => panic!("waystone IDs are nonzero"),
    }
}

/// The Greyhaven Outpost stone, beside the hub's well.
pub const GREYHAVEN_OUTPOST: WaystoneId = id(1);
/// The stone of the Seam Test West zone (#145).
pub const SEAM_TEST_WEST: WaystoneId = id(2);
/// The stone of the Seam Test East zone (#145).
pub const SEAM_TEST_EAST: WaystoneId = id(3);
/// Barrowdown's stone, by the hamlet's well on the King's Road Downs (#818).
pub const BARROWDOWN: WaystoneId = id(4);
/// Fenford's stone, by the Fenwater's ford (#818).
pub const FENFORD: WaystoneId = id(5);
/// The Westmere Lodge's stone, on Westmere Tarn's eastern shore (#818).
pub const WESTMERE_LODGE: WaystoneId = id(6);

/// The logical zone Greyhaven Vale runs as in the waystone network.
pub const GREYHAVEN_ZONE: ZoneId = ZoneId::new(1);

/// The whole network, in ID order.
pub const WAYSTONES: [WaystoneSite; 6] = [
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
    WaystoneSite {
        id: BARROWDOWN,
        name: "Barrowdown",
        zone: GREYHAVEN_ZONE,
        stone: [55_000, 64_800],
        arrival: [55_000, 65_200],
        facing: 0,
    },
    WaystoneSite {
        id: FENFORD,
        name: "Fenford",
        zone: GREYHAVEN_ZONE,
        stone: [104_400, 22_600],
        arrival: [104_400, 22_200],
        facing: 32_768,
    },
    WaystoneSite {
        id: WESTMERE_LODGE,
        name: "Westmere Lodge",
        zone: GREYHAVEN_ZONE,
        stone: [-65_200, 8_400],
        arrival: [-65_200, 8_000],
        facing: 32_768,
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
            .map(|player| player.waystones.clone())
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
                player.waystones.insert(id);
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
        let unlocked = &player.waystones;
        let Some(origin) = self.waystone_in_reach(player_id) else {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        };
        let Some(site) = waystone(target) else {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        };
        if origin == target || !unlocked.contains(target) {
            return Ok(Some((ErrorCode::InvalidWaystone, None)));
        }
        let destination = self.waystone_arrival(site);
        if !self.content.waystones().contains(&target) {
            // Another logical zone: the player departs through the fenced
            // transfer, which the host starts from `departures`.
            if !self.transfers.waystone_departures_enabled {
                return Ok(Some((ErrorCode::InvalidWaystone, None)));
            }
            self.transfers
                .waystone_departures
                .insert(player_id, destination);
            return Ok(None);
        }
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
