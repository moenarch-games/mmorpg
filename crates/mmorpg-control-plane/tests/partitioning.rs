use mmorpg_control_plane::{
    CapacityCondition, EntityId, HostId, HostRegistry, LogicalZone, LogicalZoneConfig,
    LogicalZoneIdentity, PartitionError, PartitionRef, RealWorldInstant, SimulationTick,
    TickProgress, ZoneTimer,
};
use mmorpg_core::ZoneId;

const TICK_MICROS: u64 = 50_000;

fn host(name: &str) -> HostId {
    HostId::new(name).unwrap()
}

fn hosts() -> HostRegistry {
    let mut registry = HostRegistry::new(u64::MAX).unwrap();
    for name in ["a", "b", "c"] {
        registry.register(host(name), 0).unwrap();
    }
    registry
}

fn identity() -> LogicalZoneIdentity {
    LogicalZoneIdentity {
        zone_id: ZoneId::new(1),
        content_revision: 7,
    }
}

fn zone(capacity: u32) -> LogicalZone {
    LogicalZone::new(
        identity(),
        LogicalZoneConfig {
            columns: 12,
            tick_micros: TICK_MICROS,
            partition_capacity: capacity,
        },
        host("a"),
        &hosts(),
        0,
    )
    .unwrap()
}

fn entity(id: u64) -> EntityId {
    EntityId::new(id)
}

/// Two entities per column 0..12, ids `10 * column + n`.
fn populated(capacity: u32) -> LogicalZone {
    let mut zone = zone(capacity);
    for column in 0..12 {
        for n in 0..2 {
            zone.place(entity(u64::from(column) * 10 + n), column)
                .unwrap();
        }
    }
    zone
}

fn complete_tick(zone: &mut LogicalZone, costs: &[u64]) -> TickProgress {
    let layout = zone.layout();
    let tick = zone.tick().0;
    let mut progress = None;
    for (partition, cost) in layout.partitions.iter().zip(costs) {
        let token = PartitionRef {
            layout_epoch: layout.epoch,
            index: partition.index,
        };
        progress = Some(zone.report_tick(token, tick, *cost).unwrap());
    }
    progress.unwrap()
}

#[test]
fn repartitioning_keeps_identity_and_contiguous_strips() {
    let mut zone = populated(100);
    let epoch = zone.layout().epoch;

    let change = zone
        .repartition(epoch, &[host("a"), host("b"), host("c")], &hosts(), 0)
        .unwrap();

    let columns = change
        .layout
        .partitions
        .iter()
        .map(|partition| partition.columns.clone())
        .collect::<Vec<_>>();
    assert_eq!(columns, vec![0..4, 4..8, 8..12]);
    assert_eq!(zone.identity(), identity());
    assert_eq!(change.migrations.len(), 16, "two thirds leave host a");
    assert!(change.released_hosts.is_empty());

    // A merge back to two hosts is just another fenced layout.
    let merged = zone
        .repartition(change.layout.epoch, &[host("a"), host("b")], &hosts(), 0)
        .unwrap();
    assert_eq!(merged.layout.partitions.len(), 2);
    assert_eq!(zone.identity(), identity());
    assert!(matches!(
        zone.repartition(change.layout.epoch, &[host("a")], &hosts(), 0),
        Err(PartitionError::StaleLayout { .. })
    ));
}

#[test]
fn boundaries_move_around_an_interaction_island() {
    let mut zone = populated(100);
    let epoch = zone.layout().epoch;
    zone.repartition(epoch, &[host("a"), host("b"), host("c")], &hosts(), 0)
        .unwrap();

    // A fight spanning columns 3..=5 straddles the cut at column 4.
    let migrations = zone
        .set_interactions(&[(entity(30), entity(40)), (entity(40), entity(50))])
        .unwrap();

    let layout = zone.layout();
    let columns = layout
        .partitions
        .iter()
        .map(|partition| partition.columns.clone())
        .collect::<Vec<_>>();
    assert_eq!(columns, vec![0..3, 3..8, 8..12]);
    let owner = zone.owner(entity(30)).unwrap();
    for member in [40, 50] {
        assert_eq!(zone.owner(entity(member)).unwrap(), owner);
    }
    assert!(
        migrations
            .iter()
            .all(|migration| migration.to.1 == host("b"))
    );

    // Neighbours may read the island as ghosts but never mutate it.
    let left = PartitionRef {
        layout_epoch: layout.epoch,
        index: 0,
    };
    assert!(zone.ghosts(left, 1).unwrap().contains(&entity(30)));
    assert!(matches!(
        zone.authorize_mutation(left, entity(30)),
        Err(PartitionError::NotOwner { .. })
    ));
    zone.authorize_mutation(owner, entity(30)).unwrap();

    // Once the fight separates, a rebalance may split the region again.
    zone.set_interactions(&[]).unwrap();
    let change = zone
        .repartition(
            zone.layout().epoch,
            &[host("a"), host("b"), host("c")],
            &hosts(),
            0,
        )
        .unwrap();
    assert_eq!(change.layout.partitions[0].columns, 0..4);
}

#[test]
fn an_island_spanning_the_zone_coalesces_partitions_instead_of_splitting() {
    let mut zone = populated(10);
    let edges = (0..11)
        .map(|column| (entity(column * 10), entity((column + 1) * 10)))
        .collect::<Vec<_>>();
    zone.set_interactions(&edges).unwrap();

    let change = zone
        .repartition(
            zone.layout().epoch,
            &[host("a"), host("b"), host("c")],
            &hosts(),
            0,
        )
        .unwrap();

    assert_eq!(change.layout.partitions.len(), 1);
    assert_eq!(change.released_hosts, vec![host("b"), host("c")]);
    assert!(
        zone.capacity_conditions()
            .contains(&CapacityCondition::OversizedIsland {
                representative: entity(0),
                load: 12,
            })
    );

    // The oversized island slows the whole zone with a fixed timestep.
    let TickProgress::Completed(record) = complete_tick(&mut zone, &[2 * TICK_MICROS]) else {
        panic!("single partition completes the barrier");
    };
    assert_eq!(record.simulation_micros, TICK_MICROS);
    assert_eq!(record.real_micros, 2 * TICK_MICROS);
    assert_eq!(record.dilation_ppm, 500_000);
}

#[test]
fn partitions_share_one_tick_barrier_and_dilate_together() {
    let mut zone = populated(100);
    zone.repartition(zone.layout().epoch, &[host("a"), host("b")], &hosts(), 0)
        .unwrap();
    let layout = zone.layout();
    let first = PartitionRef {
        layout_epoch: layout.epoch,
        index: 0,
    };
    let second = PartitionRef {
        layout_epoch: layout.epoch,
        index: 1,
    };

    assert_eq!(
        zone.report_tick(first, 0, 10_000).unwrap(),
        TickProgress::Waiting {
            reported: 1,
            partitions: 2
        }
    );
    // The fast partition cannot run ahead, and topology waits for the barrier.
    assert!(matches!(
        zone.report_tick(first, 1, 10_000),
        Err(PartitionError::WrongTick { .. })
    ));
    assert!(matches!(
        zone.place(entity(999), 0),
        Err(PartitionError::TickInProgress { .. })
    ));
    let TickProgress::Completed(record) = zone.report_tick(second, 0, 4 * TICK_MICROS).unwrap()
    else {
        panic!("both partitions reported");
    };
    assert_eq!(record.dilation_ppm, 250_000);
    assert_eq!(zone.tick(), SimulationTick(1));

    complete_tick(&mut zone, &[TICK_MICROS, TICK_MICROS]);
    assert_eq!(zone.simulation_micros(), 2 * TICK_MICROS);
    assert_eq!(zone.real_micros(), 5 * TICK_MICROS);
    assert_eq!(zone.time_dilation_ppm(), 400_000);

    // Gameplay timers follow simulation time; real-world commitments do not.
    let real_now = RealWorldInstant(zone.real_micros() / 1_000);
    let cooldown = ZoneTimer::Simulation {
        due: SimulationTick(3),
    };
    let auction = ZoneTimer::RealWorld {
        due: RealWorldInstant(200),
    };
    assert!(!cooldown.is_due(zone.tick(), real_now));
    assert!(auction.is_due(zone.tick(), real_now));
}

#[test]
fn migration_preserves_identity_and_command_ordering() {
    let mut zone = populated(100);
    zone.repartition(zone.layout().epoch, &[host("a"), host("b")], &hosts(), 0)
        .unwrap();
    let walker = entity(51);
    let before = zone.owner(walker).unwrap();
    assert_eq!(zone.submit_command(walker, 1).unwrap(), before);
    complete_tick(&mut zone, &[TICK_MICROS, TICK_MICROS]);

    let migrations = zone.place(walker, 9).unwrap();
    assert_eq!(migrations.len(), 1);
    assert_eq!(migrations[0].entity_id, walker);
    assert_eq!(migrations[0].to.1, host("b"));

    let after = zone.owner(walker).unwrap();
    assert_ne!(after.index, before.index);
    assert!(matches!(
        zone.submit_command(walker, 1),
        Err(PartitionError::StaleCommand { last: 1, .. })
    ));
    zone.submit_command(entity(90), 4).unwrap();
    assert_eq!(zone.submit_command(walker, 2).unwrap(), after);
    assert_eq!(
        zone.commands_for(after).unwrap(),
        vec![(walker, 2), (entity(90), 4)]
    );
}
