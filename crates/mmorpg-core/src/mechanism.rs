//! Shrine mechanisms (#1019): authored glyph stones a player works with
//! [`crate::ZoneCommand::Operate`], the seals they open and the wards they lift.
//!
//! - **Mechanisms**: a [`Mechanism`] is zone content with a stable ID, a name and a spot. A living
//!   player whose feet stand within its reach (and within [`crate::EVIDENCE_HEIGHT_UNITS`] of its
//!   height, so a diver must go down to it) works it; anyone may, with or without a quest.
//! - **Seals**: a [`Seal`] is a stone [`crate::StructurePiece`] closing a chamber, and the
//!   mechanisms that open it. Working one of them sets it for [`SEAL_WINDOW_TICKS`]; once all are
//!   set together the piece sinks (no collider) and every player who worked one of them this
//!   time is an opener and completes their open [`crate::QuestObjective::Unseal`] of it, once.
//!   Working a set mechanism again makes the player an opener too, so a whole party can share
//!   the opening. A mechanism left alone past its window drops back, so a lone diver retries.
//!   An open seal stands open [`SEAL_OPEN_TICKS`]; then, once no living player is inside its
//!   chamber and nothing stands in the piece's box, the piece rises again and every mechanism is
//!   unset: the chamber reseals the same way on every host. Blasts and shots never break a seal.
//! - **Wards**: a [`Ward`] makes every creature of a template turn every player's damage aside
//!   ([`crate::ErrorCode::Warded`]) until its mechanism is worked; then its weak point is exposed
//!   for [`WARD_EXPOSED_TICKS`], and damage lands. Working the mechanism again while exposed
//!   changes nothing; afterwards it exposes the weak point anew, so one party member can keep the
//!   ward down while the others strike.
//! - **Sequences**: a [`MechanismSequence`] is an authored order of mechanisms, the standing
//!   stones of a ring. It holds no zone state: each character works it for itself through a
//!   [`crate::QuestObjective::WorkInOrder`] whose earlier objectives are done. Working its next
//!   stone counts one more; working the stone just counted again changes nothing; any other of its
//!   stones starts the order over (at one when that stone is its first) and tells the player
//!   [`crate::ErrorCode::OutOfOrder`]. The log stores the count, so recovery and replay keep it.
//!
//! Set ticks, openers, the opening tick and each ward's exposure belong to canonical zone state
//! ([`SealState`], [`WardState`]); every projection carries each seal's set mechanisms and
//! whether it stands open, and each ward's exposure ([`SealView`], [`WardView`]). The mechanism
//! step visits each seal and ward once a tick.

use std::sync::Arc;

use crate::{
    CreatureTemplateId, EntityRef, ErrorCode, PieceId, PlayerId, QuestObjective, ZoneError,
    ZoneSimulation,
};

/// A set mechanism drops back this long (60 s) after it was worked unless the seal opened.
pub const SEAL_WINDOW_TICKS: u32 = crate::long_ticks_from_ms(60_000);
/// An opened seal stands open at least this long (5 min).
pub const SEAL_OPEN_TICKS: u32 = crate::long_ticks_from_ms(300_000);
/// A worked ward exposes its weak point this long (15 s).
pub const WARD_EXPOSED_TICKS: u32 = crate::long_ticks_from_ms(15_000);
/// The widest reach content may give a mechanism: 3 m.
pub const MAX_MECHANISM_REACH_UNITS: i32 = 300;
/// The most mechanisms one seal may ask for.
pub const MAX_SEAL_MECHANISMS: usize = 4;
/// The most seals, and the most wards, one zone may have.
pub const MAX_SEALS: usize = 8;
/// The most mechanisms one sequence may order (#1019).
pub const MAX_SEQUENCE_MECHANISMS: usize = 8;

/// Stable sequence identity within one content revision (#1019); `0` names none.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SequenceId(u32);

impl SequenceId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An authored order of mechanisms (#1019): the stones of a ring worked in an inscription's order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MechanismSequence {
    pub id: SequenceId,
    /// What clients call the whole, as in "the ring's stones".
    pub name: String,
    /// The mechanisms in the order they are worked, `2..=MAX_SEQUENCE_MECHANISMS`, distinct.
    pub mechanisms: Vec<MechanismId>,
}

/// Stable mechanism identity within one content revision; `0` names none.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MechanismId(u32);

impl MechanismId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// One authored mechanism (#1019): a glyph stone, a lever.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mechanism {
    pub id: MechanismId,
    /// What clients call it, as in "Work the lakeward glyph".
    pub name: String,
    /// The spot: X, height and Z in units, where its art stands.
    pub at: [i32; 3],
    /// How near on XZ a player's feet must come, in units.
    pub reach: i32,
}

impl Mechanism {
    /// Whether feet at `feet` stand within reach of it.
    #[must_use]
    pub fn within(&self, feet: [i32; 3]) -> bool {
        let [x, y, z] = self.at;
        let (dx, dz) = (i64::from(feet[0] - x), i64::from(feet[2] - z));
        dx * dx + dz * dz <= i64::from(self.reach).pow(2)
            && (feet[1] - y).abs() <= crate::EVIDENCE_HEIGHT_UNITS
    }
}

/// An authored seal (#1019): a stone piece closing a chamber and the mechanisms that open it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Seal {
    /// The stone piece; it starts standing.
    pub piece: PieceId,
    /// The mechanisms that must be set together, `1..=MAX_SEAL_MECHANISMS`, in ID order.
    pub mechanisms: Vec<MechanismId>,
    /// The chamber it closes, a `(min, max)` box in units: a living player whose body is inside
    /// holds it open.
    pub chamber: ([i32; 3], [i32; 3]),
}

/// An authored ward (#1019): creatures of `template` are immune to players until `mechanism`
/// exposes their weak point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ward {
    pub template: CreatureTemplateId,
    pub mechanism: MechanismId,
}

/// A seal's canonical state (#1019).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SealState {
    pub piece: PieceId,
    /// When each of its mechanisms was set, in the seal's mechanism order; `None` while unset.
    pub set: Vec<Option<u64>>,
    /// The players who worked a mechanism toward the coming opening, by ID.
    pub openers: Vec<PlayerId>,
    /// The tick it opened, while it stands open.
    pub opened: Option<u64>,
}

/// A ward's canonical state (#1019): the tick its weak point closes again (0: never exposed).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WardState {
    pub mechanism: MechanismId,
    pub exposed_until: u64,
}

/// A seal as every player sees it (#1019): which of its mechanisms are set (bit `i` for its
/// `i`th) and whether it stands open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SealView {
    pub piece: PieceId,
    pub set: u8,
    pub open: bool,
}

/// A ward as every player sees it (#1019): the tick its weak point closes again (0: never
/// exposed), so the view changes only when its glyph is worked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WardView {
    pub mechanism: MechanismId,
    pub exposed_until: u64,
}

impl WardView {
    /// Whether the weak point is exposed at `tick`.
    #[must_use]
    pub const fn exposed_at(self, tick: u64) -> bool {
        self.exposed_until > tick
    }
}

type Refusal = Option<(ErrorCode, Option<EntityRef>)>;

impl ZoneSimulation {
    /// Every seal closed with nothing set, and every ward warded: building a zone.
    pub(crate) fn build_mechanisms(
        content: &crate::ZoneContent,
    ) -> (Vec<SealState>, Vec<WardState>) {
        (
            content
                .seals()
                .iter()
                .map(|seal| SealState {
                    piece: seal.piece,
                    set: vec![None; seal.mechanisms.len()],
                    openers: Vec::new(),
                    opened: None,
                })
                .collect(),
            content
                .wards()
                .iter()
                .map(|ward| WardState {
                    mechanism: ward.mechanism,
                    exposed_until: 0,
                })
                .collect(),
        )
    }

    /// The `Operate` intent (#1019): works mechanism `id` for the player. An unknown one is an
    /// invalid target, one out of reach is out of range, and one of a seal that already stands
    /// open is refused with [`ErrorCode::SealOpen`]; refusals change nothing.
    pub(crate) fn operate(
        &mut self,
        player_id: PlayerId,
        id: u32,
        now: u64,
    ) -> Result<Refusal, ZoneError> {
        let content = Arc::clone(&self.content);
        let id = MechanismId::new(id);
        let Some(mechanism) = content.mechanism(id) else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        let centre = self.player_position(player_id)?;
        let feet = [
            centre.x,
            centre.y - crate::PLAYER_HALF_EXTENTS_UNITS[1],
            centre.z,
        ];
        if !mechanism.within(feet) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        if content.sequence_of(id).is_some() {
            self.work_in_order(player_id, id);
            return Ok(None);
        }
        if let Some(index) = content.wards().iter().position(|ward| ward.mechanism == id) {
            let state = &mut self.wards[index];
            if state.exposed_until <= now {
                state.exposed_until = now + u64::from(WARD_EXPOSED_TICKS);
            }
            return Ok(None);
        }
        let found = content
            .seals()
            .iter()
            .enumerate()
            .find_map(|(index, seal)| {
                let slot = seal
                    .mechanisms
                    .iter()
                    .position(|mechanism| *mechanism == id)?;
                Some((index, slot, seal))
            });
        let Some((index, slot, seal)) = found else {
            return Ok(None);
        };
        let state = &mut self.seals[index];
        if state.opened.is_some() {
            return Ok(Some((ErrorCode::SealOpen, None)));
        }
        if state.set[slot].is_none() {
            state.set[slot] = Some(now);
        }
        if let Err(at) = state.openers.binary_search(&player_id) {
            state.openers.insert(at, player_id);
        }
        if state.set.iter().all(Option::is_some) {
            self.open_seal(index, seal, now)?;
        }
        Ok(None)
    }

    /// Works sequence mechanism `id` toward the player's open in-order objectives (#1019): see
    /// the module documentation. Nothing else changes, and a player with none open is unaffected.
    fn work_in_order(&mut self, player_id: PlayerId, id: MechanismId) {
        let content = Arc::clone(&self.content);
        let Some(player) = self.players.get(&player_id) else {
            return;
        };
        let mut changes = Vec::new();
        let mut broken = false;
        for entry in &player.quests.entries {
            let Some(quest) = content.quest(entry.quest) else {
                continue;
            };
            let current = crate::quest::progress(quest, entry, &player.inventory, *player.depths);
            for (index, objective) in quest.objectives.iter().enumerate() {
                let QuestObjective::WorkInOrder { sequence, count } = *objective else {
                    continue;
                };
                let done = current[index];
                let Some(order) = content
                    .sequence(sequence)
                    .filter(|order| done < count && order.mechanisms.contains(&id))
                else {
                    continue;
                };
                let ready = quest.objectives[..index]
                    .iter()
                    .zip(current)
                    .all(|(earlier, count)| count >= earlier.required());
                if !ready {
                    continue;
                }
                let at = usize::from(done);
                let next = if order.mechanisms.get(at) == Some(&id) {
                    done + 1
                } else if at > 0 && order.mechanisms[at - 1] == id {
                    // The stone just counted, worked again.
                    continue;
                } else {
                    broken = true;
                    u8::from(order.mechanisms.first() == Some(&id))
                };
                changes.push((quest.id, index, next));
            }
        }
        let Some(player) = self.players.get_mut(&player_id) else {
            return;
        };
        for (quest, index, count) in changes {
            if let Some(entry) = player
                .quests
                .entries
                .iter_mut()
                .find(|entry| entry.quest == quest)
            {
                entry.progress[index] = count;
                crate::events::push_event(
                    &mut player.events,
                    crate::ZoneEvent::QuestProgress {
                        quest,
                        objective: u8::try_from(index).unwrap_or(u8::MAX),
                        count,
                    },
                );
            }
        }
        if broken {
            crate::events::push_event(
                &mut player.events,
                crate::ZoneEvent::Error {
                    code: ErrorCode::OutOfOrder,
                    target: None,
                },
            );
        }
    }

    /// Sinks seal `index`'s piece and credits its openers.
    fn open_seal(&mut self, index: usize, seal: &Seal, now: u64) -> Result<(), ZoneError> {
        self.fell_piece(seal.piece, now)?;
        let openers = {
            let state = &mut self.seals[index];
            state.opened = Some(now);
            state.set.iter_mut().for_each(|set| *set = None);
            std::mem::take(&mut state.openers)
        };
        for opener in openers {
            self.complete_objectives(opener, |objective| {
                objective == QuestObjective::Unseal { piece: seal.piece }
            });
        }
        Ok(())
    }

    /// The mechanism step (#1019), once a tick after the structures: each seal and ward once. A
    /// closed seal's mechanisms past their window drop back (with their openers once none stays
    /// set); an open seal past its time rises again once its chamber and box are empty.
    pub(crate) fn advance_mechanisms(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for (index, seal) in content.seals().iter().enumerate() {
            self.tick_work.mechanisms_visited += 1;
            match self.seals[index].opened {
                None => {
                    let state = &mut self.seals[index];
                    for set in &mut state.set {
                        if set.is_some_and(|at| now >= at + u64::from(SEAL_WINDOW_TICKS)) {
                            *set = None;
                        }
                    }
                    if state.set.iter().all(Option::is_none) {
                        state.openers.clear();
                    }
                }
                Some(opened) if now >= opened + u64::from(SEAL_OPEN_TICKS) => {
                    if self.seal_held(seal)? {
                        continue;
                    }
                    let Some(piece) = content.pieces().iter().find(|piece| piece.id == seal.piece)
                    else {
                        continue;
                    };
                    if self.piece_box_occupied(piece)? {
                        continue;
                    }
                    self.stand_piece(piece, now)?;
                    self.seals[index].opened = None;
                }
                Some(_) => {}
            }
        }
        self.tick_work.mechanisms_visited += content.wards().len();
        Ok(())
    }

    /// Whether a living player's body is inside the seal's chamber: one overlap query of the box,
    /// whose bodies the step counts, never a walk over the zone's players.
    fn seal_held(&mut self, seal: &Seal) -> Result<bool, ZoneError> {
        let (min, max) = seal.chamber;
        let centre: [i32; 3] = std::array::from_fn(|axis| min[axis] + (max[axis] - min[axis]) / 2);
        let half: [i32; 3] = std::array::from_fn(|axis| (max[axis] - min[axis]) / 2);
        let bodies = self
            .world
            .overlap_query(crate::physics::Aabb::new(
                crate::zone::vector(centre),
                crate::zone::vector(half),
            ))
            .map_err(crate::zone::physics_error)?;
        self.tick_work.seal_bodies_checked += bodies.len();
        Ok(bodies.into_iter().any(|body| {
            matches!(crate::entity::entity_of(body), Some(EntityRef::Player(player))
                if self.players.get(&player).is_some_and(|state| state.is_alive()))
        }))
    }

    /// Whether a player's damage to `creature` is turned aside by a ward (#1019): its template
    /// is warded and the ward's weak point is not exposed now.
    pub(crate) fn warded(&self, creature: crate::CreatureId) -> bool {
        if self.content.wards().is_empty() {
            return false;
        }
        let Some((_, template)) = self.content.creature_entry(creature) else {
            return false;
        };
        let template = template.id;
        self.content
            .wards()
            .iter()
            .zip(&self.wards)
            .any(|(ward, state)| ward.template == template && state.exposed_until <= self.tick + 1)
    }

    /// Every seal's state as every player sees it, in content order (#1019).
    #[must_use]
    pub fn seal_views(&self) -> Vec<SealView> {
        self.seals
            .iter()
            .map(|state| SealView {
                piece: state.piece,
                set: state.set.iter().enumerate().fold(0, |mask, (bit, set)| {
                    mask | (u8::from(set.is_some()) << bit)
                }),
                open: state.opened.is_some(),
            })
            .collect()
    }

    /// Every ward's exposure as every player sees it, in content order (#1019).
    #[must_use]
    pub fn ward_views(&self) -> Vec<WardView> {
        self.wards
            .iter()
            .map(|state| WardView {
                mechanism: state.mechanism,
                exposed_until: state.exposed_until,
            })
            .collect()
    }

    /// Seals in content order, for a canonical snapshot.
    pub(crate) fn canonical_seals(&self) -> Vec<SealState> {
        self.seals.clone()
    }

    /// Wards in content order, for a canonical snapshot.
    pub(crate) fn canonical_wards(&self) -> Vec<WardState> {
        self.wards.clone()
    }

    /// Restores a canonical snapshot's seals and wards: one per content seal and ward, in order,
    /// set ticks and the opening no later than the snapshot, openers ordered and unique, an open
    /// seal's piece sunk and a closed one's standing.
    pub(crate) fn restore_mechanisms(
        &mut self,
        seals: &[SealState],
        wards: &[WardState],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        if seals.len() != content.seals().len() || wards.len() != content.wards().len() {
            return Err(ZoneError::new(
                "snapshot seals or wards do not match the content",
            ));
        }
        for (state, seal) in seals.iter().zip(content.seals()) {
            let standing = self.piece_health(seal.piece) > 0;
            let valid = state.piece == seal.piece
                && state.set.len() == seal.mechanisms.len()
                && state.set.iter().flatten().all(|at| *at <= tick)
                && state.openers.windows(2).all(|pair| pair[0] < pair[1])
                && match state.opened {
                    Some(opened) => {
                        opened <= tick && !standing && state.set.iter().all(Option::is_none)
                    }
                    None => standing,
                };
            if !valid {
                return Err(ZoneError::new("snapshot seal is invalid"));
            }
        }
        if wards
            .iter()
            .zip(content.wards())
            .any(|(state, ward)| state.mechanism != ward.mechanism)
        {
            return Err(ZoneError::new("snapshot ward is invalid"));
        }
        self.seals = seals.to_vec();
        self.wards = wards.to_vec();
        Ok(())
    }
}
