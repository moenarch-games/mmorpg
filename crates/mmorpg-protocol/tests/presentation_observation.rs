//! #27 observer cues cross the same strict Rust/native/browser snapshot seam.
use mmorpg_core::{CreatureId, EntityRef, ZoneEvent, ZoneId, ZoneSimulation};
use mmorpg_protocol::{
    MAX_PLAYER_PROJECTION_BYTES, SNAPSHOT_WIRE_VERSION, decode_canonical_snapshot, decode_snapshot,
    encode_canonical_snapshot, encode_snapshot, pack_snapshot,
};
#[test]
fn observation_records_round_trip_without_reinterpreting_previous_snapshot_version() {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    let mut view = zone.snapshot_for_player(1).unwrap();
    let source = EntityRef::Player(2);
    let target = EntityRef::Creature(CreatureId::new(101));
    view.events = vec![
        ZoneEvent::AttackPerformed { source, target },
        ZoneEvent::DamageObserved {
            source,
            target,
            amount: 7,
            critical: true,
        },
    ];
    let bytes = encode_snapshot(&view).unwrap();
    let golden = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/protocol/presentation-observation-v15.hex"
    ))
    .unwrap();
    let expected: Vec<u8> = golden
        .trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(bytes, expected);
    assert_eq!(decode_snapshot(&bytes).unwrap(), view);
    assert_eq!(
        SNAPSHOT_WIRE_VERSION, 15,
        "one intentional migration from baseline14"
    );
    // Use the current encoder for the new schema; old version is explicitly rejected.
    let old = include_str!("../../../fixtures/protocol/player-snapshot-v14.hex");
    let old: Vec<u8> = old
        .trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    assert!(decode_snapshot(&old).is_err());
    let packed = pack_snapshot(&view).unwrap();
    assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
    assert_eq!(
        decode_snapshot(&packed.payload).unwrap().events,
        view.events
    );
    let state = zone.snapshot().unwrap();
    assert_eq!(
        decode_canonical_snapshot(&encode_canonical_snapshot(&state).unwrap()).unwrap(),
        state
    );
}
#[test]
fn full_cosmetic_section_keeps_critical_feedback_and_required_entities_under_wire_budget() {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    let mut view = zone.snapshot_for_player(1).unwrap();
    let source = EntityRef::Player(2);
    let target = EntityRef::Creature(CreatureId::new(101));
    let critical = ZoneEvent::Died {
        entity: EntityRef::Player(1),
        killer: Some(target),
    };
    view.events =
        vec![ZoneEvent::AttackPerformed { source, target }; mmorpg_core::MAX_EVENTS_PER_PLAYER - 1];
    view.events.push(critical);
    view.viewer.target = Some(target);
    let prototype = view.entities[0].clone();
    let mut target_record = prototype.clone();
    target_record.kind = mmorpg_core::EntityKind::Creature;
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
}
