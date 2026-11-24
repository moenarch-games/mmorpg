//! Construction sites (#1019): a structure piece that starts unbuilt and stands once players
//! carry enough material loads to it, such as Millbrook's barricades.
//!
//! - **Sites**: a [`ConstructionSite`] names a [`crate::StructurePiece`], the load source its
//!   material comes from and how many loads raise it. Its piece starts unbuilt: no health, no
//!   collider. It never regrows from nothing by itself; damage short of that regrows as any
//!   piece's does.
//! - **Material**: [`crate::ZoneCommand::SetDown`] within [`crate::REPAIR_REACH_UNITS`] of an
//!   unbuilt site's piece with a load of its source in both arms puts the load into the site
//!   instead of on the ground: the load is gone, the site counts it, and the player is one of
//!   its builders. A source hands out a load only while an active quest of the player still has
//!   a [`crate::QuestObjective::Build`] of one of its sites open, or a delivery of it.
//! - **Raising**: once the site has its loads and nothing stands in the piece's box, the piece
//!   stands at full health with its collider, real collision for players and creatures alike.
//!   Every builder still in the zone completes its open build objective of the piece, once; a
//!   builder who left misses the credit.
//! - **Taking down**: a raised piece stands for [`STAND_TICKS`]; then, unless a raid incident is
//!   running, the farmhands take it down and the site is unbuilt again, so later players raise it
//!   anew. A piece destroyed before then (a blast) is unbuilt at once.
//!
//! Material counts, builders and the raising tick are canonical zone state
//! ([`ConstructionState`]); the construction step visits each site once a tick.

use std::sync::Arc;

use crate::{PieceId, PlayerId, QuestObjective, ZoneError, ZoneSimulation};

/// A raised piece stands this long (10 min) before it is taken down.
pub const STAND_TICKS: u32 = crate::long_ticks_from_ms(600_000);
/// The most loads one site may ask for.
pub const MAX_CONSTRUCTION_MATERIALS: u8 = 4;
/// The most construction sites one zone may have.
pub const MAX_CONSTRUCTION_SITES: usize = 64;

/// One authored construction site (#1019).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstructionSite {
    /// The piece it raises, which starts unbuilt.
    pub piece: PieceId,
    /// Where its material comes from.
    pub source: crate::LoadSpotId,
    /// Loads that raise it, `1..=MAX_CONSTRUCTION_MATERIALS`.
    pub materials: u8,
}

/// A site's canonical state (#1019).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConstructionState {
    pub piece: PieceId,
    /// Loads it holds toward its next raising; zero while raised.
    pub materials: u8,
    /// The players who brought them, by ID; empty while raised.
    pub builders: Vec<PlayerId>,
    /// The tick its piece was raised, while it stands.
    pub raised: Option<u64>,
}

impl ZoneSimulation {
    /// Every site's state, unbuilt and empty: building a zone.
    pub(crate) fn build_constructions(
        content: &crate::ZoneContent,
    ) -> std::collections::BTreeMap<PieceId, ConstructionState> {
        content
            .constructions()
            .iter()
            .map(|site| {
                (
                    site.piece,
                    ConstructionState {
                        piece: site.piece,
                        ..ConstructionState::default()
                    },
                )
            })
            .collect()
    }

    /// The site's state (#1019).
    #[must_use]
    pub fn construction(&self, piece: PieceId) -> Option<&ConstructionState> {
        self.constructions.get(&piece)
    }

    /// Whether an active quest of the player still has a build of a site of `source` open.
    pub(crate) fn needs_material(&self, player_id: PlayerId, source: crate::LoadSpotId) -> bool {
        let Some(player) = self.players.get(&player_id) else {
            return false;
        };
        player.quests.entries.iter().any(|entry| {
            self.content.quest(entry.quest).is_some_and(|quest| {
                quest
                    .objectives
                    .iter()
                    .zip(entry.progress)
                    .any(|(objective, progress)| match *objective {
                        QuestObjective::Build { piece } => {
                            progress < objective.required()
                                && self
                                    .content
                                    .construction(piece)
                                    .is_some_and(|site| site.source == source)
                        }
                        _ => false,
                    })
            })
        })
    }

    /// Puts the player's carried load of `source` into the first unbuilt site of that source
    /// within reach of `feet` that still wants material; `false` when there is none.
    pub(crate) fn feed_construction(
        &mut self,
        player_id: PlayerId,
        source: crate::LoadSpotId,
        feet: [i32; 3],
        now: u64,
    ) -> Result<bool, ZoneError> {
        let content = Arc::clone(&self.content);
        let site = content.constructions().iter().find(|site| {
            site.source == source
                && self
                    .constructions
                    .get(&site.piece)
                    .is_some_and(|state| state.raised.is_none() && state.materials < site.materials)
                && content
                    .pieces()
                    .iter()
                    .find(|piece| piece.id == site.piece)
                    .is_some_and(|piece| piece.within_repair_reach(feet))
        });
        let Some(site) = site else {
            return Ok(false);
        };
        self.loads.remove(&player_id);
        if let Some(state) = self.constructions.get_mut(&site.piece) {
            state.materials += 1;
            if let Err(at) = state.builders.binary_search(&player_id) {
                state.builders.insert(at, player_id);
            }
        }
        self.raise_when_ready(site, now)?;
        Ok(true)
    }

    /// Raises a site that has its loads once nothing stands in its piece's box, crediting its
    /// builders.
    fn raise_when_ready(
        &mut self,
        site: &crate::ConstructionSite,
        now: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let ready = self
            .constructions
            .get(&site.piece)
            .is_some_and(|state| state.raised.is_none() && state.materials >= site.materials);
        let Some(piece) = content.pieces().iter().find(|piece| piece.id == site.piece) else {
            return Ok(());
        };
        if !ready || self.piece_box_occupied(piece)? {
            return Ok(());
        }
        self.stand_piece(piece, now)?;
        let builders = self
            .constructions
            .get_mut(&site.piece)
            .map(|state| {
                state.materials = 0;
                state.raised = Some(now);
                std::mem::take(&mut state.builders)
            })
            .unwrap_or_default();
        for builder in builders {
            self.complete_objectives(builder, |objective| {
                objective == QuestObjective::Build { piece: site.piece }
            });
        }
        Ok(())
    }

    /// The construction step (#1019), once a tick after the pieces regrew: each site is visited
    /// once. A destroyed raised piece is unbuilt; one standing past [`STAND_TICKS`] is taken down
    /// while no raid incident runs; a site with its loads rises once its box is clear.
    pub(crate) fn advance_constructions(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for site in content.constructions() {
            self.tick_work.constructions_visited += 1;
            let Some(state) = self.constructions.get(&site.piece) else {
                continue;
            };
            match state.raised {
                Some(raised) => {
                    let standing = self.piece_health(site.piece) > 0;
                    let due = now >= raised.saturating_add(u64::from(STAND_TICKS))
                        && self.raid.active.is_none();
                    if standing && due {
                        self.fell_piece(site.piece, now)?;
                    }
                    if (!standing || due)
                        && let Some(state) = self.constructions.get_mut(&site.piece)
                    {
                        state.raised = None;
                    }
                }
                None if state.materials >= site.materials => self.raise_when_ready(site, now)?,
                None => {}
            }
        }
        Ok(())
    }

    /// Sites in content order, for a canonical snapshot.
    pub(crate) fn canonical_constructions(&self) -> Vec<ConstructionState> {
        self.constructions.values().cloned().collect()
    }

    /// Restores a canonical snapshot's sites: one per content site, in order, within their
    /// material counts, a raised one standing and an unbuilt one not.
    pub(crate) fn restore_constructions(
        &mut self,
        states: &[ConstructionState],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        if states.len() != content.constructions().len() {
            return Err(ZoneError::new(
                "snapshot constructions do not match the content sites",
            ));
        }
        for (state, site) in states.iter().zip(content.constructions()) {
            let standing = self.piece_health(site.piece) > 0;
            let valid = state.piece == site.piece
                && state.builders.len() <= usize::from(state.materials)
                && state.builders.windows(2).all(|pair| pair[0] < pair[1])
                && match state.raised {
                    Some(raised) => {
                        raised <= tick && state.materials == 0 && state.builders.is_empty()
                    }
                    None => state.materials <= site.materials && !standing,
                };
            if !valid {
                return Err(ZoneError::new("snapshot construction is invalid"));
            }
            self.constructions.insert(site.piece, state.clone());
        }
        Ok(())
    }
}
