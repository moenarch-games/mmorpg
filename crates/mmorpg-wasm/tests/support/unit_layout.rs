//! The `unit-frame` layout workload (#1057, `docs/PRESENTATION_WORKLOADS.md`): the browser's
//! unit adapter (`mmorpg_wasm::units::Units`) over the `presentation_work` wolf crowd, the
//! viewer on its imported model. Per frame it samples the page's units, runs the shared
//! presenter and lays the frame out for the page; a counting allocator counts what the adapter's
//! own steps (sampling and layout, not the shared presenter) allocate. Counters only; nothing is
//! timed.
// The counting allocator is the workspace's one opt-in to unsafe code (Atlas 7.1.i1055).
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::BTreeSet,
    rc::Rc,
    sync::Arc,
};

use mmorpg_core::{
    CreatureFamily, EntityFlags, EntityKind, EntitySnapshot, ZoneId, ZoneSimulation, ZoneSnapshot,
};
use mmorpg_wasm::{
    scenery::presented_content,
    unit_layout::{HEADER_WORDS, Header, kind_code},
    units::Units,
};

/// Counts this thread's allocations, so parallel tests do not count each other's.
struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: every call forwards to `System`; counting touches only a const-initialised cell.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: as above.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// What laying out the measured frames cost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutRun {
    pub frames: usize,
    /// Units presented in the last frame, the viewer included.
    pub units: usize,
    /// Of those, the units drawn on an imported model.
    pub imported: usize,
    /// Entity lookups in the last frame: one per sampled unit, one per imported unit.
    pub layout_entity_lookups: usize,
    /// Bytes of the last frame's layout (words, numbers, models, fresh strings); no JSON.
    pub layout_bytes: usize,
    /// The most allocations the adapter's own steps made in one measured frame.
    pub warm_frame_allocations: usize,
    /// Strings sent during the measured frames (breathing torsos' new keys among them).
    pub strings_sent: usize,
}

/// `characters` wolves around the viewer at the hub, one walking east; two warm-up frames, then
/// `frames` measured frames.
#[must_use]
pub fn crowd(characters: usize, frames: usize) -> LayoutRun {
    let content = presented_content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content))
        .expect("the vale admits a zone");
    zone.add_player(1).expect("the viewer joins");
    let mut snapshot: ZoneSnapshot = zone.snapshot_for_player(1).expect("the viewer projects");
    let spawn = content
        .creature_spawns()
        .iter()
        .find(|spawn| {
            content
                .creature_template(spawn.template)
                .is_some_and(|template| template.family == CreatureFamily::Wolf)
        })
        .expect("the vale has wolves");
    let viewer = snapshot.entities[0].clone();
    for index in 0..characters {
        let (column, row) = ((index % 8) as i32, (index / 8) as i32);
        snapshot.entities.push(EntitySnapshot {
            kind: EntityKind::Creature,
            id: spawn.id.get() + u32::try_from(index).expect("a small crowd") * 10_000,
            appearance: spawn.template.get(),
            position: [
                viewer.position[0] + 300 + column * 250,
                viewer.position[1],
                viewer.position[2] + (row - 4) * 250,
            ],
            velocity: [0; 3],
            facing: 0,
            level: 2,
            health_percent: 100,
            flags: EntityFlags::default(),
        });
    }
    let mut units = Units::new(1);
    units
        .model_ready(
            "Ranger",
            r#"[["Idle_Loop", 2], ["Walk_Loop", 1], ["Jog_Fwd_Loop", 1]]"#,
        )
        .expect("the Ranger clips");
    let mut run = LayoutRun::default();
    let mut input = Vec::new();
    let mut held = BTreeSet::new();
    let warm_up = 2;
    for frame in 0..warm_up + frames {
        let seconds = frame as f64 / 60.;
        for record in snapshot.entities.iter_mut().skip(1).take(1) {
            record.position[0] += 10;
            record.velocity = [10, 0, 0];
        }
        snapshot.tick += 1;
        units.push(Rc::new(snapshot.clone()), seconds);
        input.clear();
        for record in &snapshot.entities {
            input.extend([
                f64::from(kind_code(record.kind)),
                f64::from(record.id),
                f64::from(record.position[0]),
                f64::from(record.position[1]),
                f64::from(record.position[2]),
                f64::from(record.facing),
            ]);
        }
        let viewer = mmorpg_wasm::units::viewer(1, "", f32::NAN).expect("viewer");
        let lookups = units.layout_work().entity_lookups;
        let strings = units.layout_work().strings_sent;
        let before = allocations();
        units.sample(&input).expect("sampled");
        let sampled = allocations() - before;
        let presented = units
            .present(viewer, seconds, 1. / 60., false, 0.)
            .expect("presented");
        let before = allocations();
        units.write(&presented);
        let written = allocations() - before;
        held = page_geometry(&units, &held);
        if frame >= warm_up {
            let work = units.layout_work();
            run.frames += 1;
            run.units = presented.units.len();
            run.imported = presented.imported.len();
            run.layout_entity_lookups = work.entity_lookups - lookups;
            run.layout_bytes = work.last_bytes;
            run.warm_frame_allocations = run.warm_frame_allocations.max(sampled + written);
            run.strings_sent += work.strings_sent - strings;
        }
    }
    run
}

/// The page's side of the geometry contract: every draw's resource key is one it held from the
/// previous frame or one this layout sends. Returns the keys the page holds now.
fn page_geometry(units: &Units, held: &BTreeSet<u32>) -> BTreeSet<u32> {
    let words = &units.layout().words;
    let draws = words[Header::Draws as usize] as usize;
    let geometries = words[Header::Geometries as usize] as usize;
    let sent: BTreeSet<u32> = (0..geometries)
        .map(|index| words[HEADER_WORDS + draws * 5 + index * 6])
        .collect();
    (0..draws)
        .map(|index| words[HEADER_WORDS + index * 5 + 1])
        .inspect(|key| {
            assert!(
                held.contains(key) || sent.contains(key),
                "draw {key} without its geometry"
            );
        })
        .collect()
}

/// What sampling the units of the measured frames cost (`client-interpolate`, #1058).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterpolationRun {
    /// Units sampled in the last frame, the viewer included.
    pub units: usize,
    /// The most entity records one measured frame's interpolation visited.
    pub samples_visited: usize,
    /// The most allocations one measured frame's interpolation made.
    pub warm_frame_allocations: usize,
}

/// `entities` wolves walking around the viewer at the hub, a projection a frame (the local
/// zone's pace), each frame sampled 2 ticks behind the newest at half a tick; 40 warm-up frames
/// fill the history, then `frames` measured frames.
#[must_use]
pub fn interpolation(entities: usize, frames: usize) -> InterpolationRun {
    let content = presented_content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content))
        .expect("the vale admits a zone");
    zone.add_player(1).expect("the viewer joins");
    let mut snapshot: ZoneSnapshot = zone.snapshot_for_player(1).expect("the viewer projects");
    snapshot.entities.truncate(1);
    let viewer = snapshot.entities[0].clone();
    for index in 0..entities {
        let id = u32::try_from(index).expect("a small crowd");
        snapshot.entities.push(EntitySnapshot {
            kind: EntityKind::Creature,
            id: 1 + id,
            appearance: 1,
            position: [
                viewer.position[0] + 300 + (id % 8) as i32 * 250,
                viewer.position[1],
                viewer.position[2] + (id / 8) as i32 * 250,
            ],
            velocity: [10, 0, 0],
            facing: 0,
            level: 2,
            health_percent: 100,
            flags: EntityFlags::default(),
        });
    }
    // Projections list units in no particular order.
    snapshot.entities.reverse();
    let mut units = Units::new(1);
    let mut run = InterpolationRun::default();
    let warm_up = 40;
    for frame in 0..warm_up + frames {
        for record in &mut snapshot.entities {
            record.position[0] += 10;
            record.facing = record.facing.wrapping_add(300);
        }
        snapshot.tick += 1;
        units.push(Rc::new(snapshot.clone()), frame as f64 / 60.);
        let visited = units.samples_visited();
        let before = allocations();
        units.interpolate(0.5, &[]).expect("interpolated");
        let allocated = allocations() - before;
        if frame >= warm_up {
            run.units = entities + 1;
            run.samples_visited = run.samples_visited.max(units.samples_visited() - visited);
            run.warm_frame_allocations = run.warm_frame_allocations.max(allocated);
        }
    }
    run
}
