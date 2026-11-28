//! Gathering (#917, owner choice B: crafted and found gear are equally worthwhile): the
//! materials a smith's recipe ([`crate::VendorOffer::materials`]) turns into gear.
//!
//! - **Ore veins** are authored [`GatherNode`]s. A vein is a rare, contested node (Atlas
//!   5.3.16): one gather takes its whole yield and depletes it for everyone until it regrows
//!   [`GatherNode::respawn_ticks`] later. The zone keeps when each vein is ready again, so
//!   every client sees the same full or spent vein.
//! - **Skinning** takes a beast's materials from its corpse ([`skin_yield`]): once per death,
//!   by a player the corpse's loot belongs to, like the corpse's loot itself. It never touches
//!   the corpse's rolled loot.
//!
//! Both are server-owned intents settled atomically into the bag: everything fits or nothing
//! changes, and nothing is drawn from any random stream, so replay and recovery repeat them
//! exactly.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::creature::Life;
use crate::{
    CreatureFamily, CreatureId, EntityRef, ErrorCode, ItemId, PlayerId, ZoneError, ZoneSimulation,
};

/// Inclusive horizontal (XZ) distance from a vein's centre to the gatherer (3 m).
pub const GATHER_REACH_UNITS: i32 = 300;

/// Ore from a ridge vein (#917).
pub const IRON_ORE: ItemId = ItemId::new(28);
/// A boar's hide, skinned from its corpse (#917).
pub const BOAR_HIDE: ItemId = ItemId::new(29);
/// A boar's tusk: the region's quest item (#820), also skinned from any boar (#917).
pub const BOAR_TUSK: ItemId = ItemId::new(19);
/// The smith's boar-spear, crafted from ore, hides and tusks (#917).
pub const HOLLOWPINE_BOAR_SPEAR: ItemId = ItemId::new(30);

/// One authored ore vein: a shared node that yields `quantity` of `item` and regrows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GatherNode {
    /// Centre XZ in units; the dressing draws its vein here.
    pub at: [i32; 2],
    pub item: ItemId,
    pub quantity: u16,
    /// Ticks from a gather until the vein is full again.
    pub respawn_ticks: u32,
}

/// What skinning a corpse of `family` yields, in bag order; empty when the beast has nothing
/// to skin.
#[must_use]
pub const fn skin_yield(family: CreatureFamily) -> &'static [(ItemId, u16)] {
    match family {
        CreatureFamily::Boar => &[(BOAR_HIDE, 1), (BOAR_TUSK, 1)],
        _ => &[],
    }
}

/// The zone's gathering state; canonical.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GatheringState {
    /// The tick each content vein is full again, in content order; a vein is full once the
    /// zone tick reaches it.
    pub veins: Vec<u64>,
    /// Dead creatures already skinned this death; a respawn clears its entry.
    pub skinned: BTreeSet<CreatureId>,
}

impl GatheringState {
    /// Every vein of `nodes` full.
    #[must_use]
    pub fn full(nodes: &[GatherNode]) -> Self {
        Self {
            veins: vec![0; nodes.len()],
            skinned: BTreeSet::new(),
        }
    }
}

impl ZoneSimulation {
    /// Whether content vein `node` is spent at `tick`.
    #[must_use]
    pub fn vein_depleted(&self, node: usize, tick: u64) -> bool {
        self.gathering
            .veins
            .get(node)
            .is_some_and(|&ready| ready > tick)
    }

    /// The content veins spent at the current tick, in content order.
    #[must_use]
    pub fn depleted_veins(&self) -> Vec<u32> {
        (0..self.gathering.veins.len())
            .filter(|&node| self.vein_depleted(node, self.tick))
            .filter_map(|node| u32::try_from(node).ok())
            .collect()
    }

    /// Restores the canonical gathering state, which must match the content's veins and name
    /// only corpses.
    pub(crate) fn restore_gathering(
        &mut self,
        state: GatheringState,
        tick: u64,
    ) -> Result<(), ZoneError> {
        let nodes = self.content.gather_nodes();
        if state.veins.len() != nodes.len()
            || state
                .veins
                .iter()
                .zip(nodes)
                .any(|(&ready, node)| ready > tick + u64::from(node.respawn_ticks))
        {
            return Err(ZoneError::new(
                "snapshot veins do not match the content veins",
            ));
        }
        if state.skinned.iter().any(|id| {
            !self
                .creatures
                .get(id)
                .is_some_and(|creature| !matches!(creature.life, Life::Alive))
        }) {
            return Err(ZoneError::new("a skinned creature is alive"));
        }
        self.gathering = state.into();
        Ok(())
    }

    /// The `Gather` intent: takes vein `node`'s yield into the bag when it is full and within
    /// reach, and spends it until it regrows.
    pub(crate) fn gather(
        &mut self,
        player_id: PlayerId,
        node: u32,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let content = Arc::clone(&self.content);
        let index = usize::try_from(node).unwrap_or(usize::MAX);
        let Some(site) = content.gather_nodes().get(index) else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        let now = self.tick + 1;
        if self.vein_depleted(index, now) {
            return Ok(Some((ErrorCode::EmptyLoot, None)));
        }
        let at = self.player_position(player_id)?;
        if !crate::reach::within_xz([at.x, at.z], site.at, GATHER_REACH_UNITS) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        if let Some(code) = self.settle_gathered(player_id, &[(site.item, site.quantity)])? {
            return Ok(Some((code, None)));
        }
        self.gathering.veins[index] = now + u64::from(site.respawn_ticks);
        Ok(None)
    }

    /// The `Skin` intent: takes a beast corpse's [`skin_yield`] once per death, for a player
    /// its loot belongs to, within loot reach. The corpse is the creature's current one: a
    /// respawn clears its skinned mark, and a living creature has nothing to skin.
    pub(crate) fn skin(
        &mut self,
        player_id: PlayerId,
        id: CreatureId,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let target = Some(EntityRef::Creature(id));
        let content = Arc::clone(&self.content);
        let Some(creature) = self.creatures.get(&id) else {
            return Ok(Some((ErrorCode::InvalidLoot, target)));
        };
        let Life::Corpse { died_at, position } = creature.life else {
            return Ok(Some((ErrorCode::InvalidLoot, target)));
        };
        if self.tick + 1 >= died_at.saturating_add(u64::from(crate::unit::CORPSE_TICKS)) {
            return Ok(Some((ErrorCode::InvalidLoot, target)));
        }
        let (_, template) = Self::creature_content(&content, id)?;
        let materials = skin_yield(template.family);
        if materials.is_empty() {
            return Ok(Some((ErrorCode::InvalidLoot, target)));
        }
        if !creature.corpse_open_to(player_id) {
            return Ok(Some((ErrorCode::NotLootOwner, target)));
        }
        if self.gathering.skinned.contains(&id) {
            return Ok(Some((ErrorCode::EmptyLoot, target)));
        }
        let from = self.player_position(player_id)?;
        if !crate::reach::within_sphere([from.x, from.y, from.z], position, crate::LOOT_REACH_UNITS)
        {
            return Ok(Some((ErrorCode::OutOfRange, target)));
        }
        if let Some(code) = self.settle_gathered(player_id, materials)? {
            return Ok(Some((code, target)));
        }
        self.gathering.skinned.insert(id);
        for &(item, _) in materials {
            self.report_collected(player_id, item);
        }
        Ok(None)
    }

    /// Puts every stack of `materials` in the player's bag, or nothing when they do not all
    /// fit; the bag revision advances with them.
    fn settle_gathered(
        &mut self,
        player_id: PlayerId,
        materials: &[(ItemId, u16)],
    ) -> Result<Option<ErrorCode>, ZoneError> {
        let player = self
            .players
            .get_mut(&player_id)
            .ok_or_else(|| ZoneError::new("gathering player is absent"))?;
        let mut bag = player.inventory.clone();
        for &(item, quantity) in materials {
            if bag.insert(item, quantity).is_err() {
                return Ok(Some(ErrorCode::InventoryFull));
            }
        }
        Ok(player.commit_bag(bag, None).err())
    }
}
