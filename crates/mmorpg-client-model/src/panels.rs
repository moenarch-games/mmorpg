//! What two HUD panels both clients draw offer (#1280): Captain Thorn's shared loot rows and the
//! waystone panel. Each button only names a command; the zone validates every roll, claim and
//! journey.
use mmorpg_core::{
    DepthsLootView, DropView, RollChoice, TICK_HZ, WAYSTONES, WaystoneId, ZoneContent,
    ZoneSnapshot, waystone, waystone_within_reach,
};

use crate::items::item_name;

/// What a loot row's button sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LootAction {
    /// A roll answer.
    Roll(RollChoice),
    /// Claim an item everyone passed on.
    Claim,
}

/// One button of a loot row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootButton {
    pub action: LootAction,
    pub label: &'static str,
    pub enabled: bool,
}

/// One drop of the shared pool as its row shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LootRow {
    pub drop: u8,
    pub item_name: &'static str,
    /// The drop's state: the whole seconds left to answer (rounded up), the viewer's answer, or
    /// how it was decided.
    pub line: String,
    pub buttons: Vec<LootButton>,
}

const fn choice_name(choice: RollChoice) -> &'static str {
    match choice {
        RollChoice::Need => "Need",
        RollChoice::Greed => "Greed",
        RollChoice::Pass => "Pass",
    }
}

/// The rows of the shared loot window (#404) at `tick`, in drop order.
#[must_use]
pub fn depths_loot_rows(view: &DepthsLootView, tick: u64) -> Vec<LootRow> {
    view.drops
        .iter()
        .map(|drop| {
            let mut buttons = Vec::new();
            let line = match drop.state {
                DropView::Rolling {
                    answer: Some(choice),
                    ..
                } => format!("You chose {}. Waiting for the others…", choice_name(choice)),
                DropView::Rolling {
                    deadline,
                    answer: None,
                    may_need,
                } => {
                    for (choice, enabled) in [
                        (RollChoice::Need, may_need),
                        (RollChoice::Greed, true),
                        (RollChoice::Pass, true),
                    ] {
                        buttons.push(LootButton {
                            action: LootAction::Roll(choice),
                            label: choice_name(choice),
                            enabled,
                        });
                    }
                    format!(
                        "{} s",
                        deadline.saturating_sub(tick).div_ceil(u64::from(TICK_HZ))
                    )
                }
                DropView::Held { claimable: true } => {
                    buttons.push(LootButton {
                        action: LootAction::Claim,
                        label: "Claim",
                        enabled: true,
                    });
                    "Everyone passed.".to_owned()
                }
                DropView::Held { claimable: false } => {
                    "Everyone passed; it stays until the run ends.".to_owned()
                }
                DropView::Awarded {
                    won: true,
                    roll,
                    need,
                    pending,
                } => {
                    let how = if roll == 0 {
                        "claimed".to_owned()
                    } else {
                        format!("{} {roll}", if need { "Need" } else { "Greed" })
                    };
                    if pending {
                        format!("Yours ({how}) — bag full or arriving…")
                    } else {
                        format!("Yours ({how})")
                    }
                }
                DropView::Awarded { won: false, .. } => "Taken by a party member.".to_owned(),
            };
            LootRow {
                drop: drop.drop,
                item_name: item_name(drop.item),
                line,
                buttons,
            }
        })
        .collect()
}

/// What the waystone panel (#145) offers at the stone in reach.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaystoneOffer {
    pub here: WaystoneId,
    pub name: &'static str,
    /// The unlocked other stones, in ID order.
    pub destinations: Vec<(WaystoneId, &'static str)>,
    /// Stones of the network the viewer has not unlocked.
    pub undiscovered: usize,
    /// The panel's footer: a hint without destinations, else the undiscovered count, else empty.
    pub status: String,
}

/// The waystone panel while the living viewer stands within reach of a stone; `None` otherwise.
/// The zone checks reach, unlocks and combat again.
#[must_use]
pub fn waystone_offer(snapshot: &ZoneSnapshot, content: &ZoneContent) -> Option<WaystoneOffer> {
    if snapshot.viewer.dead {
        return None;
    }
    let viewer = snapshot.entities.first()?;
    let here = waystone_within_reach(content, [viewer.position[0], viewer.position[2]])?;
    let unlocked = &snapshot.viewer.waystones;
    let destinations: Vec<_> = WAYSTONES
        .iter()
        .filter(|site| site.id != here && unlocked.contains(site.id))
        .map(|site| (site.id, site.name))
        .collect();
    let undiscovered = WAYSTONES
        .iter()
        .filter(|site| !unlocked.contains(site.id))
        .count();
    let status = if destinations.is_empty() {
        "Explore to discover more waystones.".to_owned()
    } else if undiscovered > 0 {
        format!("{undiscovered} undiscovered")
    } else {
        String::new()
    };
    Some(WaystoneOffer {
        here,
        name: waystone(here).map_or("Waystone", |site| site.name),
        destinations,
        undiscovered,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{DepthsDropView, ITEM_CATALOG};

    fn view(states: &[DropView]) -> DepthsLootView {
        DepthsLootView {
            run_id: 7,
            drops: states
                .iter()
                .enumerate()
                .map(|(index, state)| DepthsDropView {
                    drop: u8::try_from(index).unwrap(),
                    item: ITEM_CATALOG[0].id,
                    state: *state,
                })
                .collect(),
        }
    }

    #[test]
    fn loot_rows_count_down_rounding_up_and_name_each_state() {
        let tick_hz = u64::from(TICK_HZ);
        let rows = depths_loot_rows(
            &view(&[
                DropView::Rolling {
                    deadline: 100 + 2 * tick_hz + 1,
                    answer: None,
                    may_need: false,
                },
                DropView::Rolling {
                    deadline: 50,
                    answer: Some(RollChoice::Greed),
                    may_need: true,
                },
                DropView::Held { claimable: true },
                DropView::Held { claimable: false },
                DropView::Awarded {
                    won: true,
                    roll: 17,
                    need: true,
                    pending: true,
                },
                DropView::Awarded {
                    won: true,
                    roll: 0,
                    need: false,
                    pending: false,
                },
                DropView::Awarded {
                    won: false,
                    roll: 3,
                    need: false,
                    pending: false,
                },
            ]),
            100,
        );
        let lines: Vec<_> = rows.iter().map(|row| row.line.as_str()).collect();
        assert_eq!(
            lines,
            [
                "3 s",
                "You chose Greed. Waiting for the others…",
                "Everyone passed.",
                "Everyone passed; it stays until the run ends.",
                "Yours (Need 17) — bag full or arriving…",
                "Yours (claimed)",
                "Taken by a party member.",
            ]
        );
        let first: Vec<_> = rows[0]
            .buttons
            .iter()
            .map(|button| (button.label, button.enabled))
            .collect();
        assert_eq!(first, [("Need", false), ("Greed", true), ("Pass", true)]);
        assert_eq!(rows[2].buttons[0].action, LootAction::Claim);
        assert_eq!(rows[0].item_name, ITEM_CATALOG[0].name);
    }

    #[test]
    fn the_waystone_panel_offers_the_unlocked_other_stones() {
        let content = mmorpg_core::greyhaven_vale::content();
        let stone = WAYSTONES
            .iter()
            .find(|site| waystone_within_reach(&content, site.stone) == Some(site.id))
            .expect("a stone of the hosted zone");
        let mut view = crate::test_support::snapshot(1, 5);
        view.entities = vec![mmorpg_core::EntitySnapshot {
            kind: mmorpg_core::EntityKind::Player,
            id: 1,
            appearance: 0,
            position: [stone.stone[0], 0, stone.stone[1]],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: mmorpg_core::EntityFlags::default(),
        }];
        let offer = waystone_offer(&view, &content).unwrap();
        assert_eq!(offer.here, stone.id);
        assert!(offer.destinations.is_empty());
        assert_eq!(offer.status, "Explore to discover more waystones.");
        view.viewer.dead = true;
        assert_eq!(waystone_offer(&view, &content), None);
    }
}
