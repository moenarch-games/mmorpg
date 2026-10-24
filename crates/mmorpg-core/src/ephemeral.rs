//! Temporary maximum-level builds, admitted by a mode host rather than player commands.
use crate::{
    CharacterPersistence, CharacterRecord, ClassChoice, Equipment, Inventory, InventoryError,
    ItemId, MAX_PLAYER_LEVEL, PlayerClass, PlayerId, ZoneError, ZoneSimulation,
};

/// Validated appearance and equipped catalog items. No durable identity or progression is carried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EphemeralCharacterBuild {
    class: ClassChoice,
    equipment: Equipment,
}
impl EphemeralCharacterBuild {
    pub fn new(
        class: ClassChoice,
        slots: [Option<ItemId>; crate::EQUIPMENT_SLOTS],
    ) -> Result<Self, InventoryError> {
        Ok(Self {
            class,
            equipment: Equipment::from_slots(slots)?,
        })
    }
    /// Existing catalog choices: class-primary weapon, then the catalog's available armor.
    /// There are currently no item-level or class equipment restrictions in this catalog.
    pub fn standard(class: ClassChoice) -> Result<Self, InventoryError> {
        let weapon = match class.class {
            PlayerClass::Warden | PlayerClass::Rogue => 3,
            PlayerClass::Ranger => 2,
            PlayerClass::Arcanist | PlayerClass::Necromancer | PlayerClass::Cleric => 4,
        };
        Self::new(
            class,
            [weapon, 5, 6, 7, 8, 9].map(|id| Some(ItemId::new(id))),
        )
    }
    /// Reuse only the existing appearance/equipped selection; never copy progress or identity.
    #[must_use]
    pub fn from_character(record: &CharacterRecord) -> Self {
        Self {
            class: record.class,
            equipment: record.equipment,
        }
    }
    #[must_use]
    pub const fn class(&self) -> ClassChoice {
        self.class
    }
    #[must_use]
    pub const fn equipment(&self) -> Equipment {
        self.equipment
    }
}

impl ZoneSimulation {
    /// Admits a fresh, maximum-level temporary player. The mode decides when a new life begins;
    /// this cannot edit an existing player or a durable character. Host/session sequencing applies.
    pub fn admit_ephemeral(
        &mut self,
        player: PlayerId,
        build: &EphemeralCharacterBuild,
    ) -> Result<(), ZoneError> {
        if self.content.character_persistence() != CharacterPersistence::Ephemeral {
            return Err(ZoneError::new("temporary builds require an ephemeral zone"));
        }
        if self.content.ability_revision() == 0 {
            return Err(ZoneError::new("the zone content offers no classes"));
        }
        self.add_player(player)?;
        self.update_player(player, |state| {
            state.class = Some(build.class);
            state.level = MAX_PLAYER_LEVEL;
            state.inventory = Inventory::default();
            state.equipment = build.equipment;
            state.health = state.max_health();
            state.resource.value = build.class.class.resource().start(MAX_PLAYER_LEVEL);
        });
        Ok(())
    }
}
