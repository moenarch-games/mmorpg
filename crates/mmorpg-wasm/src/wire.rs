//! The browser's wire boundary (#541): projections decode, and commands
//! encode, through `mmorpg-protocol` itself. Each projection and command
//! crosses to JavaScript as a plain object (`serde-wasm-bindgen`) in the shape
//! the browser presents, and `tsify` generates its TypeScript type, so a wire
//! layout change never touches the browser.
//!
//! The types below are that browser shape: camelCase fields, kebab-case or
//! lowercase names for enum values, `bigint` for 64-bit values and `null` for
//! absent options. Each converts to and from its `mmorpg-core` type; the
//! encode direction exists for tests and validates like the core does.

use mmorpg_core as core;
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// A failed conversion in the encode direction.
type Convert<T> = Result<T, String>;

/// A browser enum that mirrors a core enum value by value, with its values in order.
macro_rules! mirror {
    ($(#[$meta:meta])* $name:ident = $core:ty { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
        $(#[$meta])*
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &[Self] = &[$(Self::$variant),+];
        }

        impl From<$core> for $name {
            fn from(value: $core) -> Self {
                type Core = $core;
                match value {
                    $(Core::$variant => Self::$variant),+
                }
            }
        }

        impl From<$name> for $core {
            fn from(value: $name) -> Self {
                match value {
                    $($name::$variant => Self::$variant),+
                }
            }
        }
    };
}

mirror!(
    #[serde(rename_all = "lowercase")]
    EntityKind = core::EntityKind {
        Player,
        Creature,
        Npc,
        Minion
    }
);
mirror!(
    /// A player climbing, mantling (#490) or rolling (#516).
    #[serde(rename_all = "lowercase")]
    Traversal = core::TraversalView { Free, Climbing, Mantling, Rolling }
);
mirror!(
    #[serde(rename_all = "lowercase")]
    ClassId = core::PlayerClass {
        Warden,
        Ranger,
        Arcanist,
        Necromancer,
        Rogue,
        Cleric,
        Druid
    }
);
mirror!(
    #[serde(rename_all = "lowercase")]
    Sex = core::Sex { Female, Male }
);
mirror!(
    #[serde(rename_all = "lowercase")]
    ResourceKind = core::ResourceKind {
        Rage,
        Focus,
        Mana,
        Faith
    }
);
mirror!(
    #[serde(rename_all = "kebab-case")]
    AuraKind = core::AuraKind {
        DamageOverTime,
        HealOverTime,
        Absorb,
        Root,
        Snare,
        Stun,
        Haste
    }
);
mirror!(
    /// A directed strike's sweep side (#154).
    #[serde(rename_all = "lowercase")]
    StrikeSide = core::StrikeSide { Left, Centre, Right }
);
mirror!(
    /// A directed strike's height and a guard's lane (#154, #156).
    #[serde(rename_all = "lowercase")]
    StrikeHeight = core::StrikeHeight { Low, Middle, High }
);
mirror!(
    /// A living Warden's movement-dependent melee stance (#155).
    #[serde(rename_all = "lowercase")]
    MeleeStance = core::MeleeStance { Planted, Charging, Moving, Airborne, Landing }
);
mirror!(
    /// A pending sword → Shield Bash follow-up (#157).
    #[serde(rename_all = "lowercase")]
    ComboPhase = core::ComboPhase { Impact, Open }
);
mirror!(
    #[serde(rename_all = "kebab-case")]
    ErrorCode = core::ErrorCode {
        NoTarget,
        OutOfRange,
        TargetDead,
        NotAttackable,
        YouAreDead,
        NotDead,
        InvalidTarget,
        TooManyIntents,
        InvalidInventoryMove,
        InventoryFull,
        InvalidLoot,
        NotLootOwner,
        EmptyLoot,
        MoneyOverflow,
        NoClass,
        NotLearned,
        NotReady,
        NotEnoughResource,
        Stunned,
        AlreadyCasting,
        LineOfSight,
        InvalidClass,
        NotEquippable,
        InvalidVendor,
        NotEnoughMoney,
        ChatThrottled,
        InvalidQuest,
        QuestLogFull,
        QuestIncomplete,
        DoorBlocked,
        VendorClosed,
        DoorBusy,
        NoMinions,
        NotWhileSwimming,
        HandoffCorridor,
        InCombat,
        InvalidWaystone,
        PrivateHome,
        NoAnswer,
        NotAtNight,
    }
);
mirror!(
    /// `available` is `!`, `complete` is `?` and `in-progress` is a grey `?`.
    #[serde(rename_all = "kebab-case")]
    QuestMarkerKind = core::QuestMarker { Available, InProgress, Complete }
);
mirror!(
    /// A destructible piece's damage stage (#305); intact pieces are not sent.
    #[serde(rename_all = "lowercase")]
    PieceStage = core::PieceStage { Intact, Damaged, Crumbling, Destroyed }
);
mirror!(
    #[serde(rename_all = "lowercase")]
    ProjectileKind = core::ProjectileKind {
        Arrow,
        Bolt,
        Missile,
        Shadow,
        Holy,
        Nature,
        Knife,
        Flask,
        Bomb
    }
);
mirror!(
    /// The breakable prop a debris piece came from (#288).
    #[serde(rename_all = "snake_case")]
    DebrisKind = core::PropKind { Crate, Barrel, ExplosiveBarrel, Fence, HeavyCrate }
);
mirror!(
    /// The core emote vocabulary (#69).
    #[serde(rename_all = "lowercase")]
    EmoteName = core::Emote { Wave, Bow, Cheer, Laugh, Point }
);
mirror!(
    #[serde(rename_all = "lowercase")]
    ChatChannel = core::ChatChannel { Say, Yell }
);

/// Unit identity shared by commands and projections.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: u32,
}

impl From<core::EntityRef> for EntityRef {
    fn from(entity: core::EntityRef) -> Self {
        Self {
            kind: entity.kind().into(),
            id: entity.id(),
        }
    }
}

impl From<EntityRef> for core::EntityRef {
    fn from(entity: EntityRef) -> Self {
        Self::new(entity.kind.into(), entity.id)
    }
}

fn entity(entity: Option<core::EntityRef>) -> Option<EntityRef> {
    entity.map(Into::into)
}

fn core_entity(entity: Option<EntityRef>) -> Option<core::EntityRef> {
    entity.map(Into::into)
}

/// Presentation flags of a visible unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct EntityFlags {
    pub dead: bool,
    pub in_combat: bool,
    /// An aggressive creature.
    pub hostile: bool,
    /// The viewer may attack it (living creatures).
    pub attackable: bool,
    /// A creature tapped by another player.
    pub tapped_by_other: bool,
    pub evading: bool,
    /// Its own target is the viewer.
    pub targets_viewer: bool,
    pub lootable: bool,
    pub traversal: Traversal,
}

/// One visible unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct EntityState {
    pub kind: EntityKind,
    pub entity_id: u32,
    /// Creature template ID, NPC ID, or for players 0 without a class and otherwise `1 + class × 2 + sex`.
    pub appearance: u16,
    #[tsify(type = "Vector3")]
    pub position: [i32; 3],
    /// Presentation velocity in units per tick, saturated to the i8 wire range.
    #[tsify(type = "Vector3")]
    pub velocity: [i8; 3],
    /// u16 yaw: 65 536 steps per turn, 0 faces +Z, increasing turns toward +X.
    pub facing: u16,
    pub level: u8,
    /// Rounded up, so living units never show 0.
    pub health_percent: u8,
    pub flags: EntityFlags,
}

impl From<&core::EntitySnapshot> for EntityState {
    fn from(unit: &core::EntitySnapshot) -> Self {
        let flags = unit.flags;
        Self {
            kind: unit.kind.into(),
            entity_id: unit.id,
            appearance: unit.appearance,
            position: unit.position,
            velocity: unit.velocity,
            facing: unit.facing,
            level: unit.level,
            health_percent: unit.health_percent,
            flags: EntityFlags {
                dead: flags.dead,
                in_combat: flags.in_combat,
                hostile: flags.hostile,
                attackable: flags.attackable,
                tapped_by_other: flags.tapped_by_other,
                evading: flags.evading,
                targets_viewer: flags.targets_viewer,
                lootable: flags.lootable,
                traversal: flags.traversal.into(),
            },
        }
    }
}

impl From<&EntityState> for core::EntitySnapshot {
    fn from(unit: &EntityState) -> Self {
        let flags = unit.flags;
        Self {
            kind: unit.kind.into(),
            id: unit.entity_id,
            appearance: unit.appearance,
            position: unit.position,
            velocity: unit.velocity,
            facing: unit.facing,
            level: unit.level,
            health_percent: unit.health_percent,
            flags: core::EntityFlags {
                dead: flags.dead,
                in_combat: flags.in_combat,
                hostile: flags.hostile,
                attackable: flags.attackable,
                tapped_by_other: flags.tapped_by_other,
                evading: flags.evading,
                targets_viewer: flags.targets_viewer,
                lootable: flags.lootable,
                traversal: flags.traversal.into(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct ClassChoice {
    pub class_id: ClassId,
    pub sex: Sex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ResourceState {
    pub kind: ResourceKind,
    pub value: u16,
    pub max: u16,
}

/// A cast or channel in progress: `elapsed` of `total` ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct CastState {
    pub ability: u8,
    pub elapsed: u16,
    pub total: u16,
    pub channel: bool,
}

impl From<core::CastView> for CastState {
    fn from(cast: core::CastView) -> Self {
        Self {
            ability: cast.ability.get(),
            elapsed: cast.elapsed,
            total: cast.total,
            channel: cast.channel,
        }
    }
}

impl From<CastState> for core::CastView {
    fn from(cast: CastState) -> Self {
        Self {
            ability: core::AbilityId::new(cast.ability),
            elapsed: cast.elapsed,
            total: cast.total,
            channel: cast.channel,
        }
    }
}

/// `amount`: total of a damage/heal over time, remaining shield, snare/haste percent, 0 otherwise.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct AuraState {
    pub ability: u8,
    pub kind: AuraKind,
    pub remaining: u16,
    pub amount: u16,
}

impl From<&core::AuraView> for AuraState {
    fn from(aura: &core::AuraView) -> Self {
        Self {
            ability: aura.ability.get(),
            kind: aura.kind.into(),
            remaining: aura.remaining,
            amount: aura.amount,
        }
    }
}

impl From<&AuraState> for core::AuraView {
    fn from(aura: &AuraState) -> Self {
        Self {
            ability: core::AbilityId::new(aura.ability),
            kind: aura.kind.into(),
            remaining: aura.remaining,
            amount: aura.amount,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct CooldownState {
    pub ability: u8,
    pub remaining: u16,
}

/// Inclusive melee damage range, the equipment bonus included.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct DamageRange {
    pub min: u16,
    pub max: u16,
}

/// The shield guard (#156): the held lane, whether the zone blocks in it now, break recovery ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct GuardState {
    pub raised: Option<StrikeHeight>,
    pub protecting: bool,
    pub broken: u16,
}

/// The viewer's own exact state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct ViewerState {
    pub copper: u32,
    pub experience: u32,
    pub experience_to_next_level: u32,
    pub health: u32,
    pub max_health: u32,
    pub level: u8,
    pub dead: bool,
    pub in_combat: bool,
    pub target: Option<EntityRef>,
    pub class_choice: Option<ClassChoice>,
    /// Present exactly when the viewer has a class.
    pub resource: Option<ResourceState>,
    pub cast: Option<CastState>,
    pub global_cooldown: u16,
    pub damage: DamageRange,
    pub guard: GuardState,
    /// A living Warden's movement-dependent melee stance (#155); null for other players.
    pub stance: Option<MeleeStance>,
    /// A pending sword → Shield Bash follow-up (#157): the slash still landing, or the window open.
    pub combo: Option<ComboPhase>,
    /// Ticks of breath left (#274); at 0 a submerged player drowns.
    pub breath: u16,
    /// Waystone IDs (#145) the viewer unlocked by exploration, ascending.
    pub waystones: Vec<u8>,
}

/// The viewer's target's cast and auras; empty without a living visible target.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
pub struct TargetDetail {
    pub cast: Option<CastState>,
    pub auras: Vec<AuraState>,
}

/// An item stack in a bag slot or on a corpse.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct ItemStack {
    pub item_id: u16,
    pub quantity: u16,
}

impl From<core::ItemStack> for ItemStack {
    fn from(stack: core::ItemStack) -> Self {
        Self {
            item_id: stack.item().get(),
            quantity: stack.quantity(),
        }
    }
}

impl TryFrom<ItemStack> for core::ItemStack {
    type Error = String;

    fn try_from(stack: ItemStack) -> Convert<Self> {
        Self::new(core::ItemId::new(stack.item_id), stack.quantity)
            .map_err(|error| format!("{error:?}"))
    }
}

/// The sums of the equipped items' attributes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct StatTotals {
    pub stamina: u16,
    pub strength: u16,
    pub agility: u16,
    pub intellect: u16,
}

/// The selected corpse's complete remaining rewards.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct LootView {
    pub creature_id: u32,
    #[tsify(type = "bigint")]
    pub died_at: u64,
    pub money: u32,
    pub item: Option<ItemStack>,
    /// One unit of a quest item, dropped while a quest of the viewer needs it.
    pub quest_item: Option<ItemStack>,
}

/// An active quest and the current progress of each objective slot (collect counts included).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct QuestEntry {
    pub quest: u8,
    pub progress: [u8; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct QuestMarker {
    pub npc: u32,
    pub marker: QuestMarkerKind,
}

/// The viewer's quests: turned-in quests (bit `id - 1`), the log and the markers of quest NPCs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct QuestSheet {
    pub completed: u32,
    pub entries: Vec<QuestEntry>,
    pub markers: Vec<QuestMarker>,
}

/// A door as every player sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct DoorState {
    pub door: u8,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct PieceState {
    pub piece: u16,
    pub stage: PieceStage,
}

/// The night raid as seen near its target (#530): the assault phase (1–3, or 0 once it resolved
/// while the barn still recovers) and the barn's integrity in percent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct RaidSignal {
    pub phase: u8,
    pub integrity: u8,
}

/// One projectile: position in units, heading/pitch in 1/65 536 turns (heading 0 is +Z).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ProjectileState {
    pub id: u16,
    pub kind: ProjectileKind,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub yaw: u16,
    pub pitch: i16,
    pub stuck: bool,
    /// A thrown flask or bomb bursting where it is (#290).
    pub burst: bool,
}

/// One piece of a broken prop: position in units, rotation as a quaternion in 1/32 767 steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct DebrisState {
    pub id: u16,
    pub kind: DebrisKind,
    pub piece: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub rotation: [i16; 4],
    /// Ticks since the break, saturating at 255.
    pub age: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct StrikeAim {
    pub side: StrikeSide,
    pub height: StrikeHeight,
}

impl From<core::StrikeAim> for StrikeAim {
    fn from(aim: core::StrikeAim) -> Self {
        Self {
            side: aim.side.into(),
            height: aim.height.into(),
        }
    }
}

impl From<StrikeAim> for core::StrikeAim {
    fn from(aim: StrikeAim) -> Self {
        Self::new(aim.side.into(), aim.height.into())
    }
}

/// Feedback the viewer received in the projection's tick; cosmetic and lossy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[tsify(missing_as_null)]
pub enum ZoneEvent {
    /// An ordinary attack rolled at `target`; a player's swing that covered nobody (#517) whiffs with null.
    AttackPerformed {
        source: EntityRef,
        target: Option<EntityRef>,
    },
    DamageDealt {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
        critical: bool,
    },
    DamageTaken {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
        critical: bool,
    },
    DamageObserved {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
        critical: bool,
    },
    Miss {
        source: EntityRef,
        target: EntityRef,
    },
    Evade {
        source: EntityRef,
        target: EntityRef,
    },
    Died {
        entity: EntityRef,
        killer: Option<EntityRef>,
    },
    Error {
        code: ErrorCode,
        target: Option<EntityRef>,
    },
    CastStarted {
        source: EntityRef,
        target: Option<EntityRef>,
        ability: u8,
        ticks: u16,
    },
    AbilityUsed {
        source: EntityRef,
        target: Option<EntityRef>,
        ability: u8,
    },
    /// `source` swept a directed strike toward `aim`; `target` is the creature it struck, null a whiff.
    StrikeSwept {
        source: EntityRef,
        target: Option<EntityRef>,
        ability: u8,
        aim: StrikeAim,
    },
    /// `target`'s guard fully blocked `source`'s blow of `amount` in `lane` (#156).
    Blocked {
        source: EntityRef,
        target: EntityRef,
        lane: StrikeHeight,
        amount: u16,
    },
    /// `source`'s blow overpowered `target`'s guard in `lane`; it recovers for `ticks` ticks.
    GuardBroken {
        source: EntityRef,
        target: EntityRef,
        lane: StrikeHeight,
        ticks: u16,
    },
    Healed {
        source: EntityRef,
        target: EntityRef,
        ability: u8,
        amount: u16,
    },
    AuraApplied {
        source: EntityRef,
        target: EntityRef,
        ability: u8,
        ticks: u16,
    },
    AuraRemoved {
        source: EntityRef,
        target: EntityRef,
        ability: u8,
    },
    Interrupted {
        source: Option<EntityRef>,
        target: EntityRef,
        ability: u8,
    },
    Absorbed {
        source: EntityRef,
        target: EntityRef,
        amount: u16,
    },
    /// Objective `objective` (its index) of `quest` reached `count`.
    QuestProgress { quest: u8, objective: u8, count: u8 },
    /// The viewer turned `quest` in.
    QuestCompleted { quest: u8 },
    /// The trailing lurker `source` lost interest in the diving viewer and swims home (#274).
    LostInterest { source: EntityRef },
}

impl From<core::ZoneEvent> for ZoneEvent {
    fn from(event: core::ZoneEvent) -> Self {
        use core::ZoneEvent as E;
        match event {
            E::AttackPerformed { source, target } => Self::AttackPerformed {
                source: source.into(),
                target: entity(target),
            },
            E::DamageDealt {
                source,
                target,
                amount,
                critical,
            } => Self::DamageDealt {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::DamageTaken {
                source,
                target,
                amount,
                critical,
            } => Self::DamageTaken {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::DamageObserved {
                source,
                target,
                amount,
                critical,
            } => Self::DamageObserved {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::Miss { source, target } => Self::Miss {
                source: source.into(),
                target: target.into(),
            },
            E::Evade { source, target } => Self::Evade {
                source: source.into(),
                target: target.into(),
            },
            E::Died {
                entity: died,
                killer,
            } => Self::Died {
                entity: died.into(),
                killer: entity(killer),
            },
            E::Error { code, target } => Self::Error {
                code: code.into(),
                target: entity(target),
            },
            E::CastStarted {
                source,
                target,
                ability,
                ticks,
            } => Self::CastStarted {
                source: source.into(),
                target: entity(target),
                ability: ability.get(),
                ticks,
            },
            E::AbilityUsed {
                source,
                target,
                ability,
            } => Self::AbilityUsed {
                source: source.into(),
                target: entity(target),
                ability: ability.get(),
            },
            E::StrikeSwept {
                source,
                target,
                ability,
                aim,
            } => Self::StrikeSwept {
                source: source.into(),
                target: entity(target),
                ability: ability.get(),
                aim: aim.into(),
            },
            E::Blocked {
                source,
                target,
                lane,
                amount,
            } => Self::Blocked {
                source: source.into(),
                target: target.into(),
                lane: lane.into(),
                amount,
            },
            E::GuardBroken {
                source,
                target,
                lane,
                ticks,
            } => Self::GuardBroken {
                source: source.into(),
                target: target.into(),
                lane: lane.into(),
                ticks,
            },
            E::Healed {
                source,
                target,
                ability,
                amount,
            } => Self::Healed {
                source: source.into(),
                target: target.into(),
                ability: ability.get(),
                amount,
            },
            E::AuraApplied {
                source,
                target,
                ability,
                ticks,
            } => Self::AuraApplied {
                source: source.into(),
                target: target.into(),
                ability: ability.get(),
                ticks,
            },
            E::AuraRemoved {
                source,
                target,
                ability,
            } => Self::AuraRemoved {
                source: source.into(),
                target: target.into(),
                ability: ability.get(),
            },
            E::Interrupted {
                source,
                target,
                ability,
            } => Self::Interrupted {
                source: entity(source),
                target: target.into(),
                ability: ability.get(),
            },
            E::Absorbed {
                source,
                target,
                amount,
            } => Self::Absorbed {
                source: source.into(),
                target: target.into(),
                amount,
            },
            E::QuestProgress {
                quest,
                objective,
                count,
            } => Self::QuestProgress {
                quest: quest.get(),
                objective,
                count,
            },
            E::QuestCompleted { quest } => Self::QuestCompleted { quest: quest.get() },
            E::LostInterest { source } => Self::LostInterest {
                source: source.into(),
            },
        }
    }
}

impl From<ZoneEvent> for core::ZoneEvent {
    fn from(event: ZoneEvent) -> Self {
        use ZoneEvent as E;
        use core::{AbilityId, QuestId};
        match event {
            E::AttackPerformed { source, target } => Self::AttackPerformed {
                source: source.into(),
                target: core_entity(target),
            },
            E::DamageDealt {
                source,
                target,
                amount,
                critical,
            } => Self::DamageDealt {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::DamageTaken {
                source,
                target,
                amount,
                critical,
            } => Self::DamageTaken {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::DamageObserved {
                source,
                target,
                amount,
                critical,
            } => Self::DamageObserved {
                source: source.into(),
                target: target.into(),
                amount,
                critical,
            },
            E::Miss { source, target } => Self::Miss {
                source: source.into(),
                target: target.into(),
            },
            E::Evade { source, target } => Self::Evade {
                source: source.into(),
                target: target.into(),
            },
            E::Died {
                entity: died,
                killer,
            } => Self::Died {
                entity: died.into(),
                killer: core_entity(killer),
            },
            E::Error { code, target } => Self::Error {
                code: code.into(),
                target: core_entity(target),
            },
            E::CastStarted {
                source,
                target,
                ability,
                ticks,
            } => Self::CastStarted {
                source: source.into(),
                target: core_entity(target),
                ability: AbilityId::new(ability),
                ticks,
            },
            E::AbilityUsed {
                source,
                target,
                ability,
            } => Self::AbilityUsed {
                source: source.into(),
                target: core_entity(target),
                ability: AbilityId::new(ability),
            },
            E::StrikeSwept {
                source,
                target,
                ability,
                aim,
            } => Self::StrikeSwept {
                source: source.into(),
                target: core_entity(target),
                ability: AbilityId::new(ability),
                aim: aim.into(),
            },
            E::Blocked {
                source,
                target,
                lane,
                amount,
            } => Self::Blocked {
                source: source.into(),
                target: target.into(),
                lane: lane.into(),
                amount,
            },
            E::GuardBroken {
                source,
                target,
                lane,
                ticks,
            } => Self::GuardBroken {
                source: source.into(),
                target: target.into(),
                lane: lane.into(),
                ticks,
            },
            E::Healed {
                source,
                target,
                ability,
                amount,
            } => Self::Healed {
                source: source.into(),
                target: target.into(),
                ability: AbilityId::new(ability),
                amount,
            },
            E::AuraApplied {
                source,
                target,
                ability,
                ticks,
            } => Self::AuraApplied {
                source: source.into(),
                target: target.into(),
                ability: AbilityId::new(ability),
                ticks,
            },
            E::AuraRemoved {
                source,
                target,
                ability,
            } => Self::AuraRemoved {
                source: source.into(),
                target: target.into(),
                ability: AbilityId::new(ability),
            },
            E::Interrupted {
                source,
                target,
                ability,
            } => Self::Interrupted {
                source: core_entity(source),
                target: target.into(),
                ability: AbilityId::new(ability),
            },
            E::Absorbed {
                source,
                target,
                amount,
            } => Self::Absorbed {
                source: source.into(),
                target: target.into(),
                amount,
            },
            E::QuestProgress {
                quest,
                objective,
                count,
            } => Self::QuestProgress {
                quest: QuestId::new(quest),
                objective,
                count,
            },
            E::QuestCompleted { quest } => Self::QuestCompleted {
                quest: QuestId::new(quest),
            },
            E::LostInterest { source } => Self::LostInterest {
                source: source.into(),
            },
        }
    }
}

/// A line the viewer heard: player `speaker` said or yelled `text`, or emoted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(tag = "channel", rename_all = "lowercase")]
pub enum ChatLine {
    Say { speaker: u32, text: String },
    Yell { speaker: u32, text: String },
    Emote { speaker: u32, emote: EmoteName },
}

impl From<&core::ChatLine> for ChatLine {
    fn from(line: &core::ChatLine) -> Self {
        let speaker = line.speaker;
        match line.message {
            core::ChatMessage::Say(text) => Self::Say {
                speaker,
                text: text.as_str().to_owned(),
            },
            core::ChatMessage::Yell(text) => Self::Yell {
                speaker,
                text: text.as_str().to_owned(),
            },
            core::ChatMessage::Emote(emote) => Self::Emote {
                speaker,
                emote: emote.into(),
            },
        }
    }
}

impl TryFrom<&ChatLine> for core::ChatLine {
    type Error = String;

    fn try_from(line: &ChatLine) -> Convert<Self> {
        let (speaker, message) = match line {
            ChatLine::Say { speaker, text } => (*speaker, core::ChatMessage::Say(chat_text(text)?)),
            ChatLine::Yell { speaker, text } => {
                (*speaker, core::ChatMessage::Yell(chat_text(text)?))
            }
            ChatLine::Emote { speaker, emote } => {
                (*speaker, core::ChatMessage::Emote((*emote).into()))
            }
        };
        Ok(Self { speaker, message })
    }
}

fn chat_text(text: &str) -> Convert<core::ChatText> {
    core::ChatText::new(text).map_err(|error| error.to_string())
}

/// A player-visible projection as the browser presents it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null, large_number_types_as_bigints)]
pub struct ZoneSnapshot {
    pub zone_id: u32,
    #[tsify(type = "bigint")]
    pub tick: u64,
    #[tsify(type = "bigint")]
    pub content_revision: u64,
    pub acknowledged_sequence: u32,
    /// The player this projection is addressed to ("self").
    pub viewer_id: u32,
    pub viewer: ViewerState,
    /// Running cooldowns in ability order.
    pub cooldowns: Vec<CooldownState>,
    /// The viewer's auras in slot order.
    pub auras: Vec<AuraState>,
    /// The viewer's target's own target.
    pub target_of_target: Option<EntityRef>,
    pub target_detail: TargetDetail,
    #[tsify(type = "bigint")]
    pub inventory_revision: u64,
    /// Complete self bag when present; null means retain prior state, never empty.
    pub inventory: Option<Vec<Option<ItemStack>>>,
    /// Item IDs by equipment slot (0 main hand … 5 feet), null when empty; present exactly with the bag.
    pub equipment: Option<Vec<Option<u16>>>,
    /// The equipment's stat totals, present exactly with the equipment.
    pub stats: Option<StatTotals>,
    /// Quest log, turned-in quests and NPC markers, present exactly with the bag.
    pub quests: Option<QuestSheet>,
    /// Complete eligible selected corpse sheet; null means no current sheet.
    pub loot: Option<LootView>,
    /// Every content door's state in ID order, the same for every viewer (#158).
    pub doors: Vec<DoorState>,
    /// Destructible pieces that are not intact, in ID order (#305).
    pub pieces: Vec<PieceState>,
    /// The night raid's signals while the viewer is near its target (#530); null otherwise.
    pub raid: Option<RaidSignal>,
    pub events: Vec<ZoneEvent>,
    /// Chat lines the viewer heard this tick, in delivery order; cosmetic and lossy like events.
    pub chat: Vec<ChatLine>,
    /// Priority order: the viewer, its target, then nearest first.
    pub entities: Vec<EntityState>,
    /// Arrows and bolts in flight or stuck, nearest first (#286).
    pub projectiles: Vec<ProjectileState>,
    /// Pieces of broken props, nearest first (#288).
    pub debris: Vec<DebrisState>,
}

impl From<&core::ZoneSnapshot> for ZoneSnapshot {
    fn from(snapshot: &core::ZoneSnapshot) -> Self {
        let viewer = &snapshot.viewer;
        let [min, max] = viewer.damage;
        Self {
            zone_id: snapshot.zone_id.get(),
            tick: snapshot.tick,
            content_revision: snapshot.content_revision,
            acknowledged_sequence: snapshot.acknowledged_sequence,
            viewer_id: snapshot.viewer_id,
            viewer: ViewerState {
                copper: viewer.copper,
                experience: viewer.experience,
                experience_to_next_level: viewer.experience_to_next_level,
                health: viewer.health,
                max_health: viewer.max_health,
                level: viewer.level,
                dead: viewer.dead,
                in_combat: viewer.in_combat,
                target: entity(viewer.target),
                class_choice: viewer.class.map(|choice| ClassChoice {
                    class_id: choice.class.into(),
                    sex: choice.sex.into(),
                }),
                resource: viewer.resource.map(|resource| ResourceState {
                    kind: resource.kind.into(),
                    value: resource.value,
                    max: resource.max,
                }),
                cast: viewer.cast.map(Into::into),
                global_cooldown: viewer.global_cooldown,
                damage: DamageRange { min, max },
                guard: GuardState {
                    raised: viewer.guard.raised.map(Into::into),
                    protecting: viewer.guard.protecting,
                    broken: viewer.guard.broken,
                },
                stance: viewer.stance.map(Into::into),
                combo: viewer.combo.map(Into::into),
                breath: viewer.breath,
                waystones: viewer.waystones.iter().map(core::WaystoneId::get).collect(),
            },
            cooldowns: snapshot
                .cooldowns
                .iter()
                .map(|cooldown| CooldownState {
                    ability: cooldown.ability.get(),
                    remaining: cooldown.remaining,
                })
                .collect(),
            auras: snapshot.auras.iter().map(Into::into).collect(),
            target_of_target: entity(snapshot.target_of_target),
            target_detail: TargetDetail {
                cast: snapshot.target_detail.cast.map(Into::into),
                auras: snapshot
                    .target_detail
                    .auras
                    .iter()
                    .map(Into::into)
                    .collect(),
            },
            inventory_revision: snapshot.inventory_revision,
            inventory: snapshot.inventory.as_ref().map(|bag| {
                bag.slots()
                    .iter()
                    .map(|slot| slot.map(Into::into))
                    .collect()
            }),
            equipment: snapshot.equipment.map(|equipment| {
                equipment
                    .slots()
                    .iter()
                    .map(|slot| slot.map(core::ItemId::get))
                    .collect()
            }),
            stats: snapshot.equipment.map(|equipment| {
                let totals = equipment.totals();
                StatTotals {
                    stamina: totals.stamina,
                    strength: totals.strength,
                    agility: totals.agility,
                    intellect: totals.intellect,
                }
            }),
            quests: snapshot.quests.as_ref().map(|sheet| QuestSheet {
                completed: sheet.completed,
                entries: sheet
                    .entries
                    .iter()
                    .map(|entry| QuestEntry {
                        quest: entry.quest.get(),
                        progress: entry.progress,
                    })
                    .collect(),
                markers: sheet
                    .markers
                    .iter()
                    .map(|marker| QuestMarker {
                        npc: marker.npc.get(),
                        marker: marker.marker.into(),
                    })
                    .collect(),
            }),
            loot: snapshot.loot.map(|loot| LootView {
                creature_id: loot.claim.creature.get(),
                died_at: loot.claim.died_at,
                money: loot.rewards.money,
                item: loot.rewards.item.map(Into::into),
                quest_item: loot.rewards.quest_item.map(Into::into),
            }),
            doors: snapshot
                .doors
                .iter()
                .map(|door| DoorState {
                    door: door.door.get(),
                    open: door.open,
                })
                .collect(),
            pieces: snapshot
                .pieces
                .iter()
                .map(|piece| PieceState {
                    piece: piece.piece.get(),
                    stage: piece.stage.into(),
                })
                .collect(),
            raid: snapshot.raid.map(|raid| RaidSignal {
                phase: raid.phase,
                integrity: raid.integrity,
            }),
            events: snapshot.events.iter().copied().map(Into::into).collect(),
            chat: snapshot.chat.iter().map(Into::into).collect(),
            entities: snapshot.entities.iter().map(Into::into).collect(),
            projectiles: snapshot
                .projectiles
                .iter()
                .map(|projectile| {
                    let [x, y, z] = projectile.position;
                    ProjectileState {
                        id: projectile.id,
                        kind: projectile.kind.into(),
                        x,
                        y,
                        z,
                        yaw: projectile.yaw,
                        pitch: projectile.pitch,
                        stuck: projectile.stuck,
                        burst: projectile.burst,
                    }
                })
                .collect(),
            debris: snapshot
                .debris
                .iter()
                .map(|debris| {
                    let [x, y, z] = debris.position;
                    DebrisState {
                        id: debris.id,
                        kind: debris.kind.into(),
                        piece: debris.piece,
                        x,
                        y,
                        z,
                        rotation: debris.rotation,
                        age: debris.age,
                    }
                })
                .collect(),
        }
    }
}

impl TryFrom<&ZoneSnapshot> for core::ZoneSnapshot {
    type Error = String;

    fn try_from(snapshot: &ZoneSnapshot) -> Convert<Self> {
        let viewer = &snapshot.viewer;
        let waystones =
            viewer
                .waystones
                .iter()
                .try_fold(core::WaystoneSet::EMPTY, |set, &id| {
                    core::WaystoneId::new(id)
                        .map(|id| set.with(id))
                        .ok_or_else(|| format!("unknown waystone {id}"))
                })?;
        let inventory = snapshot
            .inventory
            .as_ref()
            .map(|bag| {
                let slots = bag
                    .iter()
                    .map(|slot| slot.map(core::ItemStack::try_from).transpose())
                    .collect::<Convert<Vec<_>>>()?;
                let slots = slots
                    .try_into()
                    .map_err(|_| "a bag has sixteen slots".to_owned())?;
                Ok::<_, String>(core::Inventory::from_slots(slots))
            })
            .transpose()?;
        let equipment = snapshot
            .equipment
            .as_ref()
            .map(|slots| {
                let slots: Vec<_> = slots
                    .iter()
                    .map(|slot| slot.map(core::ItemId::new))
                    .collect();
                let slots = slots
                    .try_into()
                    .map_err(|_| "equipment has six slots".to_owned())?;
                core::Equipment::from_slots(slots).map_err(|error| format!("{error:?}"))
            })
            .transpose()?;
        let loot = snapshot
            .loot
            .map(|loot| {
                Ok::<_, String>(core::LootView {
                    claim: core::LootClaim {
                        creature: core::CreatureId::new(loot.creature_id),
                        died_at: loot.died_at,
                    },
                    rewards: core::LootRewards {
                        money: loot.money,
                        item: loot.item.map(TryInto::try_into).transpose()?,
                        quest_item: loot.quest_item.map(TryInto::try_into).transpose()?,
                    },
                })
            })
            .transpose()?;
        Ok(Self {
            content_revision: snapshot.content_revision,
            acknowledged_sequence: snapshot.acknowledged_sequence,
            viewer_id: snapshot.viewer_id,
            zone_id: core::ZoneId::new(snapshot.zone_id),
            tick: snapshot.tick,
            viewer: core::ViewerState {
                copper: viewer.copper,
                experience: viewer.experience,
                experience_to_next_level: viewer.experience_to_next_level,
                health: viewer.health,
                max_health: viewer.max_health,
                level: viewer.level,
                dead: viewer.dead,
                in_combat: viewer.in_combat,
                target: core_entity(viewer.target),
                class: viewer.class_choice.map(|choice| core::ClassChoice {
                    class: choice.class_id.into(),
                    sex: choice.sex.into(),
                }),
                resource: viewer.resource.map(|resource| core::ResourceView {
                    kind: resource.kind.into(),
                    value: resource.value,
                    max: resource.max,
                }),
                cast: viewer.cast.map(Into::into),
                global_cooldown: viewer.global_cooldown,
                damage: [viewer.damage.min, viewer.damage.max],
                guard: core::GuardView {
                    raised: viewer.guard.raised.map(Into::into),
                    protecting: viewer.guard.protecting,
                    broken: viewer.guard.broken,
                },
                stance: viewer.stance.map(Into::into),
                combo: viewer.combo.map(Into::into),
                breath: viewer.breath,
                waystones,
            },
            cooldowns: snapshot
                .cooldowns
                .iter()
                .map(|cooldown| core::Cooldown {
                    ability: core::AbilityId::new(cooldown.ability),
                    remaining: cooldown.remaining,
                })
                .collect(),
            auras: snapshot.auras.iter().map(Into::into).collect(),
            target_of_target: core_entity(snapshot.target_of_target),
            target_detail: core::TargetDetail {
                cast: snapshot.target_detail.cast.map(Into::into),
                auras: snapshot
                    .target_detail
                    .auras
                    .iter()
                    .map(Into::into)
                    .collect(),
            },
            inventory_revision: snapshot.inventory_revision,
            inventory,
            equipment,
            quests: snapshot.quests.as_ref().map(|sheet| core::QuestSheet {
                completed: sheet.completed,
                entries: sheet
                    .entries
                    .iter()
                    .map(|entry| core::QuestEntry {
                        quest: core::QuestId::new(entry.quest),
                        progress: entry.progress,
                    })
                    .collect(),
                markers: sheet
                    .markers
                    .iter()
                    .map(|marker| core::NpcMarker {
                        npc: core::NpcId::new(marker.npc),
                        marker: marker.marker.into(),
                    })
                    .collect(),
            }),
            loot,
            events: snapshot.events.iter().copied().map(Into::into).collect(),
            chat: snapshot
                .chat
                .iter()
                .map(TryInto::try_into)
                .collect::<Convert<_>>()?,
            entities: snapshot.entities.iter().map(Into::into).collect(),
            doors: snapshot
                .doors
                .iter()
                .map(|door| core::DoorView {
                    door: core::DoorId::new(door.door),
                    open: door.open,
                })
                .collect(),
            projectiles: snapshot
                .projectiles
                .iter()
                .map(|projectile| core::ProjectileView {
                    id: projectile.id,
                    kind: projectile.kind.into(),
                    position: [projectile.x, projectile.y, projectile.z],
                    yaw: projectile.yaw,
                    pitch: projectile.pitch,
                    stuck: projectile.stuck,
                    burst: projectile.burst,
                })
                .collect(),
            debris: snapshot
                .debris
                .iter()
                .map(|debris| core::DebrisView {
                    id: debris.id,
                    kind: debris.kind.into(),
                    piece: debris.piece,
                    position: [debris.x, debris.y, debris.z],
                    rotation: debris.rotation,
                    age: debris.age,
                })
                .collect(),
            pieces: snapshot
                .pieces
                .iter()
                .map(|piece| core::PieceView {
                    piece: core::PieceId::new(piece.piece),
                    stage: piece.stage.into(),
                })
                .collect(),
            raid: snapshot.raid.map(|raid| core::RaidView {
                phase: raid.phase,
                integrity: raid.integrity,
            }),
        })
    }
}

/// A minion group order (#295, Atlas 5.2.84): attack a unit, defend an XZ point (units), follow or retreat.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(tag = "order", rename_all = "lowercase")]
pub enum MinionOrder {
    Follow,
    Attack { target: EntityRef },
    Defend { point: [i32; 2] },
    Retreat,
}

/// One player intent. The session supplies player identity and sequence separately.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[tsify(missing_as_null, large_number_types_as_bigints)]
pub enum WorldCommand {
    Move {
        #[tsify(type = "Axis")]
        forward: i8,
        #[tsify(type = "Axis")]
        strafe: i8,
        facing: u16,
    },
    Loot {
        creature_id: u32,
        #[tsify(type = "bigint")]
        died_at: u64,
    },
    Jump,
    /// A dodge roll (#516) along the held movement, or straight back without any; the zone decides if it can start.
    Roll,
    /// Held swim ascent (`1`), descent (`-1`) or neither (#274); the zone ignores it out of the water.
    Rise {
        #[tsify(type = "Axis")]
        rise: i8,
    },
    /// Selects a unit, or clears the selection with `null`.
    SelectTarget {
        target: Option<EntityRef>,
    },
    /// Swings the weapon once toward `facing` (u16 yaw, #517); the zone hits every unit in the arc.
    Swing {
        facing: u16,
    },
    ReleaseSpirit,
    MoveItem {
        source: u8,
        destination: u8,
        quantity: u16,
    },
    /// A class ability at `target`, or at the current selection with `null`.
    UseAbility {
        ability: u8,
        target: Option<EntityRef>,
    },
    /// The held key that used `ability` went up, or `input-bindings` retired its hold (#414).
    ReleaseAbility {
        ability: u8,
        retired: bool,
    },
    /// Heroic Strike's directed mode: turn to `facing` (u16 yaw) and sweep toward `aim`.
    DirectedStrike {
        ability: u8,
        facing: u16,
        aim: StrikeAim,
    },
    /// Holds the shield guard in `lane`, or lowers it with `null` (#156).
    Guard {
        lane: Option<StrikeHeight>,
    },
    CancelCast,
    /// Class 0 Warden … 6 Druid; sex 0 female, 1 male. The zone refuses invalid values.
    ChooseClass {
        class_id: u8,
        sex: u8,
    },
    /// Equips the item in a bag slot, swapping any item already in its equipment slot.
    EquipItem {
        bag_slot: u8,
    },
    /// Equipment slot 0 main hand, 1 off hand, 2 head, 3 chest, 4 legs, 5 feet.
    UnequipItem {
        equipment_slot: u8,
    },
    /// Buys `quantity` units of the vendor NPC's offer at index `offer`.
    BuyItem {
        npc: u32,
        offer: u8,
        quantity: u16,
    },
    /// Sells `quantity` units from one of the player's own bag slots to the vendor NPC.
    SellItem {
        npc: u32,
        bag_slot: u8,
        quantity: u16,
    },
    /// Says (20 m) or yells (60 m) one line of 1–80 UTF-8 bytes; the zone rate-limits speakers.
    Chat {
        channel: ChatChannel,
        text: String,
    },
    Emote {
        emote: EmoteName,
    },
    /// Accepts `quest` from its giver `npc`; the zone checks reach and availability.
    AcceptQuest {
        npc: u32,
        quest: u8,
    },
    /// Turns `quest` in at its ender `npc`, taking reward `choice` (0 when it offers none).
    CompleteQuest {
        npc: u32,
        quest: u8,
        choice: u8,
    },
    AbandonQuest {
        quest: u8,
    },
    /// Opens (`open`) or closes content door `door` (#158); the zone checks reach and blockers.
    UseDoor {
        door: u8,
        open: bool,
    },
    /// Travels from the waystone in reach to unlocked waystone `waystone` (#145).
    UseWaystone {
        waystone: u8,
    },
    /// Starts repairing a destructible piece (#305), or stops with `null`.
    Repair {
        piece: Option<u16>,
    },
    /// Orders all of a Necromancer's minions at once (#295); the zone checks the target or point.
    CommandMinions {
        order: MinionOrder,
    },
}

fn axis(value: i8) -> Convert<i8> {
    if (-1..=1).contains(&value) {
        Ok(value)
    } else {
        Err("movement components must be -1, 0 or 1".to_owned())
    }
}

impl TryFrom<&WorldCommand> for core::ZoneCommand {
    type Error = String;

    fn try_from(command: &WorldCommand) -> Convert<Self> {
        use WorldCommand as C;
        Ok(match command.clone() {
            C::Move {
                forward,
                strafe,
                facing,
            } => Self::Move {
                forward: axis(forward)?,
                strafe: axis(strafe)?,
                facing,
            },
            C::Loot {
                creature_id,
                died_at,
            } => Self::Loot(core::LootClaim {
                creature: core::CreatureId::new(creature_id),
                died_at,
            }),
            C::Jump => Self::Jump,
            C::Roll => Self::Roll,
            C::Rise { rise } => Self::Rise(axis(rise)?),
            C::SelectTarget { target } => Self::SelectTarget(core_entity(target)),
            C::Swing { facing } => Self::Swing { facing },
            C::ReleaseSpirit => Self::ReleaseSpirit,
            C::MoveItem {
                source,
                destination,
                quantity,
            } => Self::MoveItem {
                source,
                destination,
                quantity,
            },
            C::UseAbility { ability, target } => Self::UseAbility {
                ability,
                target: core_entity(target),
            },
            C::ReleaseAbility { ability, retired } => Self::ReleaseAbility { ability, retired },
            C::DirectedStrike {
                ability,
                facing,
                aim,
            } => Self::DirectedStrike {
                ability,
                facing,
                aim: aim.into(),
            },
            C::Guard { lane } => Self::Guard {
                lane: lane.map(Into::into),
            },
            C::CancelCast => Self::CancelCast,
            C::ChooseClass { class_id, sex } => Self::ChooseClass {
                class: class_id,
                sex,
            },
            C::EquipItem { bag_slot } => Self::EquipItem { bag_slot },
            C::UnequipItem { equipment_slot } => Self::UnequipItem { equipment_slot },
            C::BuyItem {
                npc,
                offer,
                quantity,
            } => Self::BuyItem {
                npc: core::NpcId::new(npc),
                offer,
                quantity,
            },
            C::SellItem {
                npc,
                bag_slot,
                quantity,
            } => Self::SellItem {
                npc: core::NpcId::new(npc),
                bag_slot,
                quantity,
            },
            C::Chat { channel, text } => Self::Chat {
                channel: channel.into(),
                text: chat_text(&text)?,
            },
            C::Emote { emote } => Self::Emote(emote.into()),
            C::AcceptQuest { npc, quest } => Self::AcceptQuest {
                npc: core::NpcId::new(npc),
                quest,
            },
            C::CompleteQuest { npc, quest, choice } => Self::CompleteQuest {
                npc: core::NpcId::new(npc),
                quest,
                choice,
            },
            C::AbandonQuest { quest } => Self::AbandonQuest { quest },
            C::UseDoor { door, open } => Self::UseDoor {
                door: core::DoorId::new(door),
                open,
            },
            C::UseWaystone { waystone } => Self::UseWaystone {
                waystone: core::WaystoneId::new(waystone)
                    .ok_or_else(|| format!("unknown waystone {waystone}"))?,
            },
            C::Repair { piece } => Self::Repair {
                // The wire carries the piece plus one, so the largest ID has no reference.
                piece: piece
                    .map(|piece| {
                        if piece == u16::MAX {
                            Err("piece IDs must be below 65 535".to_owned())
                        } else {
                            Ok(core::PieceId::new(piece))
                        }
                    })
                    .transpose()?,
            },
            C::CommandMinions { order } => Self::CommandMinions(match order {
                MinionOrder::Follow => core::MinionOrder::Follow,
                MinionOrder::Attack { target } => core::MinionOrder::Attack(target.into()),
                MinionOrder::Defend { point } => core::MinionOrder::Defend(point),
                MinionOrder::Retreat => core::MinionOrder::Retreat,
            }),
        })
    }
}

/// What a departed character's session receives in place of its projection once the source zone
/// retired it (#145): reconnect to `route` and claim the character there; the destination applies
/// only commands after `cutoverSequence`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(large_number_types_as_bigints)]
pub struct RedirectNotice {
    /// The source zone the character left and its tick.
    pub zone_id: u32,
    #[tsify(type = "bigint")]
    pub tick: u64,
    pub destination: u32,
    #[tsify(type = "bigint")]
    pub transfer_id: u64,
    pub cutover_sequence: u32,
    pub route: String,
}

/// The wire facts the browser presents (#541), from the core itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct WireCatalog {
    /// The random 24-bit wire protocol ID (ADR 0006).
    pub protocol_id: u32,
    /// A full breath: 30 s submerged.
    pub breath_max_ticks: u16,
    pub global_cooldown_ticks: u16,
    /// Ticks a broken guard recovers.
    pub guard_break_ticks: u16,
    /// Unit kinds in `EntityRef` order.
    pub entity_kinds: Vec<EntityKind>,
    /// Emotes in wire order.
    pub emotes: Vec<EmoteName>,
}

#[must_use]
pub fn wire_catalog() -> WireCatalog {
    WireCatalog {
        protocol_id: mmorpg_protocol::PROTOCOL_ID,
        breath_max_ticks: core::water::BREATH_MAX_TICKS,
        global_cooldown_ticks: core::GLOBAL_COOLDOWN_TICKS,
        guard_break_ticks: core::GUARD_BREAK_TICKS,
        entity_kinds: EntityKind::ALL.to_vec(),
        emotes: EmoteName::ALL.to_vec(),
    }
}

/// Decodes a player projection with the Rust decoder.
pub fn decode_snapshot(payload: &[u8]) -> Convert<ZoneSnapshot> {
    mmorpg_protocol::decode_snapshot(payload)
        .map(|snapshot| ZoneSnapshot::from(&snapshot))
        .map_err(|error| error.to_string())
}

/// Encodes a projection with the Rust encoder; tests use it to feed the browser real bytes.
pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Convert<Vec<u8>> {
    mmorpg_protocol::encode_snapshot(&core::ZoneSnapshot::try_from(snapshot)?)
        .map_err(|error| error.to_string())
}

pub fn encode_command(command: &WorldCommand) -> Convert<Vec<u8>> {
    Ok(mmorpg_protocol::encode_command(command.try_into()?))
}

pub fn encode_observed_command(command: &WorldCommand, observed: u64) -> Convert<Vec<u8>> {
    Ok(mmorpg_protocol::encode_observed_command(
        command.try_into()?,
        observed,
    ))
}

/// Whether encoded (plain or observed) command bytes are movement intent (Move or Rise).
#[must_use]
pub fn is_movement_command(payload: &[u8]) -> bool {
    matches!(
        mmorpg_protocol::decode_observed_command(payload),
        Ok((
            core::ZoneCommand::Move { .. } | core::ZoneCommand::Rise(_),
            _
        ))
    )
}

pub fn decode_redirect_notice(payload: &[u8]) -> Convert<RedirectNotice> {
    let notice =
        mmorpg_protocol::decode_redirect_notice(payload).map_err(|error| error.to_string())?;
    Ok(RedirectNotice {
        zone_id: notice.zone_id.get(),
        tick: notice.tick,
        destination: notice.destination.get(),
        transfer_id: notice.transfer_id.get(),
        cutover_sequence: notice.cutover_sequence,
        route: notice.route,
    })
}

fn js_error(message: impl AsRef<str>) -> JsError {
    JsError::new(message.as_ref())
}

#[wasm_bindgen(typescript_custom_section)]
const WIRE_TYPES: &str = r#"
/** A position or velocity in units: x, y (up), z. */
export type Vector3 = [number, number, number];
/** A movement axis: back/left, none or forward/right. */
export type Axis = -1 | 0 | 1;
"#;

/// Decodes a player projection with the Rust decoder; throws on any malformed payload.
#[wasm_bindgen(js_name = decodeSnapshot)]
pub fn decode_snapshot_js(payload: &[u8]) -> Result<Ts<ZoneSnapshot>, JsError> {
    Ok(decode_snapshot(payload).map_err(js_error)?.into_ts()?)
}

/// Encodes a projection with the Rust encoder (tests feed the browser real bytes with it).
#[wasm_bindgen(js_name = encodeSnapshot)]
pub fn encode_snapshot_js(snapshot: Ts<ZoneSnapshot>) -> Result<Vec<u8>, JsError> {
    encode_snapshot(&snapshot.to_rust()?).map_err(js_error)
}

#[wasm_bindgen(js_name = encodeCommand)]
pub fn encode_command_js(command: Ts<WorldCommand>) -> Result<Vec<u8>, JsError> {
    encode_command(&command.to_rust()?).map_err(js_error)
}

/// `command` sent after observing the zone's completed tick `observed` (#415).
#[wasm_bindgen(js_name = encodeObservedCommand)]
pub fn encode_observed_command_js(
    command: Ts<WorldCommand>,
    #[wasm_bindgen(unchecked_param_type = "bigint")] observed: JsValue,
) -> Result<Vec<u8>, JsError> {
    // Through serde, so a negative or over-wide tick throws instead of wrapping.
    let observed: u64 = serde_wasm_bindgen::from_value(observed)?;
    encode_observed_command(&command.to_rust()?, observed).map_err(js_error)
}

#[wasm_bindgen(js_name = isMovementCommand)]
#[must_use]
pub fn is_movement_command_js(payload: &[u8]) -> bool {
    is_movement_command(payload)
}

/// Whether a session snapshot payload is a redirect notice rather than a player projection.
#[wasm_bindgen(js_name = isRedirectNotice)]
#[must_use]
pub fn is_redirect_notice_js(payload: &[u8]) -> bool {
    mmorpg_protocol::is_redirect_notice(payload)
}

#[wasm_bindgen(js_name = decodeRedirectNotice)]
pub fn decode_redirect_notice_js(payload: &[u8]) -> Result<Ts<RedirectNotice>, JsError> {
    Ok(decode_redirect_notice(payload)
        .map_err(js_error)?
        .into_ts()?)
}

/// The core chat text rule; `undefined` when `text` is a valid line, otherwise why not.
#[wasm_bindgen(js_name = chatTextError)]
#[must_use]
pub fn chat_text_error(text: &str) -> Option<String> {
    chat_text(text).err()
}

#[wasm_bindgen(js_name = wireCatalog)]
pub fn wire_catalog_js() -> Result<Ts<WireCatalog>, JsError> {
    Ok(wire_catalog().into_ts()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = core::golden::workspace_root()
            .join("fixtures/protocol")
            .join(name);
        let hex = std::fs::read_to_string(path).expect("fixture");
        let hex = hex.trim();
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn golden_projections_survive_the_browser_shape() {
        for name in [
            "player-snapshot.hex",
            "player-loot.hex",
            "presentation-observation.hex",
        ] {
            let bytes = fixture(name);
            let snapshot = decode_snapshot(&bytes).expect(name);
            assert_eq!(encode_snapshot(&snapshot).expect(name), bytes, "{name}");
        }
    }

    #[test]
    fn commands_validate_like_the_browser_did() {
        let encode = |command: WorldCommand| encode_command(&command);
        assert!(
            encode(WorldCommand::Move {
                forward: 2,
                strafe: 0,
                facing: 0
            })
            .is_err()
        );
        assert!(encode(WorldCommand::Rise { rise: -2 }).is_err());
        assert!(encode(WorldCommand::UseWaystone { waystone: 0 }).is_err());
        assert!(
            encode(WorldCommand::Repair {
                piece: Some(u16::MAX)
            })
            .is_err()
        );
        assert!(
            encode(WorldCommand::Chat {
                channel: ChatChannel::Say,
                text: " ".to_owned()
            })
            .is_err()
        );
        let movement = encode(WorldCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 16_384,
        })
        .expect("move");
        assert!(is_movement_command(&movement));
        assert!(!is_movement_command(
            &encode(WorldCommand::Jump).expect("jump")
        ));
    }
}
