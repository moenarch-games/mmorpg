//! Explicit authored stage fixtures rendered by the actual native world renderer (#306).
use super::*;
use mmorpg_core::{PieceStage, PieceView};

#[test]
#[ignore = "requires GPU; focused structure presentation evidence"]
fn replicated_structure_stages_change_native_pixels() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let world = WorldScene::new(&scenery);
        let pieces = mmorpg_core::greyhaven_vale::watch_post();
        let lift = scenery.height_at(700, -7200) as f32 / 100.0;
        let view = CameraView {
            field_of_view_degrees: 50.0,
            eye: [14.0, lift + 6.0, -61.0],
            target: [7.0, lift + 1.4, -72.0],
        };
        let mut presenter =
            mmorpg_presentation::structures::Structures::on_scenery(&pieces, &scenery);
        let mut lowering = crate::world::Lowering::default();
        let mut previous = None;
        for (index, (name, stage)) in [
            ("intact", PieceStage::Intact),
            ("damaged", PieceStage::Damaged),
            ("crumbling", PieceStage::Crumbling),
            ("destroyed", PieceStage::Destroyed),
            ("restored", PieceStage::Intact),
        ]
        .into_iter()
        .enumerate()
        {
            let views: Vec<_> = pieces
                .iter()
                .map(|piece| PieceView {
                    piece: piece.id,
                    stage,
                })
                .collect();
            let shared = presenter.frame(&views, index as f64 * 3.0, None, false);
            let mut frame = world.frame(0.0, false, view.eye, None);
            frame
                .draws
                .extend(shared.iter().map(|draw| lowering.draw(draw)));
            let pixels = offscreen_frame_pixels(&frame, &[], view).await.unwrap();
            super::tests::save_smoke_frame(&format!("structure-{name}.ppm"), &pixels);
            if let Some(previous) = previous {
                assert_ne!(
                    pixels, previous,
                    "a stage transition changes actual GPU pixels"
                );
            }
            previous = Some(pixels);
        }
    });
}
