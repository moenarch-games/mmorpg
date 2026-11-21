//! What Greyhaven's residents hold on their routines (#904): `content/greyhaven/chores.json`
//! maps each resident's routine activity (its post by day, each errand, the walks home and back)
//! to a chore, a `humanoid-equipment` item on a figure socket with the clips played while it is
//! held. Presentation only: the activity follows from the routine the zone already runs and the
//! projected motion, and nothing here feeds back into the simulation. Both clients draw the
//! held item ([`super::imported::Carry::chore`]); a missing item or clip leaves the hands empty.

use super::imported::{Hand, Held};
use mmorpg_core::{NpcId, NpcRoutine, PostureView, ZoneContent, is_night, time_of_day};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

/// The chore table as authored.
const CHORES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../content/greyhaven/chores.json"
));

/// One chore: the item held, the socket it rides and the clips played while it is held. A clip
/// left out keeps the ordinary idle or walk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct Chore {
    /// The `humanoid-equipment` entry key, e.g. `equipment.tool.lantern`.
    pub item: &'static str,
    /// The figure's `humanoid-sockets` socket, e.g. `hand_r.carry` or `chest.carry`.
    pub socket: &'static str,
    /// The clip while standing.
    pub idle: Option<&'static str>,
    /// The clip while walking.
    pub walk: Option<&'static str>,
}

impl Chore {
    /// The hand whose fingers close round the item and the grip they take: a staff's shaft on a
    /// `.staff` socket, a handle otherwise; none for a load hugged to the chest.
    #[must_use]
    pub fn grip(self) -> Option<(Hand, Held)> {
        let hand = if self.socket.starts_with("hand_l.") {
            Hand::Left
        } else if self.socket.starts_with("hand_r.") {
            Hand::Right
        } else {
            return None;
        };
        let held = if self.socket.ends_with(".staff") {
            Held::Staff
        } else {
            Held::Weapon
        };
        Some((hand, held))
    }
}

/// What a routine resident is doing now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity {
    /// At its post by day.
    Work,
    /// On the routine's errand of this index: walking out, staying or walking back.
    Errand(usize),
    /// Walking home at dusk or back to work at dawn.
    Commute,
    /// Home for the night.
    Home,
}

impl Activity {
    /// The activity at `time` (ticks since midnight) of a resident on `routine` working at
    /// `work`, moving or not by its projected velocity. Mirrors [`NpcRoutine::placement`]: at
    /// night it walks home or is home; by day it runs an errand within the errand's span,
    /// otherwise it walks back from home or is at work.
    #[must_use]
    pub fn of(routine: &NpcRoutine, work: [i32; 2], time: u32, moving: bool) -> Self {
        if is_night(time) {
            return if moving { Self::Commute } else { Self::Home };
        }
        if let Some(index) = routine.errands.iter().position(|errand| {
            time.checked_sub(errand.leave)
                .is_some_and(|since| since < errand.span(work))
        }) {
            return Self::Errand(index);
        }
        if moving { Self::Commute } else { Self::Work }
    }
}

/// A hunter's chores on its outing (#991): stalking, carrying its catch home and hurt.
#[derive(Debug, Default, Deserialize)]
struct Hunt {
    hunting: Option<&'static str>,
    carrying: Option<&'static str>,
    hurt: Option<&'static str>,
}

/// One resident's day: its chore at work and on each errand, by chore name.
#[derive(Debug, Deserialize)]
struct Day {
    work: Option<&'static str>,
    errands: Vec<Option<&'static str>>,
}

/// The chore table: chores by name, the walks' chore and each resident's day by NPC name.
#[derive(Debug, Deserialize)]
pub struct Chores {
    chores: BTreeMap<&'static str, Chore>,
    night: Option<&'static str>,
    /// What a hunter holds on its outing (#991), by its projected posture.
    #[serde(default)]
    hunt: Hunt,
    residents: BTreeMap<&'static str, Day>,
}

impl Chores {
    /// The authored table, parsed once.
    ///
    /// # Panics
    /// When `content/greyhaven/chores.json` does not parse.
    #[must_use]
    pub fn get() -> &'static Self {
        static CHORES_TABLE: OnceLock<Chores> = OnceLock::new();
        CHORES_TABLE.get_or_init(|| {
            serde_json::from_str(CHORES).expect("chores.json is resident-chores-v1")
        })
    }

    /// The chore the resident named `name` holds while doing `activity`, if any.
    #[must_use]
    pub fn of(&self, name: &str, activity: Activity) -> Option<Chore> {
        let day = self.residents.get(name)?;
        let chore = match activity {
            Activity::Work => day.work,
            Activity::Errand(index) => day.errands.get(index).copied().flatten(),
            Activity::Commute => self.night,
            Activity::Home => None,
        }?;
        self.chores.get(chore).copied()
    }

    /// What a hunter in `posture` holds on its outing (#991), or `None` off it: the posture
    /// then leaves the routine's chore in place.
    #[must_use]
    pub fn hunting(&self, posture: PostureView) -> Option<Option<Chore>> {
        let chore = match posture {
            PostureView::Hunting => self.hunt.hunting,
            PostureView::Carrying => self.hunt.carrying,
            PostureView::Hurt => self.hunt.hurt,
            // A merchant pushing its handcart (#992) keeps both hands on the shafts.
            PostureView::Hauling => return Some(None),
            PostureView::Free
            | PostureView::Sitting
            | PostureView::Asleep
            | PostureView::Waking => return None,
        };
        Some(chore.and_then(|chore| self.chores.get(chore).copied()))
    }

    /// Every chore, for a client that loads the items ahead.
    pub fn items(&self) -> impl Iterator<Item = &Chore> {
        self.chores.values()
    }

    /// The chore NPC `npc` of `content` holds at zone `tick`, moving or not: none for an NPC
    /// without a routine or a day in the table.
    #[must_use]
    pub fn resident(
        &self,
        content: &ZoneContent,
        npc: NpcId,
        tick: u64,
        moving: bool,
    ) -> Option<Chore> {
        let definition = content.npc(npc)?;
        let routine = content.routine(npc)?;
        let activity = Activity::of(routine, definition.position, time_of_day(tick), moving);
        self.of(&definition.name, activity)
    }
}

#[cfg(test)]
mod tests {
    use super::{Activity, Chores};
    use mmorpg_core::{DAWN, DUSK, HOUR_TICKS};

    #[test]
    fn routine_activities_map_to_their_items_and_sockets() {
        let chores = Chores::get();
        let item = |name: &str, activity| chores.of(name, activity).map(|c| (c.item, c.socket));
        // A lantern walking home after dusk, a bucket to the well, a tankard at the inn, a
        // washing basket against the chest, a broom at work, and empty hands at home.
        assert_eq!(
            item("Weaver Lysa Reed", Activity::Commute),
            Some(("equipment.tool.lantern", "hand_r.carry"))
        );
        assert_eq!(
            item("Weaver Lysa Reed", Activity::Errand(0)),
            Some(("equipment.tool.bucket", "hand_r.carry"))
        );
        assert_eq!(
            item("Peddler Rowan", Activity::Errand(1)),
            Some(("equipment.tool.tankard", "hand_r.carry"))
        );
        assert_eq!(
            item("Washer Elin", Activity::Errand(0)),
            Some(("equipment.load.basket-large", "chest.carry"))
        );
        assert_eq!(
            chores.of("Old Maud", Activity::Work).and_then(|c| c.idle),
            Some("Sweep_Loop")
        );
        assert_eq!(item("Weaver Lysa Reed", Activity::Work), None);
        assert_eq!(item("Weaver Lysa Reed", Activity::Home), None);
        assert_eq!(item("Greyhaven Guard", Activity::Commute), None);

        // Every resident in the table is a routine NPC with one entry per authored errand, and
        // every chore it names exists.
        let content = mmorpg_core::greyhaven_vale::content();
        for (name, day) in &chores.residents {
            let npc = content
                .npcs()
                .iter()
                .find(|npc| npc.name == *name)
                .unwrap_or_else(|| panic!("{name} is a Greyhaven NPC"));
            let routine = content
                .routine(npc.id)
                .unwrap_or_else(|| panic!("{name} has a routine"));
            assert_eq!(day.errands.len(), routine.errands.len(), "{name}'s errands");
            for chore in day.errands.iter().chain([&day.work]).flatten() {
                assert!(chores.chores.contains_key(chore), "{name}'s chore {chore}");
            }
        }
        assert!(
            chores
                .night
                .is_none_or(|night| chores.chores.contains_key(night))
        );

        // The routine's clock picks the activity: Lysa's well errand by day, the walk at dusk,
        // home after it and at work otherwise.
        let lysa = content
            .npcs()
            .iter()
            .find(|npc| npc.name == "Weaver Lysa Reed")
            .expect("Lysa");
        let routine = content.routine(lysa.id).expect("Lysa's routine");
        let leave = routine.errands[0].leave;
        let at = |time, moving| Activity::of(routine, lysa.position, time, moving);
        assert_eq!(at(leave + 10, true), Activity::Errand(0));
        assert_eq!(at(DUSK + 10, true), Activity::Commute);
        assert_eq!(at(DUSK + HOUR_TICKS, false), Activity::Home);
        assert_eq!(at(DAWN + 10, true), Activity::Commute);
        assert_eq!(at(leave - 10, false), Activity::Work);
    }
}
