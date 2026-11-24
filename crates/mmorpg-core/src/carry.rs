//! Carried loads (#1077): a crate, sack, firewood, hay, basket or water jug a player lifts in
//! both arms, carries and sets down, for delivery objectives.
//!
//! - **Sources**: a [`LoadSpot`] is zone content with a stable ID, a [`LoadKind`] and a reach,
//!   such as the last sack on the Wolfrun cart. It hands a load to a player only while an active
//!   quest of theirs still has a [`crate::QuestObjective::DeliverLoad`] of it open, as often as
//!   that player asks, so a lost load is simply fetched again and the quest restarts safely.
//! - **Lift**: [`crate::ZoneCommand::Lift`] takes a load from a source, or picks the player's own
//!   set-down load up again (no source). The zone checks it in the tick: a living player with
//!   empty hands (one load at a time), feet out of the water and off any face, within the reach
//!   of the source or the set-down load. Lifting sheathes the weapon, lowers the guard and stops
//!   a sit, a repair and a cast. A fresh load replaces the player's set-down one.
//! - **Carrying**: both hands are busy, so a swing, a weapon draw, a guard, every ability, a
//!   sit, a repair, gathering, skinning and a waystone are refused with
//!   [`crate::ErrorCode::HandsFull`]; a dodge roll and a climb or mantle are refused the same way,
//!   and no speed boost carries the player past the normal run speed (Greyhaven has no sprint).
//!   Clients see the load in the player's entity flags and play the loaded walk and run.
//! - **Set down**: [`crate::ZoneCommand::SetDown`] puts the load at the player's feet on a valid
//!   surface: standing on the ground, out of the water, off any face and not rolling
//!   ([`crate::ErrorCode::NoFooting`] otherwise). It stays there, seen by every nearby player, for
//!   [`PLACED_LOAD_TICKS`]; only the player who set it down lifts it again. A player who dies
//!   or leaves the zone (logout, handoff) sets the load down where they stand when the footing
//!   allows; one carried into deep water is lost ([`crate::ErrorCode::LoadLost`]).
//! - **Delivery**: [`crate::ZoneCommand::QuestAction`] at the objective's NPC with a load of its
//!   source in hand hands it over: the load is gone and the objective done.
//!
//! A load is never an item: it cannot be sold, banked or duplicated, and the bag and the saved
//! character never hold it. Loads are canonical zone state ([`LoadState`]), so snapshots,
//! replay and recovery carry them; the carry step visits each load once a tick and nothing else.

use std::sync::Arc;

use crate::events::push_event;
use crate::{
    EntityRef, ErrorCode, PLAYER_HALF_EXTENTS_UNITS, PlayerId, QuestObjective, ZoneError,
    ZoneEvent, ZoneSimulation,
};

/// A player's feet reach a source or a set-down load this far on XZ (2 m) ...
pub const LOAD_REACH_UNITS: i32 = 200;
/// ... and only this close to its height (2 m).
pub const LOAD_HEIGHT_UNITS: i32 = 200;
/// The widest reach content may give a source: 5 m, as far as a quest giver's.
pub const MAX_LOAD_REACH_UNITS: i32 = crate::QUEST_REACH_UNITS;
/// A set-down load stays this long (5 min) before it is gone.
pub const PLACED_LOAD_TICKS: u32 = crate::long_ticks_from_ms(300_000);
/// At most this many set-down loads, nearest first, travel in a projection.
pub const MAX_VISIBLE_LOADS: usize = 4;
/// At most this many loads, carried or set down, in one zone: a fresh load past it is refused.
pub const MAX_ZONE_LOADS: usize = 4_096;

/// What a load is: each one of `humanoid-equipment`'s `load.*` items.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LoadKind {
    Crate,
    Sack,
    Firewood,
    Hay,
    Basket,
    WaterJug,
}

impl LoadKind {
    pub const ALL: [Self; 6] = [
        Self::Crate,
        Self::Sack,
        Self::Firewood,
        Self::Hay,
        Self::Basket,
        Self::WaterJug,
    ];

    /// Wire code, 0–5.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Crate => 0,
            Self::Sack => 1,
            Self::Firewood => 2,
            Self::Hay => 3,
            Self::Basket => 4,
            Self::WaterJug => 5,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Crate),
            1 => Some(Self::Sack),
            2 => Some(Self::Firewood),
            3 => Some(Self::Hay),
            4 => Some(Self::Basket),
            5 => Some(Self::WaterJug),
            _ => None,
        }
    }

    /// The content name, as `content/greyhaven/quests.json` spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Crate => "crate",
            Self::Sack => "sack",
            Self::Firewood => "firewood",
            Self::Hay => "hay",
            Self::Basket => "basket",
            Self::WaterJug => "water_jug",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The `humanoid-equipment` item clients hold on the chest (#1073).
    #[must_use]
    pub const fn item(self) -> &'static str {
        match self {
            Self::Crate => "equipment.load.crate",
            Self::Sack => "equipment.load.sack",
            Self::Firewood => "equipment.load.firewood",
            Self::Hay => "equipment.load.hay",
            Self::Basket => "equipment.load.basket-large",
            Self::WaterJug => "equipment.load.water-jug",
        }
    }
}

/// Stable load-source identity within one content revision; `0` names none.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LoadSpotId(u32);

impl LoadSpotId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// One authored load source (#1077).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadSpot {
    pub id: LoadSpotId,
    /// What clients call it, as in "Deliver the last sack".
    pub name: String,
    pub kind: LoadKind,
    /// X, height and Z in units, where its art stands.
    pub at: [i32; 3],
    /// How near on XZ a player's feet must come, in units.
    pub reach: i32,
}

impl LoadSpot {
    /// Whether feet at `feet` stand within reach of the source.
    #[must_use]
    pub fn within(&self, feet: [i32; 3]) -> bool {
        within(self.at, self.reach, feet)
    }
}

fn within(at: [i32; 3], reach: i32, feet: [i32; 3]) -> bool {
    let (dx, dz) = (i64::from(feet[0] - at[0]), i64::from(feet[2] - at[2]));
    dx * dx + dz * dz <= i64::from(reach).pow(2) && (feet[1] - at[1]).abs() <= LOAD_HEIGHT_UNITS
}

/// Where a set-down load stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlacedLoad {
    /// Its base: X, height and Z in units.
    pub at: [i32; 3],
    pub facing: u16,
    /// The tick it is gone.
    pub until: u64,
}

/// A player's one load (#1077), carried or set down; canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadState {
    pub player: PlayerId,
    /// The source it came from, which names its kind and the objectives it delivers.
    pub source: LoadSpotId,
    /// `None` while the player carries it.
    pub placed: Option<PlacedLoad>,
}

impl LoadState {
    #[must_use]
    pub const fn carried(&self) -> bool {
        self.placed.is_none()
    }
}

/// A set-down load near the viewer, as a projection shows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadView {
    pub kind: LoadKind,
    /// The viewer set it down, so it may lift it again.
    pub own: bool,
    pub at: [i32; 3],
    pub facing: u16,
}

type Refusal = Option<(ErrorCode, Option<EntityRef>)>;

/// Whether `intent` needs a free hand (#1077): a swing, a weapon draw, a raised guard, any
/// ability, a seat, a repair, gathering, skinning or a waystone. A carrier is refused them.
pub(crate) const fn needs_hands(intent: &crate::PlayerIntent) -> bool {
    use crate::PlayerIntent as I;
    matches!(
        intent,
        I::Swing { .. }
            | I::SetWeaponDrawn(true)
            | I::Guard(Some(_))
            | I::UseAbility { .. }
            | I::DirectedStrike { .. }
            | I::Sit(_)
            | I::Repair(Some(_))
            | I::Gather(_)
            | I::Skin(_)
            | I::UseWaystone(_)
    )
}

impl ZoneSimulation {
    /// The player's load, carried or set down (#1077).
    #[must_use]
    pub fn load(&self, player_id: PlayerId) -> Option<&LoadState> {
        self.loads.get(&player_id)
    }

    /// What the player carries in both arms (#1077), if anything.
    #[must_use]
    pub fn carried_load(&self, player_id: PlayerId) -> Option<crate::LoadKind> {
        self.loads
            .get(&player_id)
            .filter(|load| load.carried())
            .and_then(|load| self.content.load_spot(load.source))
            .map(|spot| spot.kind)
    }

    /// Whether the player's hands are busy with a load.
    pub(crate) fn carrying(&self, player_id: PlayerId) -> bool {
        self.loads.get(&player_id).is_some_and(LoadState::carried)
    }

    /// The player's feet if they may lift or set down a load there: standing on the ground,
    /// out of deep water, off any face and not rolling. `None` otherwise.
    fn footing(&self, player_id: PlayerId) -> Result<Option<[i32; 3]>, ZoneError> {
        let Some(player) = self.players.get(&player_id) else {
            return Ok(None);
        };
        let body = self
            .world
            .body(crate::entity::body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let centre = body.position();
        let feet = [centre.x, centre.y - PLAYER_HALF_EXTENTS_UNITS[1], centre.z];
        let footed = player.traversal == crate::Traversal::Free
            && player.roll.rolling().is_none()
            && crate::water::swim_depth(self.content.waters(), feet).is_none()
            && crate::zone::is_grounded(&self.world, body)?;
        Ok(footed.then_some(feet))
    }

    /// Whether an active quest of the player still has a delivery of `source` open.
    fn needs_load(&self, player_id: PlayerId, source: crate::LoadSpotId) -> bool {
        let Some(player) = self.players.get(&player_id) else {
            return false;
        };
        player.quests.entries.iter().any(|entry| {
            self.content.quest(entry.quest).is_some_and(|quest| {
                quest
                    .objectives
                    .iter()
                    .zip(entry.progress)
                    .any(|(objective, progress)| {
                        matches!(*objective, QuestObjective::DeliverLoad { source: wanted, .. } if wanted == source)
                            && progress < objective.required()
                    })
            })
        })
    }

    /// The `Lift` intent (#1077): see the module documentation.
    pub(crate) fn lift(
        &mut self,
        player_id: PlayerId,
        source: Option<crate::LoadSpotId>,
    ) -> Result<Refusal, ZoneError> {
        if self.carrying(player_id) {
            return Ok(Some((ErrorCode::HandsFull, None)));
        }
        let Some(feet) = self.footing(player_id)? else {
            return Ok(Some((ErrorCode::NoFooting, None)));
        };
        match source {
            Some(source) => {
                let content = Arc::clone(&self.content);
                let Some(spot) = content.load_spot(source) else {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                };
                if !spot.within(feet) {
                    return Ok(Some((ErrorCode::OutOfRange, None)));
                }
                if !self.needs_load(player_id, source) {
                    return Ok(Some((ErrorCode::InvalidQuest, None)));
                }
                if self.loads.len() >= MAX_ZONE_LOADS && !self.loads.contains_key(&player_id) {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                }
            }
            None => {
                let Some(placed) = self.loads.get(&player_id).and_then(|load| load.placed) else {
                    return Ok(Some((ErrorCode::InvalidTarget, None)));
                };
                if !within(placed.at, LOAD_REACH_UNITS, feet) {
                    return Ok(Some((ErrorCode::OutOfRange, None)));
                }
            }
        }
        let source = match source {
            Some(source) => source,
            None => self
                .loads
                .get(&player_id)
                .map(|load| load.source)
                .ok_or_else(|| ZoneError::new("lifted load is absent"))?,
        };
        self.loads.insert(
            player_id,
            LoadState {
                player: player_id,
                source,
                placed: None,
            },
        );
        self.update_player(player_id, |player| {
            player.weapon_drawn = false;
            player.guard.raised = None;
            player.sitting = crate::Sitting::No;
            player.repairing = None;
        });
        self.interrupt_cast(EntityRef::Player(player_id), None, None);
        Ok(None)
    }

    /// The `SetDown` intent (#1077): see the module documentation.
    pub(crate) fn set_down(&mut self, player_id: PlayerId, now: u64) -> Result<Refusal, ZoneError> {
        if !self.carrying(player_id) {
            return Ok(Some((ErrorCode::NotCarrying, None)));
        }
        let Some(feet) = self.footing(player_id)? else {
            return Ok(Some((ErrorCode::NoFooting, None)));
        };
        self.place_load(player_id, feet, now);
        Ok(None)
    }

    /// Puts the player's carried load down at `feet`.
    fn place_load(&mut self, player_id: PlayerId, feet: [i32; 3], now: u64) {
        let facing = self
            .players
            .get(&player_id)
            .map_or(0, |player| player.facing);
        if let Some(load) = self.loads.get_mut(&player_id) {
            load.placed = Some(PlacedLoad {
                at: feet,
                facing,
                until: now + u64::from(PLACED_LOAD_TICKS),
            });
        }
    }

    /// Hands the player's carried load of `source` over for a delivery objective; `false` when
    /// it carries none of it.
    pub(crate) fn hand_over_load(
        &mut self,
        player_id: PlayerId,
        source: crate::LoadSpotId,
    ) -> bool {
        let held = self
            .loads
            .get(&player_id)
            .is_some_and(|load| load.carried() && load.source == source);
        if held {
            self.loads.remove(&player_id);
        }
        held
    }

    /// A departing player (logout or handoff) sets its carried load down where it stands when
    /// the footing allows; otherwise the load is gone. Call before its body leaves the world.
    pub(crate) fn release_load(&mut self, player_id: PlayerId) -> Result<(), ZoneError> {
        if !self.carrying(player_id) {
            return Ok(());
        }
        match self.footing(player_id)? {
            Some(feet) => self.place_load(player_id, feet, self.tick),
            None => {
                self.loads.remove(&player_id);
            }
        }
        Ok(())
    }

    /// Drops every load of the quests the player abandons: a source no active quest needs
    /// any more hands nothing over.
    pub(crate) fn forget_loads(&mut self, player_id: PlayerId) {
        if let Some(source) = self.loads.get(&player_id).map(|load| load.source)
            && !self.needs_load(player_id, source)
        {
            self.loads.remove(&player_id);
        }
    }

    /// The carry step (#1077), once a tick: each load is visited once. A set-down load whose
    /// time ran out is gone; a dead carrier sets its load down where it fell when the footing
    /// allows, and a carrier in deep water loses it.
    pub(crate) fn advance_loads(&mut self, now: u64) -> Result<(), ZoneError> {
        let mut expired = Vec::new();
        let mut fallen = Vec::new();
        let mut drowned = Vec::new();
        for (&player_id, load) in &self.loads {
            self.tick_work.loads_visited += 1;
            match load.placed {
                Some(placed) => {
                    if placed.until <= now {
                        expired.push(player_id);
                    }
                }
                None => {
                    let Some(player) = self.players.get(&player_id) else {
                        expired.push(player_id);
                        continue;
                    };
                    let body = self
                        .world
                        .body(crate::entity::body_id(EntityRef::Player(player_id)))
                        .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
                    let centre = body.position();
                    let feet = [centre.x, centre.y - PLAYER_HALF_EXTENTS_UNITS[1], centre.z];
                    if crate::water::swim_depth(self.content.waters(), feet).is_some() {
                        drowned.push(player_id);
                    } else if !player.is_alive() {
                        fallen.push(player_id);
                    }
                }
            }
        }
        for player_id in expired {
            self.loads.remove(&player_id);
        }
        for player_id in fallen {
            self.release_load(player_id)?;
        }
        for player_id in drowned {
            self.loads.remove(&player_id);
            if let Some(player) = self.players.get_mut(&player_id) {
                push_event(
                    &mut player.events,
                    ZoneEvent::Error {
                        code: ErrorCode::LoadLost,
                        target: None,
                    },
                );
            }
        }
        Ok(())
    }

    /// Set-down loads within `radius_squared` of `centre` on XZ, nearest first, at most
    /// [`MAX_VISIBLE_LOADS`].
    pub(crate) fn visible_loads(
        &self,
        viewer: PlayerId,
        centre: [i32; 2],
        radius_squared: i128,
    ) -> Vec<LoadView> {
        let mut near: Vec<_> = self
            .loads
            .values()
            .filter_map(|load| {
                let placed = load.placed?;
                let kind = self.content.load_spot(load.source)?.kind;
                let dx = i128::from(placed.at[0]) - i128::from(centre[0]);
                let dz = i128::from(placed.at[2]) - i128::from(centre[1]);
                let distance = dx * dx + dz * dz;
                (distance <= radius_squared).then_some((
                    distance,
                    load.player,
                    LoadView {
                        kind,
                        own: load.player == viewer,
                        at: placed.at,
                        facing: placed.facing,
                    },
                ))
            })
            .collect();
        near.sort_unstable_by_key(|(distance, player, _)| (*distance, *player));
        near.truncate(MAX_VISIBLE_LOADS);
        near.into_iter().map(|(_, _, view)| view).collect()
    }

    /// Loads in player-ID order, for a canonical snapshot.
    pub(crate) fn canonical_loads(&self) -> Vec<LoadState> {
        self.loads.values().copied().collect()
    }

    /// Restores a canonical snapshot's loads: each of a source the content knows.
    pub(crate) fn restore_loads(&mut self, loads: &[LoadState]) -> Result<(), ZoneError> {
        for load in loads {
            if self.content.load_spot(load.source).is_none()
                || (load.carried() && !self.players.contains_key(&load.player))
                || self.loads.len() >= MAX_ZONE_LOADS
                || self.loads.insert(load.player, *load).is_some()
            {
                return Err(ZoneError::new("snapshot load is invalid"));
            }
        }
        Ok(())
    }
}
