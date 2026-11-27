//! What the Interact key does (#1252): one ladder both clients and the browser's touch button
//! read, so a press means the same thing everywhere.
//!
//! The order, first match wins:
//!
//! 1. work the glyph in reach (#1019), even with the weapon drawn: a ward's weak point is
//!    bared between blows;
//! 2. on a forewarned evening, read the tracks, report them to the lookout or brace the barn
//!    in reach (#532), even with the lookout selected;
//! 3. inspect the quest evidence in reach an active quest needs (#1019), even beside its giver;
//! 4. lift the viewer's set-down load, or a delivery's load at its source, in reach (#1077);
//! 5. at a selected NPC, perform its open quest action (#1019), otherwise talk to it;
//! 6. skin the selected corpse in reach that has something to skin (#917);
//! 7. with a load in both arms, set it down (#1077);
//! 8. swing the door in reach (#158), 9. take or leave the seat in reach (#619), 10. repair a
//!    damaged structure piece in reach (#305), 11. help the night raid's aftermath in reach
//!    (#531), 12. mine the full ore vein in reach (#917);
//! 13. talk to the nearest NPC in quest or vendor reach.
//!
//! Every step but the glyph needs free hands: with the weapon drawn, Interact asks to sheathe
//! it instead. Nothing here decides an outcome; the zone checks reach and everything else again.

use mmorpg_core::{
    EntityKind, EntityRef, EntitySnapshot, NpcId, QuestEntry, ZoneCommand, ZoneContent,
    ZoneSnapshot,
};

/// What the refusal says while the weapon is drawn.
pub const SHEATHE_FIRST: &str = "Sheathe your weapon to interact (H)";

/// One thing Interact can do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Interaction {
    /// Send this command; the zone checks it again.
    Command(ZoneCommand),
    /// Talk to `npc`: open its dialog or trade. `selected` says the viewer already selected it;
    /// otherwise the client selects it first.
    Talk { npc: NpcId, selected: bool },
}

impl Interaction {
    /// Whether it needs a sheathed weapon: everything but working a glyph (#1019).
    #[must_use]
    pub const fn needs_free_hands(&self) -> bool {
        !matches!(self, Self::Command(ZoneCommand::Operate { .. }))
    }

    /// A short verb for a button that does it, such as the browser's touch interact button.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Talk { .. } => "Talk",
            Self::Command(command) => match command {
                ZoneCommand::Operate { .. } => "Work",
                ZoneCommand::RaidPrepare { prep } => match prep {
                    mmorpg_core::RaidPrep::Investigate => "Read",
                    mmorpg_core::RaidPrep::Report => "Report",
                    mmorpg_core::RaidPrep::Brace(_) => "Brace",
                },
                ZoneCommand::Inspect { .. } => "Inspect",
                ZoneCommand::Lift { .. } => "Lift",
                ZoneCommand::Skin { .. } => "Skin",
                ZoneCommand::SetDown => "Set down",
                ZoneCommand::UseDoor { open: true, .. } => "Open",
                ZoneCommand::UseDoor { open: false, .. } => "Close",
                ZoneCommand::Sit { .. } => "Sit",
                ZoneCommand::Repair { .. } => "Repair",
                ZoneCommand::RaidAid { .. } => "Help",
                ZoneCommand::Gather { .. } => "Mine",
                _ => "Use",
            },
        }
    }
}

/// What one press of Interact does now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Press {
    Interact(Interaction),
    /// The weapon is drawn and the next interaction needs free hands, or there is none:
    /// show [`SHEATHE_FIRST`].
    SheatheFirst,
    Nothing,
}

/// What a press of Interact does for the viewer of `snapshot` with the active quests `quests`:
/// [`next_interaction`], unless the drawn weapon refuses it.
#[must_use]
pub fn press(snapshot: &ZoneSnapshot, content: &ZoneContent, quests: &[QuestEntry]) -> Press {
    match next_interaction(snapshot, content, quests) {
        Some(next) if !(snapshot.viewer.weapon_drawn && next.needs_free_hands()) => {
            Press::Interact(next)
        }
        _ if snapshot.viewer.weapon_drawn => Press::SheatheFirst,
        _ => Press::Nothing,
    }
}

/// The interaction Interact performs for the viewer of `snapshot` with the active quests
/// `quests` and free hands, in the module's order; `None` with nothing in reach.
#[must_use]
pub fn next_interaction(
    snapshot: &ZoneSnapshot,
    content: &ZoneContent,
    quests: &[QuestEntry],
) -> Option<Interaction> {
    let viewer = snapshot
        .entities
        .iter()
        .find(|entity| entity.entity() == EntityRef::Player(snapshot.viewer_id))?;
    let at = viewer.position;
    let command = |command| Some(Interaction::Command(command));
    if let Some(mechanism) = crate::quest::mechanism_in_reach(content, at) {
        return command(ZoneCommand::Operate {
            mechanism: mechanism.id.get(),
        });
    }
    if let Some(prep) = raid_prep(snapshot, content, at) {
        return command(ZoneCommand::RaidPrepare { prep });
    }
    if let Some(spot) = crate::quest::evidence_in_reach(content, quests, at) {
        return command(ZoneCommand::Inspect { spot: spot.get() });
    }
    let carrying = viewer.flags.load.is_some();
    if let Some(lift) = crate::quest::lift_in_reach(content, quests, at, carrying, &snapshot.loads)
    {
        return command(lift);
    }
    match snapshot.viewer.target {
        Some(EntityRef::Npc(npc)) => {
            return Some(
                if crate::quest::quest_action_at(content, quests, npc).is_some() {
                    Interaction::Command(ZoneCommand::QuestAction { npc })
                } else {
                    Interaction::Talk {
                        npc,
                        selected: true,
                    }
                },
            );
        }
        Some(EntityRef::Creature(creature)) if skinnable(snapshot, content, at, creature) => {
            return command(ZoneCommand::Skin { creature });
        }
        _ => {}
    }
    if carrying {
        return command(ZoneCommand::SetDown);
    }
    if let Some(door) = content.doors().iter().find(|door| door.within_reach(at)) {
        let open = snapshot
            .doors
            .iter()
            .any(|view| view.door == door.id && view.open);
        return command(ZoneCommand::UseDoor {
            door: door.id,
            open: !open,
        });
    }
    if let Some(seat) = content.seat_in_reach([at[0], at[2]]) {
        return command(ZoneCommand::Sit {
            seat: Some(seat.id),
        });
    }
    if let Some(piece) = snapshot.pieces.iter().find(|view| {
        content
            .pieces()
            .iter()
            .any(|piece| piece.id == view.piece && piece.within_repair_reach(at))
    }) {
        return command(ZoneCommand::Repair {
            piece: Some(piece.piece),
        });
    }
    if let Some(task) = raid_aid(snapshot, content, at) {
        return command(ZoneCommand::RaidAid { task });
    }
    if let Some(node) = (0_u32..)
        .zip(content.gather_nodes())
        .find_map(|(index, node)| {
            (!snapshot.depleted_veins.contains(&index)
                && near(node.at, at, mmorpg_core::GATHER_REACH_UNITS))
            .then_some(index)
        })
    {
        return command(ZoneCommand::Gather { node });
    }
    nearest_npc(snapshot, at).map(|npc| Interaction::Talk {
        npc,
        selected: false,
    })
}

/// Whether `at` lies within `reach` of `spot` horizontally, as the zone measures its planar
/// reaches. TODO(#1253): use core's shared planar reach once it exports one.
fn near(spot: [i32; 2], at: [i32; 3], reach: i32) -> bool {
    let (dx, dz) = (
        i64::from(spot[0]) - i64::from(at[0]),
        i64::from(spot[1]) - i64::from(at[2]),
    );
    dx * dx + dz * dz <= i64::from(reach).pow(2)
}

/// The selected `creature` is a corpse in loot reach with something to skin, as core's skin
/// yield says; the zone checks the tap and the rest.
fn skinnable(
    snapshot: &ZoneSnapshot,
    content: &ZoneContent,
    at: [i32; 3],
    creature: mmorpg_core::CreatureId,
) -> bool {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.entity() == EntityRef::Creature(creature))
        .is_some_and(|corpse| {
            let distance: i64 = at
                .iter()
                .zip(corpse.position)
                .map(|(a, b)| (i64::from(*a) - i64::from(b)).pow(2))
                .sum();
            corpse.flags.dead
                && content
                    .creature_template(mmorpg_core::CreatureTemplateId::new(corpse.appearance))
                    .is_some_and(|template| !mmorpg_core::skin_yield(template.family).is_empty())
                && distance <= i64::from(mmorpg_core::LOOT_REACH_UNITS).pow(2)
        })
}

/// The night raid preparation step (#532) feet at `at` can take on a forewarned evening:
/// reporting read tracks to the lookout beside them, reading the tracks in reach, or bracing a
/// barn wall in reach once the guards are warned.
fn raid_prep(
    snapshot: &ZoneSnapshot,
    content: &ZoneContent,
    at: [i32; 3],
) -> Option<mmorpg_core::RaidPrep> {
    use mmorpg_core::raid::{BRACE_REACH_UNITS, TRACKS_REACH_UNITS};
    let signal = snapshot.raid.as_ref().filter(|signal| signal.signs)?;
    let site = content.raid()?;
    let lookout = snapshot
        .entities
        .iter()
        .find(|entity| entity.entity() == EntityRef::Npc(site.lookout))
        .map(|entity| [entity.position[0], entity.position[2]]);
    if signal.scouted
        && !signal.warned
        && lookout.is_some_and(|spot| near(spot, at, mmorpg_core::QUEST_REACH_UNITS))
    {
        return Some(mmorpg_core::RaidPrep::Report);
    }
    if !signal.scouted && near(site.tracks, at, TRACKS_REACH_UNITS) {
        return Some(mmorpg_core::RaidPrep::Investigate);
    }
    if !signal.warned {
        return None;
    }
    (0_u32..)
        .zip(&site.braces)
        .find(|&(index, &spot)| {
            !signal.braced.contains(&index) && near(spot, at, BRACE_REACH_UNITS)
        })
        .map(|(index, _)| mmorpg_core::RaidPrep::Brace(index))
}

/// The first open aftermath task (#531) feet at `at` reach, at the raid site whose signal
/// reaches them (#1019).
fn raid_aid(snapshot: &ZoneSnapshot, content: &ZoneContent, at: [i32; 3]) -> Option<u32> {
    let open = &snapshot.raid.as_ref()?.aid;
    let (_, site) = content.raid_near([at[0], at[2]])?;
    (0_u32..)
        .zip(site.tasks())
        .find(|(index, task)| {
            open.contains(index) && near(task.at, at, mmorpg_core::raid::AID_REACH_UNITS)
        })
        .map(|(index, _)| index)
}

/// The nearest projected NPC within quest or vendor reach of `at`, the lower ID on a tie.
fn nearest_npc(snapshot: &ZoneSnapshot, at: [i32; 3]) -> Option<NpcId> {
    let reach = mmorpg_core::QUEST_REACH_UNITS.max(mmorpg_core::VENDOR_REACH_UNITS);
    let distance = |entity: &EntitySnapshot| {
        let (dx, dz) = (
            i64::from(entity.position[0]) - i64::from(at[0]),
            i64::from(entity.position[2]) - i64::from(at[2]),
        );
        dx * dx + dz * dz
    };
    snapshot
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Npc)
        .map(|entity| (distance(entity), entity.id))
        .filter(|&(distance, _)| distance <= i64::from(reach).pow(2))
        .min()
        .map(|(_, id)| NpcId::new(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureFamily, EntityFlags, greyhaven_vale};

    const VIEWER: u32 = 7;

    fn unit(kind: EntityKind, id: u32, position: [i32; 3]) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id,
            appearance: 0,
            position,
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }
    }

    /// A projection of the viewer alone at `at`.
    fn viewer_at(at: [i32; 3]) -> ZoneSnapshot {
        let mut snapshot = crate::test_support::snapshot(VIEWER, 1);
        snapshot.entities = vec![unit(EntityKind::Player, VIEWER, at)];
        snapshot
    }

    #[test]
    fn a_full_vein_in_reach_is_mined_and_a_spent_one_is_not() {
        let content = greyhaven_vale::content();
        let node = &content.gather_nodes()[0];
        let mut snapshot = viewer_at([node.at[0] + 100, 0, node.at[1]]);
        let next = next_interaction(&snapshot, &content, &[]);
        assert_eq!(
            next,
            Some(Interaction::Command(ZoneCommand::Gather { node: 0 }))
        );
        assert_eq!(next.map(|next| next.label()), Some("Mine"));
        snapshot.depleted_veins = vec![0];
        assert_ne!(
            next_interaction(&snapshot, &content, &[]),
            Some(Interaction::Command(ZoneCommand::Gather { node: 0 }))
        );
    }

    #[test]
    fn a_drawn_weapon_refuses_all_but_a_glyph() {
        let content = greyhaven_vale::content();
        let node = &content.gather_nodes()[0];
        let mut snapshot = viewer_at([node.at[0], 0, node.at[1]]);
        snapshot.viewer.weapon_drawn = true;
        assert_eq!(press(&snapshot, &content, &[]), Press::SheatheFirst);
        let mechanism = &content.mechanisms()[0];
        let mut snapshot = viewer_at([
            mechanism.at[0],
            mechanism.at[1] + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1],
            mechanism.at[2],
        ]);
        snapshot.viewer.weapon_drawn = true;
        assert_eq!(
            press(&snapshot, &content, &[]),
            Press::Interact(Interaction::Command(ZoneCommand::Operate {
                mechanism: mechanism.id.get()
            }))
        );
        // Nothing in reach still asks to sheathe, as both clients always did.
        let mut nowhere = viewer_at([i32::MAX / 2, 0, i32::MAX / 2]);
        nowhere.viewer.weapon_drawn = true;
        assert_eq!(press(&nowhere, &content, &[]), Press::SheatheFirst);
        nowhere.viewer.weapon_drawn = false;
        assert_eq!(press(&nowhere, &content, &[]), Press::Nothing);
    }

    #[test]
    fn a_selected_npc_is_talked_to_before_a_door_and_a_near_one_after_it() {
        let content = greyhaven_vale::content();
        let door = &content.doors()[0];
        let at = door.closed.position;
        let npc = content.npcs()[0].id;
        let mut snapshot = viewer_at(at);
        snapshot
            .entities
            .push(unit(EntityKind::Npc, npc.get(), [at[0] + 50, at[1], at[2]]));
        let opens = Some(Interaction::Command(ZoneCommand::UseDoor {
            door: door.id,
            open: true,
        }));
        assert_eq!(next_interaction(&snapshot, &content, &[]), opens);
        assert_eq!(opens.map(|next| next.label()), Some("Open"));
        snapshot.viewer.target = Some(EntityRef::Npc(npc));
        assert_eq!(
            next_interaction(&snapshot, &content, &[]),
            Some(Interaction::Talk {
                npc,
                selected: true
            })
        );
        // Away from the door, the near NPC is talked to unselected.
        let mut alone = viewer_at([i32::MAX / 2, 0, i32::MAX / 2]);
        alone.entities.push(unit(
            EntityKind::Npc,
            npc.get(),
            [i32::MAX / 2 + 300, 0, i32::MAX / 2],
        ));
        assert_eq!(
            next_interaction(&alone, &content, &[]),
            Some(Interaction::Talk {
                npc,
                selected: false
            })
        );
    }

    #[test]
    fn a_selected_corpse_is_skinned_only_when_its_family_yields_a_skin() {
        let content = greyhaven_vale::content();
        let far = [i32::MAX / 2, 0, i32::MAX / 2];
        let corpse_of = |family: CreatureFamily| {
            let template = content
                .creature_templates()
                .iter()
                .find(|template| template.family == family)
                .expect("the family has a template");
            let mut snapshot = viewer_at(far);
            let mut corpse = unit(EntityKind::Creature, 40, [far[0] + 100, 0, far[2]]);
            corpse.appearance = template.id.get();
            corpse.flags.dead = true;
            corpse.health_percent = 0;
            snapshot.entities.push(corpse);
            snapshot.viewer.target = Some(corpse_ref());
            next_interaction(&snapshot, &content, &[])
        };
        assert_eq!(
            corpse_of(CreatureFamily::Boar),
            Some(Interaction::Command(ZoneCommand::Skin {
                creature: mmorpg_core::CreatureId::new(40)
            }))
        );
        assert_eq!(corpse_of(CreatureFamily::Wolf), None);
    }

    fn raid(signs: bool) -> mmorpg_core::RaidView {
        mmorpg_core::RaidView {
            phase: 0,
            integrity: 100,
            aid: std::collections::BTreeSet::new(),
            signs,
            warned: false,
            braced: std::collections::BTreeSet::new(),
            scouted: false,
            standing: None,
        }
    }

    #[test]
    fn a_forewarned_evening_reads_the_tracks_reports_them_and_braces_the_barn() {
        use mmorpg_core::RaidPrep;
        let content = greyhaven_vale::content();
        let site = content.raid().expect("Greyhaven has a night raid");
        let prep = |signal: &mmorpg_core::RaidView, at: [i32; 2], lookout: Option<[i32; 2]>| {
            let mut snapshot = viewer_at([at[0], 0, at[1]]);
            if let Some([x, z]) = lookout {
                snapshot
                    .entities
                    .push(unit(EntityKind::Npc, site.lookout.get(), [x, 0, z]));
            }
            snapshot.raid = Some(signal.clone());
            match next_interaction(&snapshot, &content, &[]) {
                Some(Interaction::Command(ZoneCommand::RaidPrepare { prep })) => Some(prep),
                _ => None,
            }
        };
        let evening = raid(true);
        let [x, z] = site.tracks;
        assert_eq!(
            prep(&evening, [x, z - 150], None),
            Some(RaidPrep::Investigate)
        );
        assert_eq!(prep(&evening, [x, z - 350], None), None);
        assert_eq!(prep(&raid(false), [x, z], None), None);
        let scouted = mmorpg_core::RaidView {
            scouted: true,
            ..evening
        };
        let far = [i32::MAX / 2, i32::MAX / 2];
        assert_eq!(
            prep(&scouted, far, Some([far[0], far[1] + 200])),
            Some(RaidPrep::Report)
        );
        assert_eq!(prep(&scouted, [x, z], None), None);
        let warned = mmorpg_core::RaidView {
            warned: true,
            ..scouted
        };
        let [bx, bz] = site.braces[0];
        assert_eq!(prep(&warned, [bx, bz + 50], None), Some(RaidPrep::Brace(0)));
        let braced = mmorpg_core::RaidView {
            braced: [0].into(),
            ..warned
        };
        assert_ne!(prep(&braced, [bx, bz + 50], None), Some(RaidPrep::Brace(0)));
    }

    #[test]
    fn an_open_aid_task_in_reach_is_helped_at_the_site_whose_signal_reaches() {
        let content = greyhaven_vale::content();
        for site in content.raids().iter().filter(|site| !site.aid.is_empty()) {
            let task = site.tasks()[0].at;
            let aid = |open: &[u32], at: [i32; 2]| {
                let mut snapshot = viewer_at([at[0], 0, at[1]]);
                snapshot.raid = Some(mmorpg_core::RaidView {
                    aid: open.iter().copied().collect(),
                    ..raid(false)
                });
                next_interaction(&snapshot, &content, &[])
            };
            let helps = Some(Interaction::Command(ZoneCommand::RaidAid { task: 0 }));
            assert_eq!(aid(&[0], [task[0], task[1] + 250]), helps, "{}", site.name);
            assert_ne!(aid(&[0], [task[0], task[1] + 350]), helps, "{}", site.name);
            assert_ne!(aid(&[], task), helps, "{}", site.name);
        }
    }
    fn corpse_ref() -> EntityRef {
        EntityRef::Creature(mmorpg_core::CreatureId::new(40))
    }
}
