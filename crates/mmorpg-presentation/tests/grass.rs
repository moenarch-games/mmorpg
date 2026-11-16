//! The grass fields' cover: thick on Barrowdown's green, never inside a house or on a road.
use mmorpg_core::greyhaven_vale::settlements::{self, Building};
use mmorpg_presentation::{
    StaticScene,
    grass::Grass,
    grass::{CELL_METRES, CLUMP_FLOATS},
};
use mmorpg_scenery::greyhaven_vale_scenery;

#[test]
fn grass_grows_round_barrowdown_but_not_in_its_houses_or_on_its_lane() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let grass = Grass::new(&scene, &scenery);
    let cell = |x: f64, z: f64| {
        let (column, row) = (
            (x / CELL_METRES).floor() as i32,
            (z / CELL_METRES).floor() as i32,
        );
        grass.cell(column, row)
    };
    let houses: Vec<_> = scenery
        .props
        .iter()
        .filter(|prop| {
            prop.collider
                .is_some_and(|id| settlements::building(id) == Some(Building::House))
        })
        .map(|prop| prop.footprint())
        .collect();
    let mut clumps = 0;
    for (x, z) in [(516., 636.), (520., 668.), (566., 636.), (548., 690.)] {
        let field = cell(x, z);
        assert_eq!(field.len() % CLUMP_FLOATS, 0);
        clumps += field.len() / CLUMP_FLOATS;
        for clump in field.chunks(CLUMP_FLOATS) {
            let at = [
                (clump[0] * 100.).round() as i32,
                (clump[2] * 100.).round() as i32,
            ];
            assert!(
                houses.iter().all(|house| house.distance(at) > 0),
                "a clump at {at:?} grows in a house"
            );
            assert!(
                scenery.road_distance(at) > 0,
                "a clump at {at:?} grows on a road"
            );
            let ground = scene.surface_y(f64::from(clump[0]), f64::from(clump[2]));
            assert!(
                (f64::from(clump[1]) - ground).abs() < 0.01,
                "{clump:?} floats"
            );
            assert!((0.1..1.).contains(&clump[4]), "{clump:?} height");
        }
    }
    assert!(clumps > 500, "only {clumps} clumps round Barrowdown");
}

#[test]
fn far_cells_lay_a_sparse_field_over_the_same_ground() {
    use mmorpg_presentation::grass::FAR_CELL_METRES;
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let grass = Grass::new(&scene, &scenery);
    let (column, row) = (
        (530. / FAR_CELL_METRES).floor() as i32,
        (660. / FAR_CELL_METRES).floor() as i32,
    );
    let far = grass.far_cell(column, row);
    assert_eq!(far.len() % CLUMP_FLOATS, 0);
    let clumps = far.len() / CLUMP_FLOATS;
    // About a clump a square metre where grass grows: far sparser than the near field's.
    assert!((100..1024).contains(&clumps), "{clumps} far clumps");
    for clump in far.chunks(CLUMP_FLOATS) {
        let (x, z) = (f64::from(clump[0]), f64::from(clump[2]));
        assert!(
            (f64::from(column) * FAR_CELL_METRES..f64::from(column + 1) * FAR_CELL_METRES)
                .contains(&x)
                && (f64::from(row) * FAR_CELL_METRES..f64::from(row + 1) * FAR_CELL_METRES)
                    .contains(&z),
            "{clump:?} outside its cell"
        );
        let ground = scene.surface_y(x, z);
        assert!(
            (f64::from(clump[1]) - ground).abs() < 0.01,
            "{clump:?} floats"
        );
    }
}
