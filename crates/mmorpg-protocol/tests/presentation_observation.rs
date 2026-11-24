//! #27 observer cues cross the same strict Rust/native/browser snapshot seam. The shared
//! observation fixture itself is recorded by the crate's projection tests (#1066).
use mmorpg_core::{CreatureId, EntityRef, ZoneEvent, ZoneId, ZoneSimulation};
use mmorpg_protocol::{MAX_PLAYER_PROJECTION_BYTES, decode_snapshot, pack_snapshot};
#[test]
fn full_cosmetic_section_keeps_critical_feedback_and_required_entities_under_wire_budget() {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    let mut view = zone.snapshot_for_player(1).unwrap();
    let source = EntityRef::Player(mmorpg_core::MAX_VISIBLE_ENTITIES as u32);
    let target = EntityRef::Creature(CreatureId::new(101));
    let critical = ZoneEvent::Died {
        entity: EntityRef::Player(1),
        killer: Some(target),
    };
    view.events = vec![
        ZoneEvent::AttackPerformed {
            source,
            target: Some(target),
        };
        mmorpg_core::MAX_EVENTS_PER_PLAYER - 1
    ];
    view.events.push(critical);
    view.viewer.target = Some(target);
    let prototype = view.entities[0].clone();
    let mut target_record = prototype.clone();
    target_record.kind = mmorpg_core::EntityKind::Creature;
    target_record.flags.weapon = None;
    target_record.id = 101;
    view.entities.push(target_record.clone());
    for id in 3..=mmorpg_core::MAX_VISIBLE_ENTITIES as u32 {
        let mut entity = prototype.clone();
        entity.id = id;
        view.entities.push(entity);
    }
    let packed = pack_snapshot(&view).unwrap();
    assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
    let decoded = decode_snapshot(&packed.payload).unwrap();
    assert!(decoded.events.contains(&critical));
    assert_eq!(decoded.entities[0], view.entities[0]);
    assert_eq!(decoded.entities[1], target_record);
    assert!(
        !decoded
            .entities
            .iter()
            .any(|entity| entity.entity() == source)
    );
    assert!(
        !decoded.events.iter().any(|event| matches!(
            event,
            ZoneEvent::AttackPerformed { .. } | ZoneEvent::DamageObserved { .. }
        )),
        "nonparticipant observations must not name actors omitted by packing"
    );
}
