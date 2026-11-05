//! Greyhaven's inhabited brook crossing on authoritative Rapier terrain.
use mmorpg_core::{WaterMode, ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};
use std::sync::Arc;
struct Walker {
    zone: ZoneSimulation,
    sequence: u32,
}
impl Walker {
    fn at(x: i32, z: i32) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        let y = content.definition().ground_at(x, z) + 95;
        state.players[0].position = mmorpg_core::physics::from_units([x, y, z]);
        let zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        let mut w = Self { zone, sequence: 0 };
        w.ticks(20);
        w
    }
    fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            self.zone.advance_tick().unwrap();
        }
    }
    fn walk(&mut self, facing: u16) {
        self.sequence += 1;
        self.zone
            .apply_command(
                1,
                self.sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing,
                },
            )
            .unwrap();
    }
    fn go(&mut self, x: f32, z: f32) {
        for _ in 0..900 {
            let p = self.feet();
            let dx = x - p[0];
            let dz = z - p[2];
            if dx.hypot(dz) < 30. {
                return;
            }
            let heading = (dx.atan2(dz) / std::f32::consts::TAU * 65536.).round() as i32;
            self.walk(heading.rem_euclid(65536) as u16);
            self.ticks(1);
        }
        panic!("route to {x},{z} stopped at {:?}", self.feet());
    }
    fn feet(&self) -> [f32; 3] {
        let [x, y, z] = self.zone.snapshot().unwrap().players[0].position;
        [x, y - 90., z]
    }
}
#[test]
fn brook_bridge_ford_gate_culvert_and_recovery() {
    let content = greyhaven_vale::content();
    assert_eq!(content.npcs().len(), 21);
    for id in 60000..=60002 {
        assert!(content.definition().colliders().iter().any(|c| c.id == id));
    }
    let mut bridge = Walker::at(0, 2700);
    bridge.walk(0);
    let mut highest = f32::MIN;
    for _ in 0..100 {
        bridge.ticks(1);
        highest = highest.max(bridge.feet()[1]);
    }
    assert!(
        bridge.feet()[2] > 3700.,
        "bridge route: {:?}",
        bridge.feet()
    );
    assert!(highest >= 11., "walked on the deck: {highest}");
    let mut ford = Walker::at(300, 3850);
    assert_eq!(
        WaterMode::of_feet(
            ford.zone.content().waters(),
            ford.feet().map(|v| v.round() as i32)
        ),
        WaterMode::Wading
    );
    ford.walk(16384);
    ford.ticks(60);
    assert!(
        ford.feet()[0] > 700.,
        "leave the shallow brook: {:?}",
        ford.feet()
    );
    let mut gate = Walker::at(-3500, 2000);
    gate.walk(16384);
    gate.ticks(90);
    assert!(
        gate.feet()[0] > -3000.,
        "west gate stays accessible: {:?}",
        gate.feet()
    );
    // The new lintels and faceted wall geometry leave every existing road gate usable.
    for (start, target) in [
        ([0, -1400], [0., -600.]),
        ([0, 5500], [0., 4700.]),
        ([3500, 2000], [2900., 2000.]),
    ] {
        let mut walker = Walker::at(start[0], start[1]);
        walker.go(target[0], target[1]);
    }
    let mut wall = Walker::at(-2900, 3100);
    wall.walk(49152);
    wall.ticks(90);
    assert!(
        wall.feet()[0] > -3400.,
        "the faceted timber wall is solid: {:?}",
        wall.feet()
    );
    assert_eq!(
        greyhaven_vale::fort::meshes(),
        greyhaven_vale::fort::meshes()
    );
    // The established west-gate switchback remains a real route to the wooded rim.
    let mut climb = Walker::at(-3500, 2000);
    for (x, z) in [
        (-4200., 2000.),
        (-4200., -1800.),
        (-5400., -1800.),
        (-5400., 1200.),
    ] {
        climb.go(x, z);
    }
    assert!(
        (climb.feet()[1] - 1100.).abs() < 6.,
        "wooded rim: {:?}",
        climb.feet()
    );
    for (x, z) in [
        (-5400., -1800.),
        (-4200., -1800.),
        (-4200., 2000.),
        (-3500., 2000.),
    ] {
        climb.go(x, z);
    }
    let mut alley = Walker::at(-2100, 900);
    for (x, z) in [(-1800., 900.), (-1800., 2000.), (-2200., 3100.)] {
        alley.go(x, z);
    }
    let mut drain = Walker::at(-500, 4700);
    drain.walk(0);
    drain.ticks(90);
    assert!(
        drain.feet()[2] < 4970.,
        "the low culvert is not a person-sized gate: {:?}",
        drain.feet()
    );
    let state = bridge.zone.snapshot().unwrap();
    let mut restored =
        ZoneSimulation::from_snapshot(state, Arc::clone(bridge.zone.content())).unwrap();
    for _ in 0..20 {
        bridge.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(
        bridge.zone.snapshot().unwrap(),
        restored.snapshot().unwrap()
    );
}
