//! Scripted class fights through the public zone API: the class choice,
//! resources, the global cooldown, cooldowns, casts and channels, movement
//! interrupts and every aura kind, plus the creature abilities of revision 6.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, AuraKind, CanonicalCreatureSnapshot, CanonicalPlayerSnapshot, CreatureId, EntityRef,
    ErrorCode, ResourceKind, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
    ZoneSnapshot,
};
use support::arena::{self, NORTHWARD, WOLF};
use support::classes::{self, BANDIT, LURKER};

const PLAYER: EntityRef = EntityRef::Player(1);
const FIRST: CreatureId = CreatureId::new(1);
const TARGET: EntityRef = EntityRef::Creature(FIRST);

/// One player in a class fight, with every event it received by tick.
struct Fight {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Fight {
    fn new(content: Arc<ZoneContent>, level: u8) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        Self {
            zone: classes::at_level(zone, 1, level),
            sequence: 0,
            events: Vec::new(),
        }
    }

    /// A fight whose player chose `class` (0 Warden, 1 Ranger, 2 Arcanist).
    fn with_class(content: Arc<ZoneContent>, level: u8, class: u8) -> Self {
        let mut fight = Self::new(content, level);
        fight.send(ZoneCommand::ChooseClass { class, sex: 1 });
        fight.tick();
        fight
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn use_ability(&mut self, ability: AbilityId) {
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: None,
        });
    }

    fn tick(&mut self) -> Vec<ZoneEvent> {
        self.zone.advance_tick().unwrap();
        let events = self.view().events;
        let tick = self.zone.current_tick();
        self.events
            .extend(events.iter().map(|&event| (tick, event)));
        events
    }

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Ticks until `done` holds, at most `limit` ticks; returns the tick.
    fn until(&mut self, limit: u64, done: impl Fn(&Self) -> bool) -> u64 {
        for _ in 0..limit {
            self.tick();
            if done(self) {
                return self.zone.current_tick();
            }
        }
        panic!("condition not reached within {limit} ticks");
    }

    fn view(&self) -> ZoneSnapshot {
        self.zone.snapshot_for_player(1).unwrap()
    }

    fn me(&self) -> CanonicalPlayerSnapshot {
        self.zone.snapshot().unwrap().players.remove(0)
    }

    fn creature(&self, id: CreatureId) -> CanonicalCreatureSnapshot {
        self.zone
            .snapshot()
            .unwrap()
            .creatures
            .into_iter()
            .find(|creature| creature.creature_id == id)
            .unwrap()
    }

    fn resource(&self) -> u16 {
        self.view().viewer.resource.unwrap().value
    }

    /// Events received since `tick` (inclusive) that `keep` selects.
    fn since(&self, tick: u64, keep: impl Fn(&ZoneEvent) -> bool) -> Vec<(u64, ZoneEvent)> {
        self.events
            .iter()
            .copied()
            .filter(|(at, event)| *at >= tick && keep(event))
            .collect()
    }

    fn errors_at(&self, tick: u64) -> Vec<ErrorCode> {
        self.since(tick, |_| true)
            .into_iter()
            .filter(|(at, _)| *at == tick)
            .filter_map(|(_, event)| match event {
                ZoneEvent::Error { code, .. } => Some(code),
                _ => None,
            })
            .collect()
    }
}

fn damage_dealt_to(event: &ZoneEvent, unit: EntityRef) -> Option<u16> {
    match *event {
        ZoneEvent::DamageDealt { target, amount, .. } if target == unit => Some(amount),
        _ => None,
    }
}

/// One sturdy, weak-hitting wolf `distance` units north of the player.
fn wolf_at(distance: i32) -> Arc<ZoneContent> {
    classes::arena(
        vec![arena::wolf(400, [1, 1])],
        vec![arena::spawn(1, WOLF, [0, distance])],
    )
}

#[test]
fn players_choose_a_class_once_and_need_one_for_abilities() {
    let mut fight = Fight::new(wolf_at(3_000), 1);
    let unchosen = fight.view();
    assert_eq!(unchosen.viewer.class, None);
    assert_eq!(unchosen.viewer.resource, None);
    assert_eq!(unchosen.entities[0].appearance, 0);

    fight.use_ability(ids::HEROIC_STRIKE);
    fight.send(ZoneCommand::ChooseClass { class: 3, sex: 0 });
    fight.send(ZoneCommand::ChooseClass { class: 0, sex: 2 });
    fight.tick();
    assert_eq!(
        fight.errors_at(1),
        [
            ErrorCode::NoClass,
            ErrorCode::InvalidClass,
            ErrorCode::InvalidClass
        ]
    );

    fight.send(ZoneCommand::ChooseClass { class: 2, sex: 0 });
    fight.send(ZoneCommand::ChooseClass { class: 0, sex: 1 });
    fight.tick();
    assert_eq!(fight.errors_at(2), [ErrorCode::InvalidClass], "only once");
    let view = fight.view();
    let choice = view.viewer.class.unwrap();
    assert_eq!(choice.class, mmorpg_core::PlayerClass::Arcanist);
    assert_eq!(view.entities[0].appearance, 5, "1 + 2 × 2 + female");
    let mana = view.viewer.resource.unwrap();
    assert_eq!(
        (mana.kind, mana.value, mana.max),
        (ResourceKind::Mana, 110, 110)
    );

    // Validation order: learned before everything else.
    fight.use_ability(ids::HEROIC_STRIKE);
    fight.use_ability(ids::FROST_NOVA);
    fight.use_ability(ids::MUCK_BOLT);
    fight.send(ZoneCommand::UseAbility {
        ability: 0,
        target: None,
    });
    fight.use_ability(ids::FIREBOLT);
    fight.tick();
    assert_eq!(
        fight.errors_at(3),
        [
            ErrorCode::NotLearned,
            ErrorCode::NotLearned,
            ErrorCode::NotLearned,
            ErrorCode::NotLearned,
            ErrorCode::InvalidTarget
        ]
    );
}

#[test]
fn a_warden_builds_rage_and_spends_it_on_strikes_bashes_cries_and_cleaves() {
    // Three packmates west of the spawn row: the target 2 m away and two
    // within 3 m of it.
    let content = classes::arena(
        vec![arena::wolf(400, [1, 1])],
        vec![
            arena::spawn(1, WOLF, [-200, 0]),
            arena::spawn(2, WOLF, [-350, -150]),
            arena::spawn(3, WOLF, [-350, 150]),
        ],
    );
    let mut fight = Fight::with_class(content, 6, 0);
    // Rage starts empty; the wolf that engaged at once already hit once.
    assert!(fight.resource() < 15);
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.send(ZoneCommand::StartAttack);
    fight.use_ability(ids::HEROIC_STRIKE);
    fight.tick();
    assert_eq!(fight.errors_at(2), [ErrorCode::NotEnoughResource]);

    // Rage: +6 per swing hit dealt, +4 per hit taken.
    fight.until(600, |fight| fight.resource() >= 60);
    let dealt = fight
        .since(0, |event| damage_dealt_to(event, TARGET).is_some())
        .len();
    let taken = fight
        .since(0, |event| matches!(event, ZoneEvent::DamageTaken { .. }))
        .len();
    assert!(dealt > 0 && taken > 0);
    assert_eq!(usize::from(fight.resource()), 6 * dealt + 4 * taken);

    // Heroic Strike: 15 rage, the global cooldown, damage.
    let before = fight.resource();
    fight.use_ability(ids::HEROIC_STRIKE);
    fight.use_ability(ids::SHIELD_BASH);
    let events = fight.tick();
    let now = fight.zone.current_tick();
    assert!(events.contains(&ZoneEvent::AbilityUsed {
        source: PLAYER,
        target: Some(TARGET),
        ability: ids::HEROIC_STRIKE,
    }));
    assert_eq!(
        fight.errors_at(now),
        [ErrorCode::NotReady],
        "global cooldown"
    );
    let me = fight.me();
    assert_eq!(me.combat.abilities.global_cooldown, 44);
    let strike = events
        .iter()
        .find_map(|event| damage_dealt_to(event, TARGET))
        .unwrap();
    // Weapon damage at level 6 is 8–11: × 3/2 + 6.
    assert!((18..=22).contains(&strike), "{strike}");
    // 15 rage, less what this tick's swings dealt (+6) and took (+4).
    let gained = events
        .iter()
        .map(|event| match event {
            ZoneEvent::DamageDealt { .. } => 6,
            ZoneEvent::DamageTaken { .. } => 4,
            _ => 0,
        })
        .sum::<u16>()
        - 6;
    assert_eq!(before + gained - 15, fight.resource());

    // Shield Bash after the global cooldown: a 60-tick stun and a cooldown.
    fight.ticks(44);
    fight.use_ability(ids::SHIELD_BASH);
    fight.tick();
    let bashed = fight.zone.current_tick();
    let detail = fight.view().target_detail;
    assert_eq!(detail.auras.len(), 1);
    assert_eq!(
        (detail.auras[0].kind, detail.auras[0].remaining),
        (AuraKind::Stun, 59)
    );
    assert_eq!(
        fight.view().cooldowns,
        [mmorpg_core::Cooldown {
            ability: ids::SHIELD_BASH,
            remaining: 359,
        }]
    );
    fight.ticks(45);
    fight.use_ability(ids::SHIELD_BASH);
    fight.tick();
    assert_eq!(
        fight.errors_at(fight.zone.current_tick()),
        [ErrorCode::NotReady]
    );
    // The stunned wolf neither swung nor moved during the stun.
    let wolf_hits = fight.since(
        bashed + 1,
        |event| matches!(event, ZoneEvent::DamageTaken { source, .. } if *source == TARGET),
    );
    assert!(
        wolf_hits.iter().all(|(at, _)| *at > bashed + 59),
        "{wolf_hits:?}"
    );

    // Cleave hits the target and both packmates within 3 m of it.
    fight.until(300, |fight| {
        fight.resource() >= 20 && fight.me().combat.abilities.global_cooldown == 0
    });
    fight.use_ability(ids::CLEAVE);
    let events = fight.tick();
    let hit: Vec<_> = events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::DamageDealt { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    for wolf in 1..=3 {
        assert!(
            hit.contains(&EntityRef::Creature(CreatureId::new(wolf))),
            "{hit:?}"
        );
    }

    // Rallying Cry heals 30 % of 125 = 38 over ten pulses.
    fight.until(300, |fight| {
        fight.resource() >= 20 && fight.me().combat.abilities.global_cooldown == 0
    });
    fight.use_ability(ids::RALLYING_CRY);
    fight.tick();
    let cried = fight.zone.current_tick();
    let auras = fight.view().auras;
    assert_eq!(
        (auras[0].ability, auras[0].kind, auras[0].amount),
        (ids::RALLYING_CRY, AuraKind::HealOverTime, 38)
    );
    fight.ticks(300);
    let heals = fight.since(cried, |event| matches!(event, ZoneEvent::Healed { .. }));
    assert!(!heals.is_empty());
    for (at, _) in &heals {
        assert_eq!((at + 1 - cried) % 30, 0, "heals pulse every 30 ticks");
    }
    assert!(
        fight
            .since(cried, |event| matches!(
                event,
                ZoneEvent::AuraRemoved { ability, .. } if *ability == ids::RALLYING_CRY
            ))
            .len()
            == 1
    );
}

#[test]
fn a_ranger_spends_focus_on_shots_stings_snares_and_haste() {
    let content = classes::arena(
        vec![arena::wolf(300, [1, 1])],
        vec![
            arena::spawn(1, WOLF, [0, 2_500]),
            arena::spawn(2, WOLF, [3_000, 1_800]),
        ],
    );
    let mut fight = Fight::with_class(content, 6, 1);
    assert_eq!(fight.resource(), 100, "focus starts full");

    // Beyond 30 m: out of range, nothing spent.
    fight.send(ZoneCommand::UseAbility {
        ability: ids::AIMED_SHOT.get(),
        target: Some(EntityRef::Creature(CreatureId::new(2))),
    });
    fight.tick();
    assert_eq!(fight.errors_at(2), [ErrorCode::OutOfRange]);
    assert_eq!(fight.resource(), 100);

    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.use_ability(ids::AIMED_SHOT);
    let events = fight.tick();
    let shot = events
        .iter()
        .find_map(|event| damage_dealt_to(event, TARGET))
        .unwrap();
    // 2 × weapon damage (8–11) + 8.
    assert!((24..=30).contains(&shot), "{shot}");
    assert_eq!(fight.resource(), 70);

    // Focus regenerates 5 every 30 ticks.
    fight.ticks(44);
    assert_eq!(fight.resource(), 75);
    fight.use_ability(ids::SERPENT_STING);
    fight.tick();
    let stung = fight.zone.current_tick();
    assert_eq!(fight.resource(), 55);

    // Concussive Shot halves the charging wolf's speed.
    fight.ticks(44);
    fight.use_ability(ids::CONCUSSIVE_SHOT);
    fight.tick();
    let velocity = fight.creature(FIRST).velocity;
    assert!(velocity[2] < 0 && velocity[2] >= -9, "{velocity:?}");
    assert_eq!(
        fight.view().target_detail.auras[1].kind,
        AuraKind::Snare,
        "after the sting"
    );

    // Serpent Sting: 30 + 4 × 5 = 50 over five pulses every 90 ticks,
    // counted from the tick it landed.
    fight.ticks(450);
    let pulses = fight.since(stung, |event| damage_dealt_to(event, TARGET).is_some());
    let pulses: Vec<_> = pulses
        .into_iter()
        .map(|(at, event)| (at - stung, damage_dealt_to(&event, TARGET).unwrap()))
        .collect();
    assert_eq!(
        pulses,
        [(89, 10), (179, 10), (269, 10), (359, 10), (449, 10)]
    );

    // Rapid Fire hastes swings: 60 × 100 / 140 = 42 ticks.
    fight.use_ability(ids::RAPID_FIRE);
    fight.send(ZoneCommand::StartAttack);
    fight.until(200, |fight| fight.me().combat.swing_timer > 0);
    assert_eq!(fight.me().combat.swing_timer, 42, "hasted from 60");
    assert_eq!(fight.view().auras[0].kind, AuraKind::Haste);
}

#[test]
fn an_arcanist_casts_channels_roots_and_shields_by_the_five_second_rule() {
    let content = classes::arena(
        vec![arena::wolf(300, [3, 3])],
        vec![arena::spawn(1, WOLF, [0, 2_500])],
    );
    let mut fight = Fight::with_class(content, 6, 2);
    assert_eq!(fight.resource(), 220, "110 + 22 × 5");
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));

    // Moving interrupts a cast, which costs nothing.
    fight.use_ability(ids::FIREBOLT);
    fight.tick();
    assert_eq!(fight.view().viewer.cast.unwrap().elapsed, 1);
    fight.send(arena::walk(NORTHWARD));
    let events = fight.tick();
    assert!(events.contains(&ZoneEvent::Interrupted {
        source: None,
        target: PLAYER,
        ability: ids::FIREBOLT,
    }));
    assert_eq!(fight.view().viewer.cast, None);
    assert_eq!(fight.resource(), 220);
    fight.send(arena::stand(NORTHWARD));

    // Firebolt: a 60-tick cast that pays on completion.
    fight.ticks(45);
    fight.use_ability(ids::FIREBOLT);
    fight.tick();
    let started = fight.zone.current_tick();
    fight.use_ability(ids::FROST_NOVA);
    fight.tick();
    assert_eq!(fight.errors_at(started + 1), [ErrorCode::AlreadyCasting]);
    fight.until(70, |fight| fight.view().viewer.cast.is_none());
    let landed = fight.zone.current_tick();
    assert_eq!(landed - started, 59, "60 ticks including the start tick");
    let bolt = fight
        .since(landed, |event| damage_dealt_to(event, TARGET).is_some())
        .remove(0)
        .1;
    // 10–14 + 3 × 5.
    assert!((25..=29).contains(&damage_dealt_to(&bolt, TARGET).unwrap()));
    assert_eq!(fight.resource(), 195);
    let me = fight.me();
    // The five-second rule counts the spending tick.
    assert_eq!(me.combat.abilities.mana_delay, 149);
    fight.ticks(149 + 29);
    assert_eq!(fight.resource(), 195, "no regeneration for 150 ticks");
    fight.tick();
    assert!(fight.resource() > 195);

    // Frost Nova roots the wolf once it reached melee and hit.
    fight.until(300, |fight| {
        !fight
            .since(landed, |event| {
                matches!(event, ZoneEvent::DamageTaken { .. })
            })
            .is_empty()
    });
    fight.use_ability(ids::FROST_NOVA);
    fight.tick();
    let rooted = fight.zone.current_tick();
    let velocity = fight.creature(FIRST).velocity;
    assert_eq!([velocity[0], velocity[2]], [0, 0]);
    let root = fight.view().target_detail.auras[0];
    assert_eq!((root.kind, root.remaining), (AuraKind::Root, 179));

    // Arcane Barrier absorbs 20 + 8 × 5 = 60 damage before health.
    fight.ticks(44);
    let health = fight.view().viewer.health;
    fight.use_ability(ids::ARCANE_BARRIER);
    fight.tick();
    let shielded = fight.zone.current_tick();
    fight.until(200, |fight| {
        fight
            .since(shielded, |event| {
                matches!(event, ZoneEvent::Absorbed { .. })
            })
            .len()
            >= 2
    });
    assert_eq!(
        fight.view().viewer.health,
        health,
        "the shield took the hits"
    );
    assert_eq!(fight.view().auras[0].amount, 54);

    // Damage after the first 30 ticks breaks the root.
    assert!(fight.zone.current_tick() - rooted >= 30);
    fight.use_ability(ids::FIREBOLT);
    fight.until(70, |fight| fight.view().viewer.cast.is_none());
    assert!(
        !fight
            .since(rooted, |event| matches!(
                event,
                ZoneEvent::AuraRemoved { ability, .. } if *ability == ids::FROST_NOVA
            ))
            .is_empty()
    );
    assert!(fight.view().target_detail.auras.is_empty());

    // Blizzard pays 80 at the start and pulses six times.
    fight.until(600, |fight| {
        fight.resource() >= 80 && fight.me().combat.abilities.global_cooldown == 0
    });
    let before = fight.resource();
    fight.use_ability(ids::BLIZZARD);
    fight.tick();
    let channelled = fight.zone.current_tick();
    assert_eq!(fight.resource(), before - 80);
    let cast = fight.view().viewer.cast.unwrap();
    assert!(cast.channel && cast.total == 180);
    fight.ticks(180);
    let pulses = fight.since(channelled, |event| damage_dealt_to(event, TARGET).is_some());
    let offsets: Vec<_> = pulses.iter().map(|(at, _)| at - channelled).collect();
    assert_eq!(offsets, [29, 59, 89, 119, 149, 179]);
    for (_, event) in pulses {
        // 4–6 + 5.
        assert!((9..=11).contains(&damage_dealt_to(&event, TARGET).unwrap()));
    }
}

#[test]
fn a_jump_interrupts_a_channel_that_keeps_its_cost() {
    let mut fight = Fight::with_class(wolf_at(2_500), 6, 2);
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.use_ability(ids::BLIZZARD);
    fight.tick();
    assert_eq!(fight.resource(), 140);
    fight.ticks(40);
    fight.send(ZoneCommand::Jump);
    let events = fight.tick();
    assert!(events.contains(&ZoneEvent::Interrupted {
        source: None,
        target: PLAYER,
        ability: ids::BLIZZARD,
    }));
    assert_eq!(fight.view().viewer.cast, None);
    assert_eq!(fight.resource(), 140, "channels pay at the start");
    // Only the pulse at 30 ticks landed.
    assert_eq!(
        fight
            .since(0, |event| damage_dealt_to(event, TARGET).is_some())
            .len(),
        1
    );
}

#[test]
fn a_lurker_casts_muck_bolt_until_a_shield_bash_interrupts_it() {
    let content = classes::arena(
        vec![classes::lurker(500, [1, 1])],
        vec![arena::spawn(1, LURKER, [-200, 0])],
    );
    let mut fight = Fight::with_class(content, 2, 0);
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.send(ZoneCommand::StartAttack);
    let first = fight.until(40, |fight| {
        !fight
            .since(0, |event| matches!(event, ZoneEvent::CastStarted { .. }))
            .is_empty()
    });
    assert!(fight.events.contains(&(
        first,
        ZoneEvent::CastStarted {
            source: TARGET,
            target: Some(PLAYER),
            ability: ids::MUCK_BOLT,
            ticks: 45,
        }
    )));
    // The bolt lands 45 ticks later for 5–8 damage.
    fight.ticks(44);
    let landed = fight.since(
        first + 44,
        |event| matches!(event, ZoneEvent::DamageTaken { source, .. } if *source == TARGET),
    );
    let ZoneEvent::DamageTaken { amount, .. } = landed[0].1 else {
        unreachable!()
    };
    assert!((5..=8).contains(&amount) || amount == 1, "{amount}");
    assert!(fight.events.contains(&(
        first + 44,
        ZoneEvent::AbilityUsed {
            source: TARGET,
            target: Some(PLAYER),
            ability: ids::MUCK_BOLT,
        }
    )));

    // The next cast follows after 240–270 ticks; Shield Bash interrupts it.
    let second = fight.until(300, |fight| fight.creature(FIRST).abilities.cast.is_some());
    assert!(
        (first + 44 + 240..=first + 44 + 270).contains(&second),
        "{second}"
    );
    assert!(fight.resource() >= 10);
    fight.use_ability(ids::SHIELD_BASH);
    let events = fight.tick();
    assert!(events.contains(&ZoneEvent::Interrupted {
        source: Some(PLAYER),
        target: TARGET,
        ability: ids::MUCK_BOLT,
    }));
    let lurker = fight.creature(FIRST);
    assert_eq!(lurker.abilities.cast, None);
    assert_eq!(lurker.abilities.ability_timer, 119, "locked for 120 ticks");
    let bashed = fight.zone.current_tick();
    let next = fight.until(200, |fight| fight.creature(FIRST).abilities.cast.is_some());
    assert_eq!(next, bashed + 120);
}

#[test]
fn a_bandit_bandages_itself_only_below_half_health() {
    let content = classes::arena(
        vec![classes::bandit(100, [1, 1])],
        vec![arena::spawn(1, BANDIT, [-200, 0])],
    );
    // An unchosen player keeps auto-attack; the bandit's bandage still works.
    let mut fight = Fight::new(content, 1);
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.send(ZoneCommand::StartAttack);
    let casting = fight.until(2_000, |fight| {
        fight.creature(FIRST).abilities.cast.is_some()
    });
    let bandit = fight.creature(FIRST);
    assert!(bandit.health < 50, "{}", bandit.health);
    // Every earlier tick it stood at or above half health.
    let started = fight
        .since(0, |event| matches!(event, ZoneEvent::CastStarted { .. }))
        .remove(0);
    assert_eq!(started.0, casting);
    let health = bandit.health;
    fight.ticks(89);
    let healed = fight.since(casting, |event| matches!(event, ZoneEvent::Healed { .. }));
    let (at, ZoneEvent::Healed { amount, .. }) = healed[0] else {
        unreachable!()
    };
    assert_eq!((at - casting, amount), (89, 20), "20 % of 100");
    assert!(fight.creature(FIRST).health > health);
}

#[test]
fn recovery_rejects_aura_amounts_no_player_level_reaches() {
    let mut fight = Fight::with_class(wolf_at(3_000), 4, 2);
    fight.use_ability(ids::ARCANE_BARRIER);
    fight.tick();
    let checkpoint = fight.zone.snapshot().unwrap();
    let content = Arc::clone(fight.zone.content());
    let restore = |ability: AbilityId, remaining: u16, amount: u16| {
        let mut state = checkpoint.clone();
        let aura = &mut state.players[0].combat.abilities.auras[0];
        (aura.ability, aura.remaining, aura.amount) = (ability, remaining, amount);
        ZoneSimulation::from_snapshot(state, Arc::clone(&content)).map(|_| ())
    };
    // Arcane Barrier at level 10: 20 + 8 × 9 = 92; an empty shield is gone.
    assert_eq!(checkpoint.players[0].combat.abilities.auras[0].amount, 44);
    assert!(restore(ids::ARCANE_BARRIER, 599, 92).is_ok());
    // Serpent Sting at level 10: 30 + 4 × 9 = 66; Rallying Cry: 30 % of
    // 185 + 8 stamina × 5 = 225, rounded up: 68.
    // (Amounts are checked before reachability, so these Arcanist-borne
    // records still fail on their amount alone.)
    for (ability, remaining, amount) in [
        (ids::ARCANE_BARRIER, 599, 93),
        (ids::ARCANE_BARRIER, 599, u16::MAX),
        (ids::ARCANE_BARRIER, 599, 0),
        (ids::SERPENT_STING, 449, 67),
        (ids::RALLYING_CRY, 299, 69),
        (ids::RALLYING_CRY, 299, 0),
        (ids::FROST_NOVA, 179, 1),
    ] {
        assert_eq!(
            restore(ability, remaining, amount).unwrap_err().message(),
            "aura state is out of range",
            "{ability:?} {amount}"
        );
    }
}

#[test]
fn a_pulse_that_breaks_an_earlier_root_skips_no_later_aura() {
    // Slots on the wolf: Frost Nova's root, Serpent Sting, Concussive Shot.
    let content = classes::arena(
        vec![arena::wolf(2_000, [1, 1])],
        vec![arena::spawn(1, WOLF, [-200, 0])],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    let mut zone = classes::at_level(classes::at_level(zone, 1, 6), 2, 6);
    let mut sequences = [0_u32; 2];
    let mut send = |zone: &mut ZoneSimulation, player: u32, command: ZoneCommand| {
        let sequence = &mut sequences[usize::try_from(player).unwrap() - 1];
        *sequence += 1;
        zone.apply_command(player, *sequence, command).unwrap();
    };
    let ability = |ability: AbilityId| ZoneCommand::UseAbility {
        ability: ability.get(),
        target: Some(TARGET),
    };
    send(&mut zone, 1, ZoneCommand::ChooseClass { class: 2, sex: 0 });
    send(&mut zone, 2, ZoneCommand::ChooseClass { class: 1, sex: 1 });
    zone.advance_tick().unwrap();
    send(&mut zone, 1, ability(ids::FROST_NOVA));
    send(&mut zone, 2, ability(ids::SERPENT_STING));
    zone.advance_tick().unwrap();
    let stung = zone.current_tick();
    for _ in 0..45 {
        zone.advance_tick().unwrap();
    }
    send(&mut zone, 2, ability(ids::CONCUSSIVE_SHOT));
    zone.advance_tick().unwrap();
    let snared = zone.current_tick();
    let kinds = |zone: &ZoneSimulation| {
        zone.snapshot().unwrap().creatures[0]
            .abilities
            .auras
            .iter()
            .map(|aura| (aura.ability, aura.remaining))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(&zone)
            .iter()
            .map(|(ability, _)| *ability)
            .collect::<Vec<_>>(),
        [ids::FROST_NOVA, ids::SERPENT_STING, ids::CONCUSSIVE_SHOT]
    );
    // The first sting pulse (89 ticks after it landed) breaks the root in
    // slot 0; the snare behind it still counts down that tick.
    while zone.current_tick() < stung + 89 {
        zone.advance_tick().unwrap();
    }
    let elapsed = u16::try_from(zone.current_tick() - snared + 1).unwrap();
    assert_eq!(
        kinds(&zone),
        [
            (ids::SERPENT_STING, 450 - 90),
            (ids::CONCUSSIVE_SHOT, 120 - elapsed),
        ]
    );
}

#[test]
fn recovery_rejects_casts_at_the_wrong_kind_of_unit() {
    let mut fight = Fight::with_class(wolf_at(2_500), 6, 2);
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.use_ability(ids::BLIZZARD);
    fight.tick();
    let checkpoint = fight.zone.snapshot().unwrap();
    let content = Arc::clone(fight.zone.content());
    assert!(ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(&content)).is_ok());
    let mut at_player = checkpoint;
    at_player.players[0]
        .combat
        .abilities
        .cast
        .as_mut()
        .unwrap()
        .target = Some(PLAYER);
    assert_eq!(
        ZoneSimulation::from_snapshot(at_player, Arc::clone(&content))
            .err()
            .unwrap()
            .message(),
        "a cast names a target of the wrong kind"
    );

    let lurking = classes::arena(
        vec![classes::lurker(500, [1, 1])],
        vec![arena::spawn(1, LURKER, [-200, 0])],
    );
    let mut fight = Fight::new(lurking, 1);
    fight.until(40, |fight| fight.creature(FIRST).abilities.cast.is_some());
    let mut at_creature = fight.zone.snapshot().unwrap();
    at_creature.creatures[0]
        .abilities
        .cast
        .as_mut()
        .unwrap()
        .target = Some(TARGET);
    assert_eq!(
        ZoneSimulation::from_snapshot(at_creature, Arc::clone(fight.zone.content()))
            .err()
            .unwrap()
            .message(),
        "a cast names a target of the wrong kind"
    );
}

#[test]
fn recovery_requires_a_dead_warden_without_rage() {
    let mut fight = Fight::with_class(wolf_at(3_000), 1, 0);
    let content = Arc::clone(fight.zone.content());
    fight.tick();
    let mut dead = fight.zone.snapshot().unwrap();
    dead.players[0].combat.health = 0;
    let restore = |resource: u16, ticks: u16| {
        let mut state = dead.clone();
        state.players[0].combat.abilities.resource = resource;
        state.players[0].combat.abilities.resource_ticks = ticks;
        ZoneSimulation::from_snapshot(state, Arc::clone(&content)).map(|_| ())
    };
    assert!(restore(0, 0).is_ok());
    for (resource, ticks) in [(20, 0), (0, 5)] {
        assert_eq!(
            restore(resource, ticks).unwrap_err().message(),
            "a dead Warden has no rage"
        );
    }
}

#[test]
fn recovery_rejects_auras_no_caster_could_have_put_there() {
    let mut fight = Fight::with_class(wolf_at(3_000), 4, 2);
    fight.use_ability(ids::ARCANE_BARRIER);
    fight.tick();
    let checkpoint = fight.zone.snapshot().unwrap();
    let content = Arc::clone(fight.zone.content());
    let barrier = checkpoint.players[0].combat.abilities.auras[0];
    let root = mmorpg_core::Aura {
        ability: ids::FROST_NOVA,
        remaining: 100,
        amount: 0,
        ..barrier
    };
    let restore = |player: Vec<mmorpg_core::Aura>, creature: Vec<mmorpg_core::Aura>| {
        let mut state = checkpoint.clone();
        state.players[0].combat.abilities.auras = player;
        state.creatures[0].abilities.auras = creature;
        ZoneSimulation::from_snapshot(state, Arc::clone(&content)).map(|_| ())
    };
    assert!(
        restore(vec![barrier], vec![root]).is_ok(),
        "a root on a creature"
    );
    for (player, creature) in [
        // A Warden ability the Arcanist never learned.
        (
            vec![mmorpg_core::Aura {
                ability: ids::RALLYING_CRY,
                remaining: 299,
                amount: 30,
                ..barrier
            }],
            vec![],
        ),
        // A hostile aura on a player, and a self aura on a creature.
        (vec![root], vec![]),
        (vec![], vec![barrier]),
        // A shield cast by another unit than its bearer.
        (
            vec![mmorpg_core::Aura {
                caster: TARGET,
                ..barrier
            }],
            vec![],
        ),
    ] {
        assert_eq!(
            restore(player, creature).unwrap_err().message(),
            "an aura's caster or recipient is unreachable"
        );
    }
}

#[test]
fn a_departing_target_ends_a_creature_cast_and_recovery_stays_valid() {
    let content = classes::arena(
        vec![classes::lurker(500, [1, 1])],
        vec![arena::spawn(1, LURKER, [-200, 0])],
    );
    let mut fight = Fight::new(content, 1);
    fight.until(40, |fight| fight.creature(FIRST).abilities.cast.is_some());
    let checkpoint = fight.zone.snapshot().unwrap();
    ZoneSimulation::from_snapshot(checkpoint, Arc::clone(fight.zone.content()))
        .expect("a creature cast restores");
    assert!(fight.zone.remove_player(1));
    assert_eq!(fight.creature(FIRST).abilities.cast, None);
    ZoneSimulation::from_snapshot(
        fight.zone.snapshot().unwrap(),
        Arc::clone(fight.zone.content()),
    )
    .expect("no cast names the departed player");
}

#[test]
fn recovery_rejects_creature_casts_at_dead_players() {
    let lurking = classes::arena(
        vec![classes::lurker(500, [1, 1])],
        vec![arena::spawn(1, LURKER, [-200, 0])],
    );
    let mut fight = Fight::new(lurking, 1);
    fight.until(40, |fight| fight.creature(FIRST).abilities.cast.is_some());
    let checkpoint = fight.zone.snapshot().unwrap();
    let content = Arc::clone(fight.zone.content());
    assert!(ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(&content)).is_ok());
    let mut dead = checkpoint;
    dead.players[0].combat.health = 0;
    assert_eq!(
        ZoneSimulation::from_snapshot(dead, content)
            .err()
            .unwrap()
            .message(),
        "a creature casts at a dead player"
    );
}

#[test]
fn an_arcanist_level_up_grows_current_mana_with_its_maximum() {
    let mut fight = Fight::with_class(
        classes::arena(
            vec![arena::wolf(1, [1, 1])],
            vec![arena::spawn(1, WOLF, [-200, 0])],
        ),
        1,
        2,
    );
    assert_eq!(fight.resource(), ResourceKind::Mana.max(1));
    let mut state = fight.zone.snapshot().unwrap();
    state.players[0].combat.experience = mmorpg_core::experience_to_next_level(1).unwrap() - 1;
    fight.zone = ZoneSimulation::from_snapshot(state, Arc::clone(fight.zone.content())).unwrap();
    fight.send(arena::stand(NORTHWARD));
    fight.send(ZoneCommand::SelectTarget(Some(TARGET)));
    fight.send(ZoneCommand::StartAttack);
    fight.until(200, |fight| fight.me().combat.level == 2);
    assert_eq!(fight.resource(), 132);
    assert_eq!(fight.resource(), ResourceKind::Mana.max(2));
}

#[test]
fn content_without_the_ability_catalog_refuses_classes() {
    let unbound = arena::arena(Vec::new(), Vec::new(), Vec::new());
    assert_eq!(unbound.ability_revision(), 0);
    let mut fight = Fight::new(unbound, 1);
    fight.send(ZoneCommand::ChooseClass { class: 2, sex: 0 });
    let tick = fight.zone.current_tick() + 1;
    fight.tick();
    assert_eq!(fight.errors_at(tick), [ErrorCode::InvalidClass]);
    assert_eq!(fight.me().combat.abilities.class, None);
    fight.use_ability(ids::FIREBOLT);
    let tick = fight.zone.current_tick() + 1;
    fight.tick();
    assert_eq!(fight.errors_at(tick), [ErrorCode::NoClass]);

    // A class checkpoint does not restore against content that never bound
    // the catalog: its fingerprint does not cover the ability rules.
    let classed = Fight::with_class(wolf_at(3_000), 1, 2).me();
    let mut state = fight.zone.snapshot().unwrap();
    state.players[0].combat.abilities = classed.combat.abilities;
    assert_eq!(
        ZoneSimulation::from_snapshot(state, Arc::clone(fight.zone.content()))
            .err()
            .unwrap()
            .message(),
        "a class player needs content that binds the ability catalog"
    );
}
