//! Radial (pie) menus for fast actions (#653, Atlas `5.2.i653`, `5.2.i653-2`): which existing
//! actions a fixed or context wheel offers, which sector a mouse direction picks, and the opt-in
//! local usage counters. A wheel only *selects* an action that already has a key; each client then
//! runs it exactly as that key would, and the zone validates every resulting command. The mouse
//! owner ([`crate::mouse_owner`]) holds the wheel's open/pick/cancel lifecycle.
use std::collections::BTreeMap;

use mmorpg_core::{
    AbilityId, Emote, EntityKind, EntityRef, PlayerClass, ZoneContent, ZoneSnapshot, ability_by_id,
};
use strum::VariantArray;

use crate::hud::{ACTION_BAR_SLOTS, class_kit};
use crate::mouse_owner::{GESTURE_THRESHOLD_PX, MouseOwner};

/// The most sectors a wheel has: eight directions, north first, clockwise.
pub const WHEEL_SECTORS: usize = 8;

/// A release of the wheel's key sooner than this, still in the centre, is a tap: the wheel stays
/// open for a left click to confirm (tap-to-open, click-to-confirm).
pub const WHEEL_TAP_MS: f64 = 250.0;

/// Which wheel a key opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WheelKind {
    /// The player's own eight sectors (`controls.wheel_sector_1..8`), stable directions.
    Fixed,
    /// The actions the current target or situation supports.
    Context,
}

impl WheelKind {
    const fn index(self) -> usize {
        match self {
            Self::Fixed => 0,
            Self::Context => 1,
        }
    }
}

/// A minion group order (#295), as the `minions.*` actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, VariantArray)]
pub enum MinionCall {
    Attack,
    Defend,
    Follow,
    Retreat,
}

/// One existing action a wheel sector runs. Each has the same id as its key binding where one
/// exists, so a wheel never invents a second copy of an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WheelAction {
    /// Action-bar slot 1–20 (`ability.slotN`).
    Slot(u8),
    /// `combat.swing`.
    Swing,
    /// `move.roll`.
    Roll,
    /// `target.next`.
    TargetNext,
    /// `target.clear`: the selection goes (no key; a focused click on empty world does it too).
    ClearTarget,
    /// `ui.talk` (native `world.interact`): talk to the selected NPC or use what is in reach.
    Interact,
    /// `ui.toggleVendor`: the selected vendor's wares (native: its NPC context).
    Trade,
    /// `minions.*`.
    Minions(MinionCall),
    /// `emote.*`: the emote chat commands.
    Emote(Emote),
}

impl WheelAction {
    /// Every action a fixed sector may hold, in settings order.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let slots = (1..=ACTION_BAR_SLOTS).filter_map(|slot| u8::try_from(slot).ok());
        slots
            .map(Self::Slot)
            .chain([
                Self::Swing,
                Self::Roll,
                Self::TargetNext,
                Self::ClearTarget,
                Self::Interact,
                Self::Trade,
            ])
            .chain(MinionCall::VARIANTS.iter().copied().map(Self::Minions))
            .chain(Emote::VARIANTS.iter().copied().map(Self::Emote))
            .collect()
    }

    /// The semantic id, also the fixed-sector settings choice.
    #[must_use]
    pub fn id(self) -> String {
        match self {
            Self::Slot(slot) => format!("ability.slot{slot}"),
            Self::Swing => "combat.swing".into(),
            Self::Roll => "move.roll".into(),
            Self::TargetNext => "target.next".into(),
            Self::ClearTarget => "target.clear".into(),
            Self::Interact => "ui.talk".into(),
            Self::Trade => "ui.toggleVendor".into(),
            Self::Minions(call) => format!("minions.{}", call_name(call)),
            Self::Emote(emote) => format!("emote.{}", emote_name(emote)),
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|action| action.id() == id)
    }

    /// A short sector label: a slot shows the ability it holds for the viewer's class.
    #[must_use]
    pub fn label(self, class: Option<PlayerClass>) -> String {
        match self {
            Self::Slot(slot) => class
                .and_then(|class| class_kit(class).get(usize::from(slot) - 1).copied())
                .and_then(|ability| ability_by_id(AbilityId::new(ability)))
                .map_or_else(|| format!("Ability {slot}"), |ability| ability.name.into()),
            Self::Swing => "Swing".into(),
            Self::Roll => "Dodge roll".into(),
            Self::TargetNext => "Next target".into(),
            Self::ClearTarget => "Clear target".into(),
            Self::Interact => "Talk / use".into(),
            Self::Trade => "Trade".into(),
            Self::Minions(MinionCall::Attack) => "Minions: attack".into(),
            Self::Minions(MinionCall::Defend) => "Minions: defend".into(),
            Self::Minions(MinionCall::Follow) => "Minions: follow".into(),
            Self::Minions(MinionCall::Retreat) => "Minions: retreat".into(),
            Self::Emote(emote) => {
                let name = emote_name(emote);
                name[..1].to_uppercase() + &name[1..]
            }
        }
    }
}

const fn call_name(call: MinionCall) -> &'static str {
    match call {
        MinionCall::Attack => "attack",
        MinionCall::Defend => "defend",
        MinionCall::Follow => "follow",
        MinionCall::Retreat => "retreat",
    }
}

const fn emote_name(emote: Emote) -> &'static str {
    match emote {
        Emote::Wave => "wave",
        Emote::Bow => "bow",
        Emote::Cheer => "cheer",
        Emote::Laugh => "laugh",
        Emote::Point => "point",
    }
}

/// An open wheel's sectors. A fixed wheel always has eight, gaps included, so directions stay put;
/// a context wheel has one sector per offered action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wheel {
    pub kind: WheelKind,
    sectors: [Option<WheelAction>; WHEEL_SECTORS],
    count: u8,
}

impl Wheel {
    /// The player's fixed wheel from the eight sector settings, north first, clockwise; unknown or
    /// `"none"` choices are gaps.
    #[must_use]
    pub fn fixed(choices: &[&str]) -> Self {
        let mut sectors = [None; WHEEL_SECTORS];
        for (sector, choice) in sectors.iter_mut().zip(choices) {
            *sector = WheelAction::from_id(choice);
        }
        Self {
            kind: WheelKind::Fixed,
            sectors,
            count: WHEEL_SECTORS as u8,
        }
    }

    /// The context wheel: only actions the projection says are relevant now. The zone still
    /// decides whether each one succeeds; nothing here grants availability.
    ///
    /// - Dead: nothing; death ends any wheel (Atlas `5.2.i636-2`).
    /// - A selected NPC: talk, trade when content lists it as a vendor, clear target.
    /// - A selected attackable unit: swing, the learned class abilities that reach a target, a
    ///   Necromancer's minion attack, next target, clear target.
    /// - Nothing selected: next target, use what is in reach, a Necromancer's minion follow, and
    ///   the wave and cheer emotes.
    #[must_use]
    pub fn context(snapshot: &ZoneSnapshot, content: &ZoneContent) -> Self {
        let viewer = &snapshot.viewer;
        let class = viewer.class.map(|choice| choice.class);
        let necromancer = class == Some(PlayerClass::Necromancer);
        let mut actions = Vec::new();
        let me = EntityRef::Player(snapshot.viewer_id);
        let selected = viewer.target.filter(|target| *target != me);
        let attackable = selected.is_some_and(|target| {
            snapshot
                .entities
                .iter()
                .any(|unit| unit.entity() == target && unit.flags.attackable && !unit.flags.dead)
        });
        if viewer.dead {
            // No action: death ends a wheel anyway.
        } else if let Some(EntityRef::Npc(npc)) = selected {
            actions.push(WheelAction::Interact);
            if content.vendor_stock(npc).is_some() {
                actions.push(WheelAction::Trade);
            }
            actions.push(WheelAction::ClearTarget);
        } else if attackable {
            actions.push(WheelAction::Swing);
            if let Some(class) = class {
                for (index, ability) in class_kit(class).into_iter().enumerate() {
                    let offensive = ability_by_id(AbilityId::new(ability)).is_some_and(|ability| {
                        viewer.level >= ability.level
                            && matches!(
                                ability.range,
                                mmorpg_core::AbilityRange::Melee
                                    | mmorpg_core::AbilityRange::Units(_)
                            )
                    });
                    if offensive && let Ok(slot) = u8::try_from(index + 1) {
                        actions.push(WheelAction::Slot(slot));
                    }
                }
            }
            if necromancer {
                actions.push(WheelAction::Minions(MinionCall::Attack));
            }
            // The last two sectors always stay: next and clear.
            actions.truncate(WHEEL_SECTORS - 2);
            actions.push(WheelAction::TargetNext);
            actions.push(WheelAction::ClearTarget);
        } else {
            actions.push(WheelAction::TargetNext);
            actions.push(WheelAction::Interact);
            if necromancer {
                actions.push(WheelAction::Minions(MinionCall::Follow));
            }
            if selected.is_some_and(|target| target.kind() != EntityKind::Npc) {
                actions.push(WheelAction::ClearTarget);
            }
            actions.push(WheelAction::Emote(Emote::Wave));
            actions.push(WheelAction::Emote(Emote::Cheer));
        }
        let mut sectors = [None; WHEEL_SECTORS];
        for (sector, action) in sectors.iter_mut().zip(actions) {
            *sector = Some(action);
        }
        let count = sectors.iter().filter(|sector| sector.is_some()).count();
        Self {
            kind: WheelKind::Context,
            sectors,
            count: u8::try_from(count).unwrap_or(0),
        }
    }

    /// The sectors, north first and clockwise, gaps as `None`.
    #[must_use]
    pub fn sectors(&self) -> &[Option<WheelAction>] {
        &self.sectors[..usize::from(self.count)]
    }

    /// Whether any sector holds an action.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sectors().iter().all(Option::is_none)
    }

    /// The sector a pointer travel `[dx, dy]` (screen y down) points at, or `None` inside the
    /// central dead zone. Sector 0 is straight up; sectors run clockwise.
    #[must_use]
    pub fn sector_at(&self, offset: [f64; 2]) -> Option<usize> {
        let [dx, dy] = offset;
        let count = usize::from(self.count);
        if count == 0 || !dx.is_finite() || !dy.is_finite() || dx.hypot(dy) < GESTURE_THRESHOLD_PX {
            return None;
        }
        let turn = dx.atan2(-dy).rem_euclid(std::f64::consts::TAU);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let sector = (turn / (std::f64::consts::TAU / count as f64)).round() as usize % count;
        Some(sector)
    }

    /// The action a pointer travel picks, if it points at a filled sector.
    #[must_use]
    pub fn pick(&self, offset: [f64; 2]) -> Option<WheelAction> {
        self.sector_at(offset)
            .and_then(|sector| self.sectors[sector])
    }
}

/// Wheel events since the counters were last taken: plain counts, kept by the mouse owner so
/// both clients count exactly the same opens, picks and cancels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WheelTally {
    pub opens: [u32; 2],
    pub picks: [u32; 2],
    pub cancels: [u32; 2],
}

impl WheelTally {
    pub(crate) fn opened(&mut self, kind: WheelKind) {
        self.opens[kind.index()] = self.opens[kind.index()].saturating_add(1);
    }

    pub(crate) fn picked(&mut self, kind: WheelKind) {
        self.picks[kind.index()] = self.picks[kind.index()].saturating_add(1);
    }

    pub(crate) fn cancelled(&mut self, kind: WheelKind) {
        self.cancels[kind.index()] = self.cancels[kind.index()].saturating_add(1);
    }
}

/// The opt-in playtest usage counters (Atlas `5.2.i653-2`, setting `controls.wheel_usage`):
/// wheel opens, picks and cancels per wheel, and milliseconds spent under each mouse owner
/// (free look, focus, and each gesture). Kept locally only, as JSON the client stores in a file
/// or local storage; nothing is ever sent anywhere.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WheelUsage {
    tally: WheelTally,
    owner_ms: BTreeMap<&'static str, f64>,
}

const OWNER_NAMES: [&str; 6] = ["free_look", "focus", "strike", "guard", "aim", "wheel"];
const KIND_NAMES: [&str; 2] = ["fixed", "context"];

/// A mouse owner's stable counter name.
#[must_use]
pub const fn owner_name(owner: MouseOwner) -> &'static str {
    use crate::mouse_owner::{Gesture, Hold};
    match owner {
        MouseOwner::FreeLook => OWNER_NAMES[0],
        MouseOwner::Focus => OWNER_NAMES[1],
        MouseOwner::Gesture(Gesture::Hold(Hold::Strike)) => OWNER_NAMES[2],
        MouseOwner::Gesture(Gesture::Hold(Hold::Guard)) => OWNER_NAMES[3],
        MouseOwner::Gesture(Gesture::Aim) => OWNER_NAMES[4],
        MouseOwner::Gesture(Gesture::Wheel) => OWNER_NAMES[5],
    }
}

impl WheelUsage {
    /// Counters read back from [`Self::to_json`]; anything unreadable starts from zero.
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        let mut usage = Self::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return usage;
        };
        let count = |section: &str, kind: &str| {
            value[section][kind]
                .as_u64()
                .and_then(|count| u32::try_from(count).ok())
                .unwrap_or(0)
        };
        for (index, kind) in KIND_NAMES.iter().enumerate() {
            usage.tally.opens[index] = count("opens", kind);
            usage.tally.picks[index] = count("picks", kind);
            usage.tally.cancels[index] = count("cancels", kind);
        }
        for name in OWNER_NAMES {
            if let Some(ms) = value["owner_ms"][name].as_f64().filter(|ms| *ms > 0.0) {
                usage.owner_ms.insert(name, ms);
            }
        }
        usage
    }

    /// Adds wheel events taken from the mouse owner.
    pub fn add(&mut self, tally: WheelTally) {
        for index in 0..2 {
            self.tally.opens[index] = self.tally.opens[index].saturating_add(tally.opens[index]);
            self.tally.picks[index] = self.tally.picks[index].saturating_add(tally.picks[index]);
            self.tally.cancels[index] =
                self.tally.cancels[index].saturating_add(tally.cancels[index]);
        }
    }

    /// `ms` milliseconds passed under `owner`.
    pub fn spend(&mut self, owner: MouseOwner, ms: f64) {
        if ms.is_finite() && ms > 0.0 {
            *self.owner_ms.entry(owner_name(owner)).or_default() += ms;
        }
    }

    #[must_use]
    pub fn tally(&self) -> WheelTally {
        self.tally
    }

    /// The counters as stable, readable JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let per_kind = |counts: [u32; 2]| {
            serde_json::Value::Object(
                KIND_NAMES
                    .iter()
                    .zip(counts)
                    .map(|(kind, count)| ((*kind).to_owned(), count.into()))
                    .collect(),
            )
        };
        let owner_ms = OWNER_NAMES
            .iter()
            .map(|name| {
                let ms = self.owner_ms.get(name).copied().unwrap_or(0.0).round();
                ((*name).to_owned(), serde_json::json!(ms))
            })
            .collect();
        let value = serde_json::json!({
            "opens": per_kind(self.tally.opens),
            "picks": per_kind(self.tally.picks),
            "cancels": per_kind(self.tally.cancels),
            "owner_ms": serde_json::Value::Object(owner_ms),
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;
    use mmorpg_core::{ClassChoice, EntityFlags, EntitySnapshot, NpcId};

    #[test]
    fn a_direction_picks_its_sector_and_the_centre_picks_nothing() {
        let wheel = Wheel::fixed(&[
            "ability.slot1",
            "ability.slot2",
            "move.roll",
            "none",
            "emote.wave",
            "target.next",
            "combat.swing",
            "emote.cheer",
        ]);
        assert_eq!(wheel.sectors().len(), WHEEL_SECTORS);
        assert_eq!(wheel.pick([0.0, -40.0]), Some(WheelAction::Slot(1)));
        assert_eq!(wheel.pick([30.0, -30.0]), Some(WheelAction::Slot(2)));
        assert_eq!(wheel.pick([40.0, 0.0]), Some(WheelAction::Roll));
        assert_eq!(wheel.sector_at([30.0, 30.0]), Some(3));
        assert_eq!(wheel.pick([30.0, 30.0]), None, "a gap picks nothing");
        assert_eq!(
            wheel.pick([0.0, 40.0]),
            Some(WheelAction::Emote(Emote::Wave))
        );
        assert_eq!(wheel.pick([-40.0, 0.0]), Some(WheelAction::Swing));
        assert_eq!(wheel.sector_at([5.0, -5.0]), None, "the dead zone");
    }

    #[test]
    fn every_action_id_round_trips() {
        for action in WheelAction::all() {
            assert_eq!(WheelAction::from_id(&action.id()), Some(action));
        }
        assert_eq!(WheelAction::from_id("none"), None);
        assert_eq!(WheelAction::Emote(Emote::Wave).label(None), "Wave");
        assert_eq!(
            WheelAction::Slot(1).label(Some(PlayerClass::Warden)),
            "Heroic Strike"
        );
    }

    fn unit(kind: EntityKind, id: u32, attackable: bool) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags {
                attackable,
                ..EntityFlags::default()
            },
        }
    }

    #[test]
    fn the_context_wheel_follows_the_selection() {
        let content = mmorpg_core::greyhaven_vale::content();
        let mut view = snapshot(1, 1);
        view.viewer.class = Some(ClassChoice {
            class: PlayerClass::Warden,
            sex: mmorpg_core::Sex::Female,
        });
        view.viewer.level = 1;
        let alone = Wheel::context(&view, &content);
        assert_eq!(alone.sectors()[0], Some(WheelAction::TargetNext));
        assert!(!alone.sectors().contains(&Some(WheelAction::Swing)));

        view.entities.push(unit(EntityKind::Creature, 7, true));
        view.viewer.target = Some(EntityRef::Creature(mmorpg_core::CreatureId::new(7)));
        let fight = Wheel::context(&view, &content);
        assert_eq!(fight.sectors()[0], Some(WheelAction::Swing));
        assert!(fight.sectors().contains(&Some(WheelAction::Slot(1))));
        assert_eq!(
            fight.sectors().last(),
            Some(&Some(WheelAction::ClearTarget))
        );

        let (vendor, _) = content.vendors()[0];
        view.entities
            .push(unit(EntityKind::Npc, vendor.get(), false));
        view.viewer.target = Some(EntityRef::Npc(vendor));
        let talk = Wheel::context(&view, &content);
        assert_eq!(
            talk.sectors(),
            [
                Some(WheelAction::Interact),
                Some(WheelAction::Trade),
                Some(WheelAction::ClearTarget)
            ]
        );
        view.viewer.target = Some(EntityRef::Npc(NpcId::new(60_000)));
        assert!(
            !Wheel::context(&view, &content)
                .sectors()
                .contains(&Some(WheelAction::Trade))
        );

        view.viewer.dead = true;
        assert!(
            Wheel::context(&view, &content).is_empty(),
            "death opens no wheel"
        );
    }

    #[test]
    fn usage_round_trips_through_json() {
        let mut usage = WheelUsage::default();
        let mut tally = WheelTally::default();
        tally.opened(WheelKind::Fixed);
        tally.picked(WheelKind::Fixed);
        tally.opened(WheelKind::Context);
        tally.cancelled(WheelKind::Context);
        usage.add(tally);
        usage.spend(MouseOwner::FreeLook, 1500.0);
        usage.spend(MouseOwner::Focus, 250.0);
        let back = WheelUsage::from_json(&usage.to_json());
        assert_eq!(back.tally(), usage.tally());
        assert!(usage.to_json().contains("\"free_look\": 1500.0"));
        assert_eq!(WheelUsage::from_json("not json"), WheelUsage::default());
    }
}
