//! Resource lifetime follows actual submitted dynamic effects, not a numeric cache budget.
use mmorpg_client::{camera::OrbitCamera, presentation::Presentation, world::WorldScene};
use mmorpg_core::{AbilityId, EntityRef, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[test]
fn animated_effect_meshes_retire_after_last_draw_and_session_reset() {
    let content = greyhaven_vale::content();
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut snapshot = zone.snapshot_for_player(1).unwrap();
    snapshot.events.push(ZoneEvent::AbilityUsed {
        source: EntityRef::Player(1),
        target: None,
        ability: AbilityId::new(11),
    });
    snapshot.auras.push(mmorpg_core::AuraView {
        ability: AbilityId::new(11),
        kind: mmorpg_core::AuraKind::Absorb,
        remaining: 60,
        amount: 20,
    });
    let mut expired = snapshot.clone();
    expired.tick += 1;
    expired.events.clear();
    expired.auras.clear();
    let now = Instant::now();
    let mut p = Presentation::new(1, scenery, content, now).unwrap();
    p.push(snapshot, now).unwrap();
    let view = p.camera_view(&mut OrbitCamera::default(), &world, now);
    let first = p.unit_draws(now + Duration::from_millis(100), view);
    let bubble = first
        .iter()
        .find(|d| d.id.ends_with("-bubble"))
        .expect("accepted Barrier must produce actual bubble mesh");
    let old = Arc::downgrade(&bubble.mesh);
    drop(first);
    let next = p.unit_draws(now + Duration::from_millis(200), view);
    assert!(
        old.upgrade().is_none(),
        "old animated radius geometry retained after no draw owns it"
    );
    let bubble = next.iter().find(|d| d.id.ends_with("-bubble")).unwrap();
    let active = Arc::downgrade(&bubble.mesh);
    drop(next);
    p.push(expired, now + Duration::from_secs(2)).unwrap();
    drop(p.unit_draws(now + Duration::from_secs(2), view));
    assert!(
        active.upgrade().is_none(),
        "expired bubble geometry remains cached"
    );
    let body = p.unit_draws(now + Duration::from_secs(60), view);
    let retained = Arc::downgrade(&body[0].mesh);
    drop(body);
    p.reset(now + Duration::from_secs(61));
    assert!(
        retained.upgrade().is_none(),
        "session reset retained previous mesh resources"
    );
}
