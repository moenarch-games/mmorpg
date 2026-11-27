//! Where each basic strike's clip carries the weapon (#1249): the blade sweeps `sim-combat`
//! places every strike's reach, arc and height band by, measured here from the shipped clips
//! so the zone hits where the weapon is drawn.
//!
//! A strike is the clip both clients play for it ([`melee_clips`]) on the Ranger figure at a
//! player's drawn height, facing +Z, striking with the joint its clip records for its contact:
//! the weapon that hand holds in its family's loadout, else the fist, else the joint itself.
//! The sweep covers the clip from 0.1 s before that contact to 0.05 s after: the farthest the
//! striking end gets from the body's vertical axis, how far it turns from the facing while
//! ahead and past half that distance, and its lowest and highest point above the feet.
use super::*;
use mmorpg_core::{StrikeVariant, WeaponFamily};
use mmorpg_presentation::units::imported::melee_clips;

const TABLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../sim-combat/src/melee/sweeps.rs"
);
/// A player's drawn height, in metres (`PLAYER_HALF_EXTENTS_UNITS` up, twice).
const HEIGHT: f32 = 1.8;
/// How far past the knuckles an empty fist strikes, in metres.
const FIST: f32 = 0.08;
/// The measured window around the contact, in seconds.
const BEFORE: f32 = 0.1;
const AFTER: f32 = 0.05;
/// A joint striking with itself.
const IDENTITY: M4 = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

/// The loadout whose weapons `family` strikes with; `None` strikes with fists and the body.
fn loadout(family: WeaponFamily) -> Option<Loadout> {
    match family {
        WeaponFamily::Sword => Some(Loadout::SwordAndShield),
        WeaponFamily::Mace => Some(Loadout::FlangedMace),
        WeaponFamily::Dagger => Some(Loadout::TwinDaggers),
        WeaponFamily::Staff => Some(Loadout::GnarledStaff),
        WeaponFamily::Spear => Some(Loadout::Spear),
        WeaponFamily::TwoHanded => Some(Loadout::Greatsword),
        // The Ranger's bow rests on its back through a bash; it strikes as a fist does.
        WeaponFamily::Unarmed | WeaponFamily::Bow => None,
    }
}

/// The recorded contact of every clip that has one: its striking joint and seconds in.
fn contacts() -> BTreeMap<String, (String, f32)> {
    let clips: serde_json::Value =
        serde_json::from_slice(ranger_file(ranger_bundle::MOTIONS)).expect("clips JSON");
    clips["clips"]
        .as_array()
        .expect("a clip list")
        .iter()
        .filter_map(|clip| {
            let contact = &clip["contact"];
            let joint = contact["joint"].as_str()?.to_owned();
            let seconds = contact["seconds"].as_f64()? as f32;
            Some((clip["name"].as_str()?.to_owned(), (joint, seconds)))
        })
        .collect()
}

/// `family`'s `variant`: the clip measured and its sweep's tip, half arc, low and high.
fn measure(
    family: WeaponFamily,
    variant: StrikeVariant,
    contacts: &BTreeMap<String, (String, f32)>,
) -> (&'static str, [i32; 4]) {
    let character = &asset(Kind::Ranger).model;
    let clip = melee_clips(family, variant)
        .into_iter()
        .find(|&clip| character.clips.contains_key(clip))
        .unwrap_or_else(|| panic!("no clip for {family:?} {variant:?}"));
    let duration = character.clips[clip].duration;
    let (striker, contact) = contacts
        .get(clip)
        .cloned()
        .unwrap_or_else(|| panic!("{clip} records no contact"));
    // What strikes, as a joint's frame and two ends along it: the weapon the contact's hand
    // holds, else that hand's fist, else the joint itself (a shoulder charge).
    let descriptor: serde_json::Value =
        serde_json::from_slice(ranger_file(ranger_bundle::SOCKETS)).expect("sockets");
    let item = loadout(family).and_then(|loadout| {
        Equipment::loadout(loadout)
            .items
            .iter()
            .find(|(_, joint, _)| *joint == striker)
    });
    let (joint, frame, ends): (&str, M4, [f32; 2]) = match (item, striker.as_str()) {
        (Some((item, joint, socket)), _) => (
            joint,
            socket.elements,
            [item.floor, item.floor + item.height],
        ),
        (None, "hand_r" | "hand_l") => {
            let (joint, socket) = socket(&descriptor, &format!("{striker}.weapon"));
            (joint, socket.elements, [0., FIST])
        }
        (None, _) => (striker.as_str(), IDENTITY, [0., 0.]),
    };
    let scale = HEIGHT / character.height;
    let placed = |m: &M4| {
        [
            (m[12] - character.centre[0]) * scale,
            (m[13] - character.floor) * scale,
            (m[14] - character.centre[1]) * scale,
        ]
    };
    let mut tips = Vec::new();
    let steps = ((BEFORE + AFTER) * 120.).round() as u16;
    for step in 0..=steps {
        let time = (contact - BEFORE + f32::from(step) / 120.).clamp(0., duration);
        let pose = [Layer::single(clip, time)];
        let held = mul(
            &character
                .joint(joint, &pose)
                .expect("the striking joint")
                .elements,
            &frame,
        );
        let tip = ends
            .map(|y| placed(&mul(&held, &trs([0., y, 0.], [0., 0., 0., 1.], [1.; 3]))))
            .into_iter()
            .max_by(|a, b| a[0].hypot(a[2]).total_cmp(&b[0].hypot(b[2])))
            .expect("two ends");
        tips.push(tip);
    }
    let reach = tips.iter().map(|t| t[0].hypot(t[2])).fold(0., f32::max);
    let arc = tips
        .iter()
        .filter(|t| t[2] > 0. && t[0].hypot(t[2]) >= reach / 2.)
        .map(|t| t[0].atan2(t[2]).to_degrees().abs())
        .fold(0., f32::max);
    let low = tips.iter().map(|t| t[1]).fold(f32::MAX, f32::min);
    let high = tips.iter().map(|t| t[1]).fold(f32::MIN, f32::max);
    let units = |metres: f32| (metres * 100.).round() as i32;
    (
        clip,
        [units(reach), arc.round() as i32, units(low), units(high)],
    )
}

/// The generated `sim-combat` table.
fn table() -> String {
    let contacts = contacts();
    let mut arms = String::new();
    for family in WeaponFamily::ALL {
        for variant in StrikeVariant::ALL {
            let (clip, [tip, arc, low, high]) = measure(family, variant, &contacts);
            arms += &format!(
                "        // {clip}\n        ({family:?}, {variant:?}) => BladeSweep::new({tip}, {arc}, {low}, {high}),\n"
            );
        }
    }
    format!(
        "//! Each basic strike's blade sweep (#1249), measured from the clip that shows it with its
//! family's weapon. Generated: `MMORPG_UPDATE_FIXTURES=1 cargo test -p mmorpg-client
//! blade_sweeps` measures the clips again; do not edit by hand.

use super::{{BladeSweep, StrikeVariant, WeaponFamily}};

/// The sweep of `family`'s `variant`.
#[must_use]
pub const fn blade_sweep(family: WeaponFamily, variant: StrikeVariant) -> BladeSweep {{
    use StrikeVariant::{{Finisher, First, Jumping, Running, Second}};
    use WeaponFamily::{{Bow, Dagger, Mace, Spear, Staff, Sword, TwoHanded, Unarmed}};
    match (family, variant) {{
{arms}    }}
}}
"
    )
}

#[test]
fn blade_sweeps_follow_the_strike_clips() {
    let generated = table();
    if std::env::var_os("MMORPG_UPDATE_FIXTURES").is_some() {
        std::fs::write(TABLE, &generated).expect("the sweep table is writable");
        return;
    }
    let committed = std::fs::read_to_string(TABLE).expect("the sweep table");
    assert_eq!(
        committed, generated,
        "the strike clips moved: MMORPG_UPDATE_FIXTURES=1 re-measures the blade sweeps"
    );
}
