//! Runtime partitioning of one logical zone (reference model).
//!
//! A logical zone keeps one identity, content revision and simulation
//! timeline. At runtime it may be cut into spatially contiguous column strips,
//! each owned by one host. The model enforces the partitioning contract:
//!
//! - a strip boundary never falls inside a connected interaction island, so
//!   every island has exactly one mutation owner on one machine;
//! - neighbours may read ghost entities across a boundary but never mutate them;
//! - all partitions report tick `N` before tick `N + 1` begins (tick barrier);
//! - overload keeps the fixed timestep and slows the whole zone (time dilation);
//! - command sequence numbers belong to the logical zone, so migration between
//!   partitions preserves entity identity and command ordering.
//!
//! The model holds no `ZoneSimulation` and decides no gameplay outcome. The
//! zone reports entity columns and interaction edges; the model decides only
//! which partition owns which entity.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::ops::Range;

use mmorpg_core::ZoneId;

use crate::{EntityId, HostId, HostRegistry};

const PARTS_PER_MILLION: u64 = 1_000_000;

type Ownership = BTreeMap<EntityId, (PartitionRef, HostId)>;

/// Stable identity of a logical zone. Repartitioning never changes it; durable
/// state stays keyed by the logical zone, not by a runtime partition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalZoneIdentity {
    pub zone_id: ZoneId,
    pub content_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalZoneConfig {
    /// Number of spatial columns the zone is cut along.
    pub columns: u32,
    /// Fixed simulation timestep in microseconds. Overload never changes it.
    pub tick_micros: u64,
    /// Load (entities) one partition host is provisioned for.
    pub partition_capacity: u32,
}

/// Fencing token for one partition in one layout. Any layout change advances
/// the epoch, so tokens from an older layout fail closed.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PartitionRef {
    pub layout_epoch: u64,
    pub index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Partition {
    pub index: u32,
    pub host_id: HostId,
    /// Half-open, contiguous column range.
    pub columns: Range<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartitionLayout {
    pub epoch: u64,
    pub partitions: Vec<Partition>,
}

/// A connected set of interacting entities that needs one simulation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionIsland {
    /// Sorted entity IDs; the first is the island's stable representative.
    pub entities: Vec<EntityId>,
    pub min_column: u32,
    pub max_column: u32,
}

impl InteractionIsland {
    #[must_use]
    pub fn load(&self) -> u32 {
        u32::try_from(self.entities.len()).unwrap_or(u32::MAX)
    }
}

/// An entity whose owning partition or host changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub entity_id: EntityId,
    pub from: Option<(PartitionRef, HostId)>,
    pub to: (PartitionRef, HostId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutChange {
    pub layout: PartitionLayout,
    pub migrations: Vec<Migration>,
    /// Hosts offered to the layout that ended up without a partition because
    /// interaction islands forced partitions to coalesce.
    pub released_hosts: Vec<HostId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapacityCondition {
    /// One island exceeds a host's capacity. It stays on one machine; the zone
    /// absorbs the cost through time dilation instead of distributed physics.
    OversizedIsland {
        representative: EntityId,
        load: u32,
    },
    PartitionOverCapacity {
        partition: PartitionRef,
        load: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickRecord {
    pub tick: u64,
    /// Simulation time advanced: always exactly the fixed timestep.
    pub simulation_micros: u64,
    /// Real time the barrier took: the slowest partition, never less than the
    /// timestep.
    pub real_micros: u64,
    /// Simulation speed relative to real time for this tick, in ppm.
    pub dilation_ppm: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TickProgress {
    Waiting { reported: u32, partitions: u32 },
    Completed(TickRecord),
}

/// Simulation clock domain: gameplay timers (physics, combat, AI, cooldowns).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SimulationTick(pub u64);

/// Real-world clock domain, always supplied by the caller (UTC milliseconds).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RealWorldInstant(pub u64);

/// A timer declares its clock domain. Simulation timers slow down with the
/// zone under time dilation; real-world commitments (auctions, schedules,
/// offline obligations) do not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneTimer {
    Simulation { due: SimulationTick },
    RealWorld { due: RealWorldInstant },
}

impl ZoneTimer {
    #[must_use]
    pub fn is_due(self, simulation: SimulationTick, real_world: RealWorldInstant) -> bool {
        match self {
            Self::Simulation { due } => simulation >= due,
            Self::RealWorld { due } => real_world >= due,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartitionError {
    InvalidConfig,
    NoHosts,
    DuplicateHost(HostId),
    Host(crate::ControlPlaneError),
    ColumnOutOfRange {
        column: u32,
        columns: u32,
    },
    UnknownEntity(EntityId),
    SelfInteraction(EntityId),
    StaleLayout {
        expected: u64,
        actual: u64,
    },
    UnknownPartition(PartitionRef),
    NotOwner {
        entity_id: EntityId,
        partition: PartitionRef,
    },
    TickInProgress {
        tick: u64,
    },
    WrongTick {
        reported: u64,
        current: u64,
    },
    ConflictingTickReport {
        partition: PartitionRef,
        tick: u64,
    },
    StaleCommand {
        entity_id: EntityId,
        sequence: u64,
        last: u64,
    },
}

impl fmt::Display for PartitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(
                formatter,
                "logical zone needs at least one column, a timestep and capacity"
            ),
            Self::NoHosts => write!(formatter, "a layout needs at least one host"),
            Self::DuplicateHost(host_id) => {
                write!(formatter, "host {} was offered twice", host_id.as_str())
            }
            Self::Host(error) => error.fmt(formatter),
            Self::ColumnOutOfRange { column, columns } => {
                write!(formatter, "column {column} is outside 0..{columns}")
            }
            Self::UnknownEntity(entity_id) => {
                write!(formatter, "entity {} is not placed", entity_id.get())
            }
            Self::SelfInteraction(entity_id) => {
                write!(
                    formatter,
                    "entity {} cannot interact with itself",
                    entity_id.get()
                )
            }
            Self::StaleLayout { expected, actual } => write!(
                formatter,
                "layout epoch {expected} is stale; current epoch is {actual}"
            ),
            Self::UnknownPartition(partition) => write!(
                formatter,
                "partition {} does not exist in layout {}",
                partition.index, partition.layout_epoch
            ),
            Self::NotOwner {
                entity_id,
                partition,
            } => write!(
                formatter,
                "partition {} does not own entity {}",
                partition.index,
                entity_id.get()
            ),
            Self::TickInProgress { tick } => write!(
                formatter,
                "tick {tick} is partially reported; topology changes wait for the barrier"
            ),
            Self::WrongTick { reported, current } => write!(
                formatter,
                "tick {reported} reported while the zone is at tick {current}"
            ),
            Self::ConflictingTickReport { partition, tick } => write!(
                formatter,
                "partition {} reported tick {tick} twice with different costs",
                partition.index
            ),
            Self::StaleCommand {
                entity_id,
                sequence,
                last,
            } => write!(
                formatter,
                "command {sequence} for entity {} is stale; last accepted is {last}",
                entity_id.get()
            ),
        }
    }
}

impl Error for PartitionError {}

impl From<crate::ControlPlaneError> for PartitionError {
    fn from(error: crate::ControlPlaneError) -> Self {
        Self::Host(error)
    }
}

/// Partition map, tick barrier and clock of one logical zone.
pub struct LogicalZone {
    identity: LogicalZoneIdentity,
    config: LogicalZoneConfig,
    epoch: u64,
    /// Strictly increasing interior cut columns; partition `i` spans
    /// `cuts[i - 1]..cuts[i]`.
    cuts: Vec<u32>,
    hosts: Vec<HostId>,
    entities: BTreeMap<EntityId, u32>,
    interactions: BTreeSet<(EntityId, EntityId)>,
    tick: u64,
    reports: BTreeMap<u32, u64>,
    last_sequence: BTreeMap<EntityId, u64>,
    pending_commands: BTreeSet<(EntityId, u64)>,
    simulation_micros: u64,
    real_micros: u64,
}

impl LogicalZone {
    /// Starts the zone as one partition on `host_id` at tick zero.
    pub fn new(
        identity: LogicalZoneIdentity,
        config: LogicalZoneConfig,
        host_id: HostId,
        registry: &HostRegistry,
        now_tick: u64,
    ) -> Result<Self, PartitionError> {
        if config.columns == 0 || config.tick_micros == 0 || config.partition_capacity == 0 {
            return Err(PartitionError::InvalidConfig);
        }
        registry.ensure_live(&host_id, now_tick)?;
        Ok(Self {
            identity,
            config,
            epoch: 1,
            cuts: Vec::new(),
            hosts: vec![host_id],
            entities: BTreeMap::new(),
            interactions: BTreeSet::new(),
            tick: 0,
            reports: BTreeMap::new(),
            last_sequence: BTreeMap::new(),
            pending_commands: BTreeSet::new(),
            simulation_micros: 0,
            real_micros: 0,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> LogicalZoneIdentity {
        self.identity
    }

    #[must_use]
    pub const fn config(&self) -> LogicalZoneConfig {
        self.config
    }

    /// The next tick the barrier waits for.
    #[must_use]
    pub const fn tick(&self) -> SimulationTick {
        SimulationTick(self.tick)
    }

    #[must_use]
    pub const fn simulation_micros(&self) -> u64 {
        self.simulation_micros
    }

    #[must_use]
    pub const fn real_micros(&self) -> u64 {
        self.real_micros
    }

    /// Cumulative simulation speed relative to real time, in ppm.
    #[must_use]
    pub fn time_dilation_ppm(&self) -> u64 {
        if self.real_micros == 0 {
            return PARTS_PER_MILLION;
        }
        ratio_ppm(self.simulation_micros, self.real_micros)
    }

    #[must_use]
    pub fn layout(&self) -> PartitionLayout {
        PartitionLayout {
            epoch: self.epoch,
            partitions: (0..self.hosts.len())
                .map(|index| self.partition(index))
                .collect(),
        }
    }

    /// Connected interaction islands in a stable order (by representative).
    #[must_use]
    pub fn islands(&self) -> Vec<InteractionIsland> {
        let mut parent = self
            .entities
            .keys()
            .map(|entity| (*entity, *entity))
            .collect::<BTreeMap<_, _>>();
        for (a, b) in &self.interactions {
            let root_a = find(&mut parent, *a);
            let root_b = find(&mut parent, *b);
            // Union by smaller ID keeps the representative deterministic.
            let (low, high) = if root_a <= root_b {
                (root_a, root_b)
            } else {
                (root_b, root_a)
            };
            parent.insert(high, low);
        }
        let mut members = BTreeMap::<EntityId, Vec<EntityId>>::new();
        let entities = self.entities.keys().copied().collect::<Vec<_>>();
        for entity in entities {
            let root = find(&mut parent, entity);
            members.entry(root).or_default().push(entity);
        }
        members
            .into_values()
            .map(|entities| {
                let columns = entities.iter().map(|entity| self.entities[entity]);
                let min_column = columns.clone().min().expect("islands are non-empty");
                let max_column = columns.max().expect("islands are non-empty");
                InteractionIsland {
                    entities,
                    min_column,
                    max_column,
                }
            })
            .collect()
    }

    /// The current owner of an entity.
    pub fn owner(&self, entity_id: EntityId) -> Result<PartitionRef, PartitionError> {
        let column = *self
            .entities
            .get(&entity_id)
            .ok_or(PartitionError::UnknownEntity(entity_id))?;
        Ok(self.partition_ref(self.index_of(column)))
    }

    /// Fails closed unless `partition` is the current owner of `entity_id`.
    /// Neighbours holding a ghost copy cannot mutate it.
    pub fn authorize_mutation(
        &self,
        partition: PartitionRef,
        entity_id: EntityId,
    ) -> Result<(), PartitionError> {
        self.ensure_partition(partition)?;
        if self.owner(entity_id)? != partition {
            return Err(PartitionError::NotOwner {
                entity_id,
                partition,
            });
        }
        Ok(())
    }

    /// Read-only ghost entities owned by neighbours within `margin` columns of
    /// `partition`'s strip.
    pub fn ghosts(
        &self,
        partition: PartitionRef,
        margin: u32,
    ) -> Result<Vec<EntityId>, PartitionError> {
        self.ensure_partition(partition)?;
        let columns = self.columns_of(partition.index as usize);
        let low = columns.start.saturating_sub(margin);
        let high = columns.end.saturating_add(margin);
        Ok(self
            .entities
            .iter()
            .filter(|(_, column)| (low..high).contains(*column) && !columns.contains(*column))
            .map(|(entity, _)| *entity)
            .collect())
    }

    /// Places or moves an entity. Boundaries shift around any island this
    /// splits; the returned migrations include the entity's own move.
    pub fn place(
        &mut self,
        entity_id: EntityId,
        column: u32,
    ) -> Result<Vec<Migration>, PartitionError> {
        self.ensure_between_ticks()?;
        self.ensure_column(column)?;
        let before = self.ownership();
        self.entities.insert(entity_id, column);
        Ok(self.reconcile(&before))
    }

    pub fn remove(&mut self, entity_id: EntityId) -> Result<(), PartitionError> {
        self.ensure_between_ticks()?;
        self.entities
            .remove(&entity_id)
            .ok_or(PartitionError::UnknownEntity(entity_id))?;
        self.interactions
            .retain(|(a, b)| *a != entity_id && *b != entity_id);
        Ok(())
    }

    /// Replaces the interaction edges the zone reports for the next tick and
    /// moves boundaries so no island spans two partitions.
    pub fn set_interactions(
        &mut self,
        edges: &[(EntityId, EntityId)],
    ) -> Result<Vec<Migration>, PartitionError> {
        self.ensure_between_ticks()?;
        let mut interactions = BTreeSet::new();
        for &(a, b) in edges {
            for entity in [a, b] {
                if !self.entities.contains_key(&entity) {
                    return Err(PartitionError::UnknownEntity(entity));
                }
            }
            if a == b {
                return Err(PartitionError::SelfInteraction(a));
            }
            interactions.insert(if a < b { (a, b) } else { (b, a) });
        }
        let before = self.ownership();
        self.interactions = interactions;
        Ok(self.reconcile(&before))
    }

    /// Rebalances the zone over `hosts` (one partition per host, in order) by
    /// load. Islands are never split; if they leave too few valid boundaries,
    /// partitions coalesce and the unused hosts are released.
    pub fn repartition(
        &mut self,
        expected_epoch: u64,
        hosts: &[HostId],
        registry: &HostRegistry,
        now_tick: u64,
    ) -> Result<LayoutChange, PartitionError> {
        self.ensure_between_ticks()?;
        if expected_epoch != self.epoch {
            return Err(PartitionError::StaleLayout {
                expected: expected_epoch,
                actual: self.epoch,
            });
        }
        if hosts.is_empty() {
            return Err(PartitionError::NoHosts);
        }
        let mut seen = BTreeSet::new();
        for host_id in hosts {
            if !seen.insert(host_id) {
                return Err(PartitionError::DuplicateHost(host_id.clone()));
            }
            registry.ensure_live(host_id, now_tick)?;
        }

        let cuts = self.balanced_cuts(hosts.len());
        let partitions = cuts.len() + 1;
        let before = self.ownership();
        let changed = cuts != self.cuts || hosts[..partitions] != self.hosts[..];
        if changed {
            self.cuts = cuts;
            self.hosts = hosts[..partitions].to_vec();
            self.epoch += 1;
        }
        Ok(LayoutChange {
            layout: self.layout(),
            migrations: self.migrations(&before),
            released_hosts: hosts[partitions..].to_vec(),
        })
    }

    #[must_use]
    pub fn capacity_conditions(&self) -> Vec<CapacityCondition> {
        let capacity = self.config.partition_capacity;
        let mut conditions = self
            .islands()
            .into_iter()
            .filter(|island| island.load() > capacity)
            .map(|island| CapacityCondition::OversizedIsland {
                representative: island.entities[0],
                load: island.load(),
            })
            .collect::<Vec<_>>();
        let mut loads = vec![0_u32; self.hosts.len()];
        for column in self.entities.values() {
            loads[self.index_of(*column)] += 1;
        }
        conditions.extend(
            loads
                .into_iter()
                .enumerate()
                .filter(|(_, load)| *load > capacity)
                .map(|(index, load)| CapacityCondition::PartitionOverCapacity {
                    partition: self.partition_ref(index),
                    load,
                }),
        );
        conditions
    }

    /// Accepts a command for the current tick and routes it to the entity's
    /// owner. Sequences are tracked per logical zone, so they survive
    /// migration; stale or duplicate sequences fail closed.
    pub fn submit_command(
        &mut self,
        entity_id: EntityId,
        sequence: u64,
    ) -> Result<PartitionRef, PartitionError> {
        let owner = self.owner(entity_id)?;
        if let Some(&last) = self.last_sequence.get(&entity_id)
            && sequence <= last
        {
            return Err(PartitionError::StaleCommand {
                entity_id,
                sequence,
                last,
            });
        }
        self.last_sequence.insert(entity_id, sequence);
        self.pending_commands.insert((entity_id, sequence));
        Ok(owner)
    }

    /// Commands `partition` applies this tick, ordered by entity then
    /// sequence independently of the layout.
    pub fn commands_for(
        &self,
        partition: PartitionRef,
    ) -> Result<Vec<(EntityId, u64)>, PartitionError> {
        self.ensure_partition(partition)?;
        Ok(self
            .pending_commands
            .iter()
            .filter(|(entity, _)| self.owner(*entity) == Ok(partition))
            .copied()
            .collect())
    }

    /// Reports that `partition` finished `tick` at `cost_micros` of real
    /// time. The zone advances only once every partition has reported.
    pub fn report_tick(
        &mut self,
        partition: PartitionRef,
        tick: u64,
        cost_micros: u64,
    ) -> Result<TickProgress, PartitionError> {
        self.ensure_partition(partition)?;
        if tick != self.tick {
            return Err(PartitionError::WrongTick {
                reported: tick,
                current: self.tick,
            });
        }
        match self.reports.get(&partition.index) {
            Some(&previous) if previous != cost_micros => {
                return Err(PartitionError::ConflictingTickReport { partition, tick });
            }
            _ => {
                self.reports.insert(partition.index, cost_micros);
            }
        }
        let partitions = u32::try_from(self.hosts.len()).unwrap_or(u32::MAX);
        let reported = u32::try_from(self.reports.len()).unwrap_or(u32::MAX);
        if reported < partitions {
            return Ok(TickProgress::Waiting {
                reported,
                partitions,
            });
        }

        let slowest = self.reports.values().copied().max().unwrap_or(0);
        let simulation_micros = self.config.tick_micros;
        let real_micros = slowest.max(simulation_micros);
        let record = TickRecord {
            tick,
            simulation_micros,
            real_micros,
            dilation_ppm: ratio_ppm(simulation_micros, real_micros),
        };
        self.simulation_micros = self.simulation_micros.saturating_add(simulation_micros);
        self.real_micros = self.real_micros.saturating_add(real_micros);
        self.tick += 1;
        self.reports.clear();
        self.pending_commands.clear();
        Ok(TickProgress::Completed(record))
    }

    fn ensure_between_ticks(&self) -> Result<(), PartitionError> {
        if self.reports.is_empty() {
            Ok(())
        } else {
            Err(PartitionError::TickInProgress { tick: self.tick })
        }
    }

    fn ensure_column(&self, column: u32) -> Result<(), PartitionError> {
        if column < self.config.columns {
            Ok(())
        } else {
            Err(PartitionError::ColumnOutOfRange {
                column,
                columns: self.config.columns,
            })
        }
    }

    fn ensure_partition(&self, partition: PartitionRef) -> Result<(), PartitionError> {
        if partition.layout_epoch != self.epoch {
            return Err(PartitionError::StaleLayout {
                expected: partition.layout_epoch,
                actual: self.epoch,
            });
        }
        if partition.index as usize >= self.hosts.len() {
            return Err(PartitionError::UnknownPartition(partition));
        }
        Ok(())
    }

    fn partition_ref(&self, index: usize) -> PartitionRef {
        PartitionRef {
            layout_epoch: self.epoch,
            index: u32::try_from(index).expect("partition count fits u32"),
        }
    }

    fn partition(&self, index: usize) -> Partition {
        Partition {
            index: self.partition_ref(index).index,
            host_id: self.hosts[index].clone(),
            columns: self.columns_of(index),
        }
    }

    fn columns_of(&self, index: usize) -> Range<u32> {
        let start = if index == 0 { 0 } else { self.cuts[index - 1] };
        let end = self.cuts.get(index).copied().unwrap_or(self.config.columns);
        start..end
    }

    fn index_of(&self, column: u32) -> usize {
        self.cuts.partition_point(|cut| *cut <= column)
    }

    /// A cut at `cut` separates column `cut - 1` from column `cut`; it is
    /// valid when no island has entities on both sides.
    fn valid_cut(islands: &[InteractionIsland], cut: u32) -> bool {
        islands
            .iter()
            .all(|island| !(island.min_column < cut && cut <= island.max_column))
    }

    /// Moves each cut that splits an island to the nearest valid column
    /// between its neighbours, dropping it (coalescing two partitions, the
    /// left host keeps the merged strip) when none exists.
    fn reconcile(&mut self, before: &Ownership) -> Vec<Migration> {
        let islands = self.islands();
        let mut cuts = Vec::with_capacity(self.cuts.len());
        let mut hosts = vec![self.hosts[0].clone()];
        for (position, &cut) in self.cuts.iter().enumerate() {
            let low = cuts.last().copied().unwrap_or(0);
            let high = self
                .cuts
                .get(position + 1)
                .copied()
                .unwrap_or(self.config.columns);
            let chosen = if Self::valid_cut(&islands, cut) {
                Some(cut)
            } else {
                (low + 1..high)
                    .filter(|candidate| Self::valid_cut(&islands, *candidate))
                    .min_by_key(|candidate| (candidate.abs_diff(cut), *candidate))
            };
            if let Some(chosen) = chosen {
                cuts.push(chosen);
                hosts.push(self.hosts[position + 1].clone());
            }
        }
        if cuts != self.cuts {
            self.cuts = cuts;
            self.hosts = hosts;
            self.epoch += 1;
        }
        self.migrations(before)
    }

    /// Load-balanced cuts for `partitions` strips that never split an island.
    fn balanced_cuts(&self, partitions: usize) -> Vec<u32> {
        let islands = self.islands();
        let columns = self.config.columns;
        let mut loads = vec![0_u64; columns as usize];
        for column in self.entities.values() {
            loads[*column as usize] += 1;
        }
        let mut prefix = vec![0_u64; columns as usize + 1];
        for (column, load) in loads.iter().enumerate() {
            prefix[column + 1] = prefix[column] + load;
        }
        let total = prefix[columns as usize];
        let parts = partitions as u64;
        let mut cuts = Vec::new();
        for target in 1..parts {
            let low = cuts.last().copied().unwrap_or(0);
            let best = (low + 1..columns)
                .filter(|cut| Self::valid_cut(&islands, *cut))
                .min_by_key(|cut| {
                    let load_error = (prefix[*cut as usize] * parts).abs_diff(target * total);
                    let width_error =
                        (u64::from(*cut) * parts).abs_diff(target * u64::from(columns));
                    (load_error, width_error, *cut)
                });
            match best {
                Some(cut) => cuts.push(cut),
                None => break,
            }
        }
        cuts
    }

    fn ownership(&self) -> Ownership {
        self.entities
            .iter()
            .map(|(entity, column)| {
                let index = self.index_of(*column);
                (
                    *entity,
                    (self.partition_ref(index), self.hosts[index].clone()),
                )
            })
            .collect()
    }

    /// Entities whose host changed since `before`; hosts in one layout are
    /// distinct, so an unchanged host means the entity stayed on its machine.
    fn migrations(&self, before: &Ownership) -> Vec<Migration> {
        self.ownership()
            .into_iter()
            .filter_map(|(entity_id, to)| {
                let from = before.get(&entity_id).cloned();
                if from.as_ref().is_some_and(|(_, host_id)| *host_id == to.1) {
                    return None;
                }
                Some(Migration {
                    entity_id,
                    from,
                    to,
                })
            })
            .collect()
    }
}

fn find(parent: &mut BTreeMap<EntityId, EntityId>, entity: EntityId) -> EntityId {
    let mut root = entity;
    while parent[&root] != root {
        root = parent[&root];
    }
    let mut current = entity;
    while parent[&current] != root {
        let next = parent[&current];
        parent.insert(current, root);
        current = next;
    }
    root
}

fn ratio_ppm(numerator: u64, denominator: u64) -> u64 {
    u64::try_from(u128::from(numerator) * u128::from(PARTS_PER_MILLION) / u128::from(denominator))
        .unwrap_or(u64::MAX)
}
