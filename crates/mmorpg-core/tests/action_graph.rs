//! The authoritative parallel action graph (#380, PR 1): the Warden's
//! jump → directed slash → landing → Shield Bash/Guard path runs through
//! authored regions, edges and rules, and the read-only inspection reports
//! the exact ticks at which the zone actually resolves each input.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionId, ActionInput, BlockReason, Candidate, Condition, EdgeId, EdgeStatus, Outcome,
    Phase, Region, RuleId, Verdict,
};
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureBehaviour, CreatureTemplate, StrikeAim, StrikeHeight,
    StrikeSide, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

const SOUTH: u16 = 32_768;

fn content() -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::wolf(5_000, [1, 1])
    };
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![arena::spawn(1, WOLF, [0, -150])],
            Vec::new(),
            GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(ABILITY_CATALOG_REVISION, Default::default())
        .unwrap(),
    )
}

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    /// A level-2 Warden (Shield Bash learned) with full Rage.
    fn new() -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        for _ in 0..mmorpg_core::golden::t30(20) {
            warden.tick();
        }
        let mut state = warden.zone.snapshot().unwrap();
        let player = &mut state.players[0].combat;
        player.level = 2;
        player.health = mmorpg_core::unit::player_max_health(2);
        player.abilities.resource = 100;
        warden.zone = ZoneSimulation::from_snapshot(state, content).unwrap();
        warden
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        self.zone.snapshot_for_player(1).unwrap().events
    }

    fn inspect(&self) -> action::ActionInspection {
        self.zone.action_inspection(1).unwrap()
    }

    fn region(&self, region: Region) -> action::RegionReport {
        self.inspect().regions[region as usize]
    }

    fn slash(&mut self) {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        });
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: None,
        });
    }

    /// Ticks until the Shield Bash resolves (used or refused); its tick.
    fn bash_resolution(&mut self) -> u64 {
        for _ in 0..mmorpg_core::golden::t30(60) {
            let events = self.tick();
            if events.iter().any(|event| match event {
                ZoneEvent::AbilityUsed { ability, .. } => *ability == ids::SHIELD_BASH,
                ZoneEvent::Error { .. } => true,
                _ => false,
            }) {
                return self.inspect().tick;
            }
        }
        panic!("the Bash resolves");
    }
}

#[test]
fn the_baseline_graph_validates_and_keeps_the_current_values() {
    action::validate().unwrap();
    // The authored windows in ms, at 60 Hz (#379): 16⅔ ms per tick.
    assert_eq!(mmorpg_core::COMBO_IMPACT_TICKS, 12);
    assert_eq!(mmorpg_core::COMBO_WINDOW_TICKS, 36);
    assert_eq!(mmorpg_core::STRIKE_RECOVERY_TICKS, 12);
    assert_eq!(mmorpg_core::LANDING_RECOVERY_TICKS, 18);
    assert_eq!(mmorpg_core::OVERHEAD_LANDING_TICKS, 30);
    assert_eq!(mmorpg_core::GUARD_BREAK_TICKS, 90);
    assert_eq!(mmorpg_core::ticks_from_ms(200), 12);
    assert_eq!(mmorpg_core::ticks_from_ms(50), 3);
}

#[test]
fn a_grounded_slash_buffers_bash_and_guard_until_the_inspected_ticks() {
    const IMPACT: u64 = mmorpg_core::COMBO_IMPACT_TICKS as u64;
    const WINDOW: u64 = mmorpg_core::COMBO_WINDOW_TICKS as u64;
    let mut warden = Warden::new();
    warden.slash();
    warden.tick();
    let strike = warden.inspect().tick;
    let inspection = warden.inspect();
    assert_eq!(
        inspection.regions[Region::Offense as usize].action,
        Some(ActionId::DirectedSlash)
    );
    assert_eq!(
        inspection.regions[Region::Offense as usize].phase,
        Some(Phase::Recovery)
    );
    let edge = inspection.edges[0];
    assert_eq!(edge.edge, EdgeId::SlashToShieldBash);
    assert_eq!(edge.status, EdgeStatus::Buffering);
    assert_eq!(edge.anchor_tick, Some(strike));
    assert_eq!(edge.open_from, Some(strike + IMPACT));
    assert_eq!(edge.open_until, Some(strike + IMPACT + WINDOW));
    let guard_hold = inspection
        .rules
        .iter()
        .find(|rule| rule.rule == RuleId::SlashRecoveryHoldsGuard)
        .unwrap();
    assert_eq!(guard_hold.released_at, Some(strike + IMPACT));

    // The combo edge outranks the Bash's direct use and holds it.
    let arbitration = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(
        arbitration.outcome,
        Outcome::WaitForEdge(EdgeId::SlashToShieldBash)
    );
    assert_eq!(
        arbitration.candidates,
        [
            (Candidate::Edge(EdgeId::SlashToShieldBash), Verdict::Holding),
            (
                Candidate::Direct,
                Verdict::Outranked(Candidate::Edge(EdgeId::SlashToShieldBash))
            ),
        ]
    );

    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::Middle),
    });
    warden.bash();
    // The guard protects from exactly the released tick.
    for _ in 0..mmorpg_core::golden::t30(4) {
        warden.tick();
        assert!(warden.inspect().tick < strike + IMPACT);
        assert!(
            warden
                .zone
                .arbitrate_input(1, ActionInput::GuardProtection)
                .is_some_and(|arbitration| arbitration.outcome
                    == Outcome::WaitForRule(RuleId::SlashRecoveryHoldsGuard))
        );
    }
    assert_eq!(warden.bash_resolution(), strike + IMPACT);
    assert_eq!(
        warden.inspect().edges[0].status,
        EdgeStatus::Blocked(BlockReason::SourceInactive)
    );
}

#[test]
fn a_whiff_reports_the_unmet_damage_condition_and_the_bash_is_direct() {
    let mut warden = Warden::new();
    warden.send(ZoneCommand::DirectedStrike {
        ability: ids::HEROIC_STRIKE.get(),
        facing: SOUTH,
        aim: StrikeAim::new(StrikeSide::Left, StrikeHeight::Middle),
    });
    warden.tick();
    assert_eq!(
        warden.inspect().edges[0].status,
        EdgeStatus::Blocked(BlockReason::Condition(Condition::PositiveHealthDamage))
    );
    let arbitration = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(arbitration.outcome, Outcome::Proceed(Candidate::Direct));
}

#[test]
fn an_airborne_slash_lands_into_a_held_bash_at_the_inspected_tick() {
    let mut warden = Warden::new();
    warden.send(ZoneCommand::Jump);
    for _ in 0..mmorpg_core::golden::t30(3) {
        warden.tick();
    }
    assert_eq!(
        warden.region(Region::Movement).action,
        Some(ActionId::Airborne)
    );
    warden.slash();
    let hit = warden
        .tick()
        .iter()
        .any(|event| matches!(event, ZoneEvent::DamageDealt { .. }));
    assert_eq!(
        warden.region(Region::Offense).action,
        Some(ActionId::DirectedSlash)
    );
    for _ in 0..mmorpg_core::golden::t30(120) {
        if warden.region(Region::Movement).action == Some(ActionId::Landing) {
            break;
        }
        warden.tick();
    }
    let movement = warden.region(Region::Movement);
    assert_eq!(movement.phase, Some(Phase::Recovery));
    // An airborne melee hit lengthens the landing (5.2.70).
    assert_eq!(
        movement.recovery_left,
        Some(if hit {
            mmorpg_core::OVERHEAD_LANDING_TICKS
        } else {
            mmorpg_core::LANDING_RECOVERY_TICKS
        })
    );
    let released = warden
        .inspect()
        .rules
        .iter()
        .find(|rule| rule.rule == RuleId::LandingHoldsMelee)
        .and_then(|rule| rule.released_at)
        .unwrap();
    warden.bash();
    let arbitration = warden
        .zone
        .arbitrate_input(1, ActionInput::ShieldBash)
        .unwrap();
    assert_eq!(
        arbitration.outcome,
        Outcome::WaitForRule(RuleId::LandingHoldsMelee)
    );
    assert!(
        arbitration
            .candidates
            .iter()
            .all(|(_, verdict)| *verdict == Verdict::HeldBy(RuleId::LandingHoldsMelee))
    );
    assert_eq!(warden.bash_resolution(), released);
}
