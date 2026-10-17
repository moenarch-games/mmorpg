//! Sword → Shield Bash combo (#157, Atlas 5.2.39–5.2.51): a directed Heroic
//! Strike that damages a creature lets an early Shield Bash wait out the
//! impact, then bash exactly that creature despite another selection and
//! despite the slash's global cooldown; misses and late requests do not.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, COMBO_IMPACT_TICKS, COMBO_WINDOW_TICKS, ComboPhase,
    CreatureBehaviour, CreatureId, CreatureTemplate, EntityRef, ErrorCode, GLOBAL_COOLDOWN_TICKS,
    StrikeAim, StrikeHeight, StrikeSide, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, GRAVEYARD, WOLF};

/// Spawn slots grow north, so the creatures stand to the south.
const SOUTH: u16 = 32_768;
const STRUCK: EntityRef = EntityRef::Creature(CreatureId::new(1));
const OTHER: EntityRef = EntityRef::Creature(CreatureId::new(2));

/// Two sturdy neutral wolves south of the spawn, both within reach.
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
            vec![
                arena::spawn(1, WOLF, [0, -150]),
                arena::spawn(2, WOLF, [150, -150]),
            ],
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
    /// A level-2 Warden (Shield Bash learned) at the origin with full Rage.
    fn new() -> Self {
        let content = content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass { class: 0, sex: 0 });
        warden.send(arena::stand(SOUTH));
        for _ in 0..20 {
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

    fn slash(&mut self, side: StrikeSide) -> Vec<ZoneEvent> {
        self.send(ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(side, StrikeHeight::Middle),
        });
        self.tick()
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: None,
        });
    }

    fn combo(&self) -> Option<ComboPhase> {
        self.zone.snapshot_for_player(1).unwrap().viewer.combo
    }
}

fn bash_target(events: &[ZoneEvent]) -> Option<Option<EntityRef>> {
    events.iter().find_map(|event| match *event {
        ZoneEvent::AbilityUsed {
            ability, target, ..
        } if ability == ids::SHIELD_BASH => Some(target),
        _ => None,
    })
}

fn errors(events: &[ZoneEvent]) -> Vec<ErrorCode> {
    events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

#[test]
fn a_confirmed_slash_lets_an_early_bash_follow_up_on_the_struck_creature() {
    let mut warden = Warden::new();
    let events = warden.slash(StrikeSide::Centre);
    assert!(events.iter().any(|event| matches!(
        event,
        ZoneEvent::DamageDealt { target: STRUCK, amount, .. } if *amount > 0
    )));
    assert_eq!(warden.combo(), Some(ComboPhase::Impact));
    // Select another creature, then request the Bash during the impact.
    warden.send(ZoneCommand::SelectTarget(Some(OTHER)));
    warden.bash();
    let mut waited = 0;
    let target = loop {
        let events = warden.tick();
        if let Some(target) = bash_target(&events) {
            break target;
        }
        assert_eq!(errors(&events), [], "the early Bash waits, not refused");
        waited += 1;
        assert!(
            waited <= COMBO_IMPACT_TICKS,
            "the Bash resolves when the window opens"
        );
    };
    // The slash's global cooldown is still running, yet the Bash lands on
    // the struck creature, not the selection, and starts its own cooldown.
    assert!(u32::from(waited) < u32::from(GLOBAL_COOLDOWN_TICKS));
    assert_eq!(target, Some(STRUCK));
    assert_eq!(warden.combo(), None);
    let viewer = warden.zone.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(viewer.global_cooldown, GLOBAL_COOLDOWN_TICKS - 1);
    assert_eq!(viewer.target, Some(OTHER));
}

#[test]
fn a_whiff_opens_nothing_and_the_slash_cooldown_still_applies() {
    let mut warden = Warden::new();
    // Nothing stands west of the spawn.
    let events = warden.slash(StrikeSide::Left);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ZoneEvent::StrikeSwept { target: None, .. }))
    );
    assert_eq!(warden.combo(), None);
    warden.bash();
    assert_eq!(errors(&warden.tick()), [ErrorCode::NotReady]);
}

#[test]
fn the_window_closes_and_a_late_bash_obeys_the_cooldown() {
    let mut warden = Warden::new();
    warden.slash(StrikeSide::Centre);
    for _ in 0..COMBO_IMPACT_TICKS + COMBO_WINDOW_TICKS {
        warden.tick();
    }
    assert_eq!(warden.combo(), None);
    warden.bash();
    assert_eq!(errors(&warden.tick()), [ErrorCode::NotReady]);
}

#[test]
fn the_opening_survives_a_checkpoint() {
    let mut warden = Warden::new();
    warden.slash(StrikeSide::Centre);
    let state = warden.zone.snapshot().unwrap();
    let restored =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(warden.zone.content())).unwrap();
    assert_eq!(restored.snapshot().unwrap(), state);
    assert_eq!(
        restored.snapshot_for_player(1).unwrap().viewer.combo,
        Some(ComboPhase::Impact)
    );
}
