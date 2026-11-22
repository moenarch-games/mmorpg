//! The browser's unit frame layout (#1057): one frame of the shared unit presenter as
//! typed-array records in WASM memory, which `web/src/world/unit-presenter.ts` reads through
//! views and turns into its renderer nodes in place.
//!
//! Draw ids, resource keys, clip names and every other string cross the boundary once, when
//! first seen: each is interned to a stable index and sent in that frame's fresh strings;
//! records carry the index. Per frame only numbers cross: `words` (`u32`), `numbers` (`f64`) and
//! the draws' model matrices (`models`, `f32`, 16 per draw in draw order), plus the geometry of
//! resources the page did not hold. The buffers are reused, so a warm frame allocates nothing
//! here.
//!
//! The records follow each other in the order below; `unit-presenter.ts` reads them with the
//! same cursors. [`NONE`] marks an absent string or value.
//!
//! - header `words`: [`Header`]; `numbers`: the camera kick `[x, y, z]`;
//! - draw: `words` id, key, colour `0xRRGGBB`, emissive `0xRRGGBB` or [`NONE`], flags
//!   ([`DRAW_UNLIT`], [`DRAW_DOUBLE_SIDED`]); `numbers` opacity;
//! - geometry: `words` key, vertex offset, vertex count, coloured, index offset, index count;
//! - unit: `words` kind code, entity id, node, body start, body end; `numbers` feet `[x, y, z]`,
//!   centre y, yaw, label `[x, y, z]`;
//! - imported: `words` id, kind, entity kind code, entity id, flags (`IMPORTED_*`), upper clip,
//!   lower clip, loadout, resident, figure, variety, outfit (`preset;socket,socket`), bow clip,
//!   throwable, belted, crossbow bolt, channel joint, chore item, chore socket, layer count, then
//!   per layer its clip and mask; `numbers` feet `[x, y, z]`, yaw, pitch, height, upper time,
//!   lower time, per layer its time and weight, then tint (3), swing (4), chains (6), bow time
//!   and channel point (3), each only when its flag is set;
//! - platform: `words` platform id; `numbers` base `[x, y, z]`;
//! - effect: `words` key, clip; `numbers` time, tint `[r, g, b]` (`NaN` as authored), model (16).

use std::collections::HashMap;

use mmorpg_core::{EntityKind, EntityRef};
use mmorpg_presentation::{
    Draw,
    units::{
        ImportedUnit, UnitFrame,
        imported::{Bolt, Hand, Loadout, Outfit, Resident, Throwable},
    },
};

/// An absent string or value.
pub const NONE: u32 = u32::MAX;
/// Interned strings beyond which the next frame starts the table again, so a long session
/// cannot grow it without bound; the page drops its copies with it.
const STRING_LIMIT: usize = 1 << 16;

pub const DRAW_UNLIT: u32 = 1;
pub const DRAW_DOUBLE_SIDED: u32 = 1 << 1;

pub const IMPORTED_HEADWEAR: u32 = 1;
pub const IMPORTED_SHEATHED_LEFT: u32 = 1 << 1;
pub const IMPORTED_SHEATHED_RIGHT: u32 = 1 << 2;
/// The player's weapon mode is known; [`IMPORTED_WEAPON_DRAWN`] holds it.
pub const IMPORTED_WEAPON_MODE: u32 = 1 << 3;
pub const IMPORTED_WEAPON_DRAWN: u32 = 1 << 4;
pub const IMPORTED_TINT: u32 = 1 << 5;
pub const IMPORTED_SWING: u32 = 1 << 6;
pub const IMPORTED_CHAINS: u32 = 1 << 7;
pub const IMPORTED_BOW: u32 = 1 << 8;
pub const IMPORTED_CROSSBOW: u32 = 1 << 9;
pub const IMPORTED_CROSSBOW_LEFT_HAND: u32 = 1 << 10;
pub const IMPORTED_CROSSBOW_CRANK: u32 = 1 << 11;
pub const IMPORTED_CHANNEL: u32 = 1 << 12;

/// The header's words, before the first record.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Header {
    /// 1 when the page drops its interned strings (and what it keys by them) first.
    StringsReset,
    FreshStrings,
    Draws,
    Geometries,
    Units,
    Imported,
    Platforms,
    Effects,
}

pub const HEADER_WORDS: usize = Header::Effects as usize + 1;
const GEOMETRY_WORDS: usize = 6;

/// Deterministic work of the layouts written so far (`unit-frame`,
/// `docs/PRESENTATION_WORKLOADS.md`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutWork {
    /// Entity lookups: a sampled unit's snapshot record, an imported unit's view.
    pub entity_lookups: usize,
    /// Bytes of the last layout: words, numbers, models and fresh strings. Geometry uploads
    /// are `geometry_uploads`' business.
    pub last_bytes: usize,
    /// Strings interned and sent so far.
    pub strings_sent: usize,
}

/// Interned strings: each crosses once, when first seen.
#[derive(Default)]
struct Strings {
    ids: HashMap<String, u32>,
    outfits: Vec<(Outfit, u32)>,
    /// This frame's new strings, each followed by `\n`.
    fresh: String,
    fresh_count: u32,
    reset: bool,
    /// The next frame starts the table again.
    restart: bool,
}

impl Strings {
    fn begin(&mut self) {
        self.fresh.clear();
        self.fresh_count = 0;
        self.reset = self.restart || self.ids.len() >= STRING_LIMIT;
        self.restart = false;
        if self.reset {
            self.ids.clear();
            self.outfits.clear();
        }
    }

    fn id(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        debug_assert!(
            !text.contains('\n'),
            "interned strings are newline-separated"
        );
        let id = u32::try_from(self.ids.len()).expect("fewer strings than u32");
        self.ids.insert(text.to_owned(), id);
        self.fresh.push_str(text);
        self.fresh.push('\n');
        self.fresh_count += 1;
        id
    }

    fn optional(&mut self, text: Option<&str>) -> u32 {
        text.map_or(NONE, |text| self.id(text))
    }

    /// An outfit as `preset;socket,socket`, spelled out only the first time it is seen.
    fn outfit(&mut self, outfit: Outfit) -> u32 {
        if let Some(&(_, id)) = self.outfits.iter().find(|(seen, _)| *seen == outfit) {
            return id;
        }
        let id = self.id(&format!(
            "{};{}",
            outfit.preset(),
            outfit.replaced().join(",")
        ));
        self.outfits.push((outfit, id));
        id
    }
}

#[must_use]
pub fn kind_code(kind: EntityKind) -> u32 {
    match kind {
        EntityKind::Player => 0,
        EntityKind::Creature => 1,
        EntityKind::Npc => 2,
        EntityKind::Minion => 3,
    }
}

/// `0xRRGGBB` of 0–1 channels.
fn rgb(color: [f32; 3]) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |channel: f32| u32::from((channel.clamp(0., 1.) * 255.).round() as u8);
    (byte(color[0]) << 16) | (byte(color[1]) << 8) | byte(color[2])
}

fn index(value: usize) -> u32 {
    u32::try_from(value).expect("a frame's records fit in u32")
}

/// One frame's layout, and the strings and geometry the page already holds.
#[derive(Default)]
pub struct UnitLayout {
    strings: Strings,
    pub words: Vec<u32>,
    pub numbers: Vec<f64>,
    pub models: Vec<f32>,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    /// This frame's geometry records, appended after the draws.
    geometry: Vec<u32>,
    /// The frame each interned resource key was last drawn in (0 never): the page holds the
    /// geometry of those drawn in the previous frame.
    drawn: Vec<u64>,
    frame: u64,
    /// Each presented unit's position in the frame's `units`, for an imported unit's yaw.
    views: HashMap<EntityRef, usize>,
    work: LayoutWork,
}

impl UnitLayout {
    #[must_use]
    pub fn work(&self) -> LayoutWork {
        self.work
    }

    /// Counts entity lookups made for the frame outside the layout (the sampled units' records).
    pub fn count_lookups(&mut self, lookups: usize) {
        self.work.entity_lookups += lookups;
    }

    /// The new strings of the last layout, each followed by `\n`.
    #[must_use]
    pub fn fresh_strings(&self) -> &str {
        &self.strings.fresh
    }

    /// The next layout starts again: it tells the page to drop its strings and geometry, and
    /// sends every one again.
    pub fn reset(&mut self) {
        self.strings.restart = true;
    }

    /// Lays out `frame`.
    pub fn write(&mut self, frame: &UnitFrame) {
        self.strings.begin();
        if self.strings.reset {
            self.drawn.clear();
        }
        self.frame += 1;
        for buffer in [&mut self.words, &mut self.indices, &mut self.geometry] {
            buffer.clear();
        }
        self.numbers.clear();
        self.models.clear();
        self.vertices.clear();
        self.words.resize(HEADER_WORDS, 0);
        self.numbers.extend(frame.kick.map(f64::from));
        for draw in &frame.draws {
            self.draw(draw);
        }
        self.words.extend_from_slice(&self.geometry);
        self.views.clear();
        for (position, view) in frame.units.iter().enumerate() {
            self.views.insert(view.entity, position);
            let node = self.strings.id(&view.id);
            self.words.extend([
                kind_code(view.entity.kind()),
                view.entity.id(),
                node,
                index(view.body.start),
                index(view.body.end),
            ]);
            self.numbers.extend(view.feet);
            self.numbers.extend([f64::from(view.centre_y), view.yaw]);
            self.numbers.extend(view.label.map(f64::from));
        }
        for unit in &frame.imported {
            self.imported(frame, unit);
        }
        for view in &frame.platforms {
            self.words.push(view.platform.get());
            self.numbers.extend(view.base.map(f64::from));
        }
        for effect in &frame.effects {
            let key = self.strings.id(effect.key);
            let clip = self.strings.optional(effect.clip);
            self.words.extend([key, clip]);
            self.numbers.push(f64::from(effect.time));
            self.numbers.extend(
                effect
                    .tint
                    .map_or([f64::NAN; 3], |tint| tint.map(f64::from)),
            );
            self.numbers.extend(effect.model.map(f64::from));
        }
        let header = [
            (Header::StringsReset, u32::from(self.strings.reset)),
            (Header::FreshStrings, self.strings.fresh_count),
            (Header::Draws, index(frame.draws.len())),
            (
                Header::Geometries,
                index(self.geometry.len() / GEOMETRY_WORDS),
            ),
            (Header::Units, index(frame.units.len())),
            (Header::Imported, index(frame.imported.len())),
            (Header::Platforms, index(frame.platforms.len())),
            (Header::Effects, index(frame.effects.len())),
        ];
        for (word, value) in header {
            self.words[word as usize] = value;
        }
        self.work.strings_sent += self.strings.fresh_count as usize;
        self.work.last_bytes = self.words.len() * 4
            + self.numbers.len() * 8
            + self.models.len() * 4
            + self.strings.fresh.len();
    }

    fn draw(&mut self, draw: &Draw) {
        let id = self.strings.id(&draw.id);
        let key = self.strings.id(&draw.resource_key);
        let slot = key as usize;
        if self.drawn.len() <= slot {
            self.drawn.resize(slot + 1, 0);
        }
        // Never drawn, or not in the previous frame (the page dropped it) nor already sent in this one.
        let last = self.drawn[slot];
        if last == 0 || last + 1 < self.frame {
            let g = &draw.geometry;
            let vertex_offset = self.vertices.len();
            for values in [&g.p, &g.n, &g.colors] {
                #[allow(clippy::cast_possible_truncation)]
                self.vertices
                    .extend(values.iter().flatten().map(|value| *value as f32));
            }
            let index_offset = self.indices.len();
            self.indices.extend(&g.indices);
            self.geometry.extend([
                key,
                index(vertex_offset),
                index(g.p.len()),
                u32::from(!g.colors.is_empty()),
                index(index_offset),
                index(g.indices.len()),
            ]);
        }
        self.drawn[slot] = self.frame;
        self.models.extend(draw.model);
        let material = &draw.material;
        let flags = if material.unlit { DRAW_UNLIT } else { 0 }
            | if material.double_sided {
                DRAW_DOUBLE_SIDED
            } else {
                0
            };
        let emissive = if material.emissive == [0.; 3] {
            NONE
        } else {
            rgb(material.emissive)
        };
        self.words
            .extend([id, key, rgb(material.color), emissive, flags]);
        self.numbers.push(f64::from(material.opacity));
    }

    fn imported(&mut self, frame: &UnitFrame, unit: &ImportedUnit) {
        self.work.entity_lookups += 1;
        let yaw = self
            .views
            .get(&unit.entity)
            .map_or(0., |&position| frame.units[position].yaw);
        let animation = &unit.animation;
        let carry = &animation.carry;
        let strings = &mut self.strings;
        let mut flags = 0;
        let mut flag = |set: bool, bit: u32| {
            if set {
                flags |= bit;
            }
        };
        flag(unit.headwear.is_some(), IMPORTED_HEADWEAR);
        flag(carry.sheathed[0], IMPORTED_SHEATHED_LEFT);
        flag(carry.sheathed[1], IMPORTED_SHEATHED_RIGHT);
        flag(carry.weapon_drawn.is_some(), IMPORTED_WEAPON_MODE);
        flag(carry.weapon_drawn == Some(true), IMPORTED_WEAPON_DRAWN);
        flag(unit.tint.is_some(), IMPORTED_TINT);
        flag(!unit.swing.is_rest(), IMPORTED_SWING);
        let chains = unit.swing.chain_drive();
        flag(chains.is_some(), IMPORTED_CHAINS);
        flag(unit.bow.is_some(), IMPORTED_BOW);
        flag(carry.crossbow.is_some(), IMPORTED_CROSSBOW);
        flag(
            carry.crossbow.is_some_and(|crossbow| crossbow.left_hand),
            IMPORTED_CROSSBOW_LEFT_HAND,
        );
        flag(
            carry.crossbow.is_some_and(|crossbow| crossbow.crank),
            IMPORTED_CROSSBOW_CRANK,
        );
        flag(animation.channel.is_some(), IMPORTED_CHANNEL);
        let words = [
            strings.id(&unit.id),
            strings.id(unit.kind.name()),
            kind_code(unit.entity.kind()),
            unit.entity.id(),
            flags,
            strings.id(animation.upper.0),
            strings.id(animation.lower.0),
            strings.optional(unit.loadout.map(Loadout::name)),
            strings.optional(unit.resident.map(Resident::preset)),
            strings.optional(unit.resident.map(|resident| resident.figure().character())),
            unit.variety.unwrap_or(NONE),
            unit.outfit.map_or(NONE, |outfit| strings.outfit(outfit)),
            strings.optional(unit.bow.map(|shot| shot.clip)),
            strings.optional(carry.throwable.map(Throwable::name)),
            strings.optional(carry.belted.map(Throwable::name)),
            strings.optional(
                carry
                    .crossbow
                    .and_then(|crossbow| crossbow.bolt)
                    .map(Bolt::name),
            ),
            strings.optional(animation.channel.map(|channel| match channel.hand {
                Hand::Left => "hand_l",
                Hand::Right => "hand_r",
            })),
            strings.optional(carry.chore.map(|chore| chore.item)),
            strings.optional(carry.chore.map(|chore| chore.socket)),
            index(animation.layers.len()),
        ];
        self.words.extend(words);
        self.numbers.extend(unit.root.p);
        self.numbers.extend([
            yaw + animation.attitude.heading,
            animation.attitude.pitch,
            unit.height,
            f64::from(animation.upper.1),
            f64::from(animation.lower.1),
        ]);
        for layer in &animation.layers {
            let clip = self.strings.id(layer.clip);
            let mask = self.strings.id(layer.mask.name());
            self.words.extend([clip, mask]);
            self.numbers
                .extend([f64::from(layer.time), f64::from(layer.weight)]);
        }
        if let Some(tint) = unit.tint {
            self.numbers.extend(tint.map(f64::from));
        }
        if !unit.swing.is_rest() {
            self.numbers.extend(unit.swing.quaternion().map(f64::from));
        }
        if let Some(chains) = chains {
            self.numbers.extend(chains.map(f64::from));
        }
        if let Some(shot) = unit.bow {
            self.numbers.push(f64::from(shot.time));
        }
        if let Some(channel) = animation.channel {
            self.numbers.extend(channel.point);
        }
    }
}
