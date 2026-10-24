//! Destructible structures (#305): a blast breaks walls and the deck resting on them falls,
//! a broken wall leaves its rubble blocker, a shot chips a piece, pieces regrow on their
//! site's timer, a player's free repair speeds that up, and a restored zone continues exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, PieceId, PieceStage, PieceView, StructurePiece, StructureSite,
    ZoneCommand, ZoneContent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};

const ARCANIST: u8 = 2;
const RANGER: u8 = 1;
/// Spawn slots lie along +X and +Z, so the post stands toward −Z.
const SOUTH: u16 = 32_768;

/// The yaw facing `(x, z)` from the origin.
fn yaw(x: f64, z: f64) -> u16 {
    let turn = x.atan2(z) / std::f64::consts::TAU * 65_536.0;
    u16::try_from((turn.round() as i64).rem_euclid(65_536)).unwrap()
}

/// Flat ground, a sturdy neutral boar 14.5 m south between two weak walls and a deck on both.
fn content(regrowth_ticks: u32) -> Arc<ZoneContent> {
    let wall = |id: u16, x: i32, rubble| StructurePiece {
        id: PieceId::new(id),
        site: 1,
        body: 50 + u32::from(id),
        position: [x, 125, -1_450],
        half_extents: [20, 125, 150],
        health: 10,
        supports: Vec::new(),
        rubble,
    };
    let pieces = vec![
        wall(1, -300, Some(([-300, 20, -1_450], [40, 20, 150]))),
        wall(2, 300, None),
        StructurePiece {
            id: PieceId::new(3),
            site: 1,
            body: 53,
            position: [0, 265, -1_450],
            half_extents: [320, 15, 160],
            health: 10,
            supports: vec![PieceId::new(1), PieceId::new(2)],
            rubble: None,
        },
    ];
    let content = arena::arena(
        vec![mmorpg_core::CreatureTemplate {
            health: 400,
            ..arena::boar()
        }],
        vec![arena::spawn(1, BOAR, [0, -1_450])],
        Vec::new(),
    );
    Arc::new(
        Arc::unwrap_or_clone(content)
            .with_creature_abilities(Vec::new())
            .unwrap()
            .with_structures(
                vec![StructureSite {
                    id: 1,
                    name: "Test Post".into(),
                    regrowth_ticks,
                }],
                pieces,
            )
            .unwrap(),
    )
}

struct Player {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Player {
    fn new(content: Arc<ZoneContent>, class: u8) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 6);
        let mut player = Self { zone, sequence: 0 };
        player.send(ZoneCommand::ChooseClass { class, sex: 0 });
        player.send(arena::stand(SOUTH));
        player.ticks(1);
        let mut state = player.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        player.zone =
            ZoneSimulation::from_snapshot(state, Arc::clone(player.zone.content())).unwrap();
        player
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    fn pieces(&self) -> Vec<PieceView> {
        self.zone.snapshot_for_player(1).unwrap().pieces
    }

    fn health(&self, piece: u16) -> u32 {
        self.zone
            .snapshot()
            .unwrap()
            .pieces
            .iter()
            .find(|record| record.piece == PieceId::new(piece))
            .unwrap()
            .health
    }

    fn blast_the_boar(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::FIRE_BLAST.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        });
        self.ticks(50);
    }
}

fn view(piece: u16, stage: PieceStage) -> PieceView {
    PieceView {
        piece: PieceId::new(piece),
        stage,
    }
}

#[test]
fn a_blast_breaks_the_walls_the_deck_falls_and_rubble_stays() {
    let mut arcanist = Player::new(content(9_000), ARCANIST);
    assert!(arcanist.pieces().is_empty(), "every piece starts intact");
    arcanist.blast_the_boar();
    assert_eq!(
        arcanist.pieces(),
        [
            view(1, PieceStage::Destroyed),
            view(2, PieceStage::Destroyed),
            view(3, PieceStage::Destroyed)
        ],
        "both walls broke and the deck resting on them fell"
    );
    // Restored from its canonical snapshot, the broken post stays broken.
    let restored = ZoneSimulation::from_snapshot(
        arcanist.zone.snapshot().unwrap(),
        Arc::clone(arcanist.zone.content()),
    )
    .unwrap();
    assert_eq!(
        restored.snapshot().unwrap(),
        arcanist.zone.snapshot().unwrap()
    );
}

#[test]
fn a_shot_chips_a_piece() {
    let mut ranger = Player::new(content(9_000), RANGER);
    // Aimed Shot without a target flies along the facing; turn toward the east wall's face.
    let yaw = |x: f64, z: f64| {
        ((x.atan2(z) / std::f64::consts::TAU * 65_536.0).round() as i64).rem_euclid(65_536) as u16
    };
    ranger.send(arena::stand(yaw(280.0, -1_450.0)));
    ranger.send(ZoneCommand::UseAbility {
        ability: ids::AIMED_SHOT.get(),
        target: None,
    });
    ranger.ticks(40);
    assert_eq!(ranger.health(2), 7, "the arrow chipped the east wall");
    // Seven of ten is still above two thirds: intact, so not projected.
    assert!(ranger.pieces().is_empty());
    assert_eq!(PieceStage::of(6, 10), PieceStage::Damaged);
}

#[test]
fn pieces_regrow_on_their_timer_and_repair_speeds_that_up() {
    let regrow = |repair: bool| {
        let mut arcanist = Player::new(content(300), ARCANIST);
        arcanist.blast_the_boar();
        assert_eq!(arcanist.health(2), 0);
        if repair {
            // Walk up to the east wall and repair it.
            arcanist.send(arena::walk(yaw(471.0, -1_168.0)));
            arcanist.ticks(60);
            arcanist.send(arena::stand(SOUTH));
            arcanist.ticks(1);
            arcanist.send(ZoneCommand::Repair {
                piece: Some(PieceId::new(2)),
            });
        }
        let mut ticks = 0;
        while arcanist.health(2) == 0 {
            arcanist.ticks(1);
            ticks += 1;
            assert!(ticks < 400, "the wall regrew");
        }
        // The deck regrows once a wall stands again.
        arcanist.ticks(2);
        (ticks, arcanist.health(3))
    };
    let (alone, deck) = regrow(false);
    assert_eq!(deck, 10, "the deck regrew on its wall");
    let (repaired, _) = regrow(true);
    assert!(
        repaired * 2 < alone,
        "repairing regrows it much sooner: {repaired} vs {alone}"
    );
}
