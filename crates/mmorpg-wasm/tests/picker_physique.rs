//! The picker's physique path (#634): a never-stored character is created with the confirmed
//! sliders at its first entry, and a stored one is edited outside any zone under its lease.

use std::collections::BTreeMap;

use mmorpg_characters::{CharacterStore, EntryBackend, EntryChange, LeaseHolder, StoreError};
use mmorpg_core::{CharacterId, ClassChoice, Physique, PhysiquePreset, PlayerClass, Sex};
use mmorpg_wasm::host::{LOCAL_ZONE_ID, LocalZoneHost, customize_stored_physique};

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl EntryBackend for Memory {
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.get(id.as_str()).cloned())
    }

    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError> {
        let next = change(self.0.get(id.as_str()).map(Vec::as_slice))?;
        self.0.insert(id.as_str().to_owned(), next);
        Ok(())
    }
}

fn holder(zone_epoch: u64) -> LeaseHolder {
    LeaseHolder {
        zone_id: LOCAL_ZONE_ID,
        zone_epoch,
    }
}

fn committed(store: &CharacterStore<Memory>, id: &CharacterId) -> Physique {
    store.committed(id).unwrap().unwrap().record.physique
}

#[test]
fn picker_physique_creates_edits_and_reloads() {
    let mut store = CharacterStore::new(Memory::default());
    let id = CharacterId::new("local-1").unwrap();
    let lean = PhysiquePreset::Lean.physique();

    // Never stored: the picker's save changes nothing; the first entry creates it with the sliders.
    assert!(!customize_stored_physique(&mut store, &id, lean, holder(1)).unwrap());
    assert!(store.committed(&id).unwrap().is_none());
    let mut host = LocalZoneHost::new().unwrap();
    let class = ClassChoice {
        class: PlayerClass::Warden,
        sex: Sex::Male,
    };
    let player = host
        .enter_with_physique(&mut store, id.clone(), class, lean, holder(2))
        .unwrap();
    assert_eq!(committed(&store, &id), lean);
    host.exit(&mut store, player).unwrap();

    // Stored: edited at the picker, then entered again with its own values.
    let muscular = PhysiquePreset::Muscular.physique();
    assert!(customize_stored_physique(&mut store, &id, muscular, holder(3)).unwrap());
    assert_eq!(committed(&store, &id), muscular);
    let player = host
        .enter_with_physique(&mut store, id.clone(), class, Physique::NEUTRAL, holder(4))
        .unwrap();
    host.exit(&mut store, player).unwrap();
    assert_eq!(committed(&store, &id), muscular);
}
