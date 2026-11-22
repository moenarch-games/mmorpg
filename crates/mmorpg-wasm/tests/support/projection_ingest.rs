//! The `projection-ingest` workload (#1009, `docs/PRESENTATION_WORKLOADS.md`): a running
//! Greyhaven player's projections, tick by tick, through the browser's ingest path. Each one is
//! ingested into a session's `ProjectionStore` (the local host's inside WASM, an online one as
//! the network payload the page receives) and pushed to the shared unit presenter by id, as
//! `world-view.ts` does. Counters only; nothing is timed.

use mmorpg_core::{ZoneCommand, ZoneSnapshot};
use mmorpg_wasm::{host::LocalZoneHost, projections::ProjectionStore, units::Units};

/// What ingesting and presenting `projections` cost, both paths together.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IngestRun {
    pub projections: usize,
    /// The most decodes any one projection took, from receipt to the unit presenter.
    pub largest_decodes_per_projection: usize,
    /// Sheet objects (bag, quests) materialised for the page.
    pub sheet_views: usize,
    /// Of those, the ones equal to the copy the page already held.
    pub unchanged_sheet_views: usize,
    /// Bytes copied into the store beyond the online network payloads (none for local ones).
    pub extra_boundary_bytes: usize,
}

#[derive(Clone, Copy)]
enum Path {
    Local,
    Online,
}

fn bag(snapshot: &ZoneSnapshot) -> impl PartialEq + '_ {
    (
        snapshot.inventory_revision,
        &snapshot.inventory,
        &snapshot.equipment,
    )
}

fn run_path(path: Path, ticks: usize, run: &mut IngestRun) {
    let mut host = LocalZoneHost::new().expect("local host");
    let player = host.join_as(0, 1).expect("player");
    let mut store = ProjectionStore::new();
    let mut units = Units::new(player);
    let mut network_bytes = 0;
    let mut held: Option<std::rc::Rc<ZoneSnapshot>> = None;
    for tick in 0..ticks {
        // Run east, turning now and then, so the projection changes every tick.
        let facing = u16::try_from(16_384 + (tick / 16) * 2_048).expect("facing");
        let sequence = u32::try_from(tick + 2).expect("sequence");
        let command = mmorpg_protocol::encode_command(ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        });
        host.submit(player, sequence, &command).expect("move");
        host.tick().expect("tick");
        let payload = host.projection(player).expect("projection");
        let decodes = store.work().decodes;
        let ingested = match path {
            Path::Local => store.ingest_local(&payload),
            Path::Online => {
                network_bytes += payload.len();
                store.ingest(&payload)
            }
        }
        .expect("ingest");
        let snapshot = store.get(ingested.id).expect("retained");
        units.push(std::rc::Rc::clone(&snapshot), tick as f64 / 60.);
        run.largest_decodes_per_projection = run
            .largest_decodes_per_projection
            .max(store.work().decodes - decodes);
        if let Some(previous) = &held {
            run.unchanged_sheet_views +=
                usize::from(ingested.view.inventory.is_some() && bag(previous) == bag(&snapshot))
                    + usize::from(
                        ingested.view.quests.is_some() && previous.quests == snapshot.quests,
                    );
        }
        held = Some(snapshot);
    }
    let work = store.work();
    run.projections += work.projections;
    run.sheet_views += work.sheet_views;
    run.extra_boundary_bytes += work.boundary_bytes - network_bytes;
}

/// `ticks` projections through each path, local and online.
#[must_use]
pub fn ingest(ticks: usize) -> IngestRun {
    let mut run = IngestRun::default();
    run_path(Path::Local, ticks, &mut run);
    run_path(Path::Online, ticks, &mut run);
    run
}
