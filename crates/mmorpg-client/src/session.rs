//! Owns network progress, bounded resume, input delivery and shutdown for a window.
use crate::{
    ClientError,
    network::{ClientSession, SessionError},
};
use mmorpg_client_model::outbox::{
    MovementCommands, MovementInput, MovementOutbox, RESEND_INTERVAL_MS,
};
use mmorpg_core::{EntityRef, ZoneCommand, ZoneSnapshot};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

const RESEND_INTERVAL: Duration = Duration::from_millis(RESEND_INTERVAL_MS as u64);

/// Latest local input published by the window: held movement plus counted
/// discrete presses. Each counter (`jumps`, `rolls`, `selections`, `swings`,
/// `releases`, `ability_uses`, `ability_releases`, `cancels`) makes the session send one command
/// when it advances, so coalesced watch updates can merge presses but never
/// replay them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerInput {
    pub forward: i8,
    pub strafe: i8,
    pub facing: u16,
    pub jumps: u32,
    /// Dodge-roll presses (#516).
    pub rolls: u32,
    /// The latest requested selection (`None` clears it).
    pub target: Option<EntityRef>,
    pub selections: u32,
    /// Weapon swings (#517); each goes toward the latest `facing`.
    pub swings: u32,
    pub releases: u32,
    /// The latest requested ability ID.
    pub ability: u8,
    pub ability_uses: u32,
    /// The latest released ability ID and whether a retirement let it go (#485).
    pub ability_release: (u8, bool),
    pub ability_releases: u32,
    pub cancels: u32,
    /// Latest intentional NPC action, delivered once and retired on reconnect.
    pub interaction: Option<ZoneCommand>,
    pub interactions: u32,
    /// The class and sex wire values to choose. The session resends the
    /// choice under fresh sequences until a projection confirms a class, so
    /// a lost datagram cannot leave the character classless.
    pub class_choice: Option<(u8, u8)>,
    /// The held shield guard lane (#156); `None` once released. Held like
    /// movement: the session sends each change and resends it after a reconnect.
    pub guard: Option<mmorpg_core::StrikeHeight>,
    /// Held swim ascent (`1`), descent (`-1`) or neither (#274). Held like the guard: the
    /// session sends each change and resends a held one after a reconnect.
    pub rise: i8,
}

/// Latest received refusal, retained across coalesced GUI updates in one epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefusalReceipt {
    pub tick: u64,
    pub acknowledged_sequence: u32,
    pub code: mmorpg_core::ErrorCode,
    pub target: Option<EntityRef>,
}

#[derive(Clone)]
pub enum NetworkUpdate {
    Waiting,
    Reconnecting,
    /// The character moves on to another zone (#145); its snapshots follow from there.
    Travelling(mmorpg_core::ZoneId),
    // Epoch accompanies every snapshot: watch coalescing may skip intermediate
    // status updates, but must never skip the presentation reset on resume.
    Snapshot {
        connection_epoch: u32,
        snapshot: Arc<ZoneSnapshot>,
        refusal: Option<RefusalReceipt>,
    },
    Failed(String),
}

/// Commands to send for one input observation, in order: a selection, a
/// weapon swing, a spirit release, a cast cancellation, an ability, its
/// release, an NPC interaction, then the movement commands (a jump, the swim
/// rise, a move, then a roll).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Outgoing {
    choose: Option<(u8, u8)>,
    select: Option<Option<EntityRef>>,
    swing: Option<u16>,
    release: bool,
    cancel: bool,
    ability: Option<u8>,
    ability_release: Option<(u8, bool)>,
    interaction: Option<ZoneCommand>,
    guard: Option<Option<mmorpg_core::StrikeHeight>>,
    movement: MovementCommands,
}

impl Outgoing {
    fn deliver(self, session: &mut ClientSession) -> Result<(), SessionError> {
        if let Some((class, sex)) = self.choose {
            session.send_choose_class(class, sex)?;
        }
        if let Some(target) = self.select {
            session.send_select_target(target)?;
        }
        if let Some(facing) = self.swing {
            session.send_swing(facing)?;
        }
        if self.release {
            session.send_release_spirit()?;
        }
        if self.cancel {
            session.send_cancel_cast()?;
        }
        if let Some(lane) = self.guard {
            session.send_command(ZoneCommand::Guard { lane })?;
        }
        if let Some(ability) = self.ability {
            session.send_use_ability(ability)?;
        }
        if let Some((ability, retired)) = self.ability_release {
            session.send_command(ZoneCommand::ReleaseAbility { ability, retired })?;
        }
        if let Some(command) = self.interaction {
            session.send_command(command)?;
        }
        for command in self.movement.commands() {
            session.send_command(command)?;
        }
        Ok(())
    }
}

/// Press counters already handled; a counter that moves on means one more
/// command, however many presses it merged. Jumps and rolls are the movement
/// outbox's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Presses {
    selections: u32,
    swings: u32,
    releases: u32,
    ability_uses: u32,
    ability_releases: u32,
    cancels: u32,
    interactions: u32,
}

impl Presses {
    const fn of(input: PlayerInput) -> Self {
        Self {
            selections: input.selections,
            swings: input.swings,
            releases: input.releases,
            ability_uses: input.ability_uses,
            ability_releases: input.ability_releases,
            cancels: input.cancels,
            interactions: input.interactions,
        }
    }
}

const fn movement_input(input: PlayerInput) -> MovementInput {
    MovementInput {
        forward: input.forward,
        strafe: input.strafe,
        facing: input.facing,
        jumps: input.jumps,
        rolls: input.rolls,
        rise: input.rise,
    }
}

/// Decides what to send and tracks what was last sent. Movement follows the
/// shared [`MovementOutbox`] (#543), driven by a heartbeat timer as well as
/// input changes; the other presses go out once each. It performs no I/O; a
/// failed delivery ends in [`Outbox::resume`] or ends the session.
struct Outbox {
    movement: MovementOutbox,
    /// The clock the movement outbox counts milliseconds on.
    origin: Instant,
    presses: Presses,
    /// A projection showed the character with a class.
    class_confirmed: bool,
    /// The latest projection showed the character dead.
    dead: bool,
    /// The guard lane last sent; `None` before any or after a resume.
    sent_guard: Option<Option<mmorpg_core::StrikeHeight>>,
}

impl Outbox {
    fn new(input: PlayerInput, now: Instant) -> Self {
        Self {
            movement: MovementOutbox::new(movement_input(input)),
            origin: now,
            presses: Presses::of(input),
            class_confirmed: false,
            dead: false,
            sent_guard: Some(None),
        }
    }

    fn ms(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.origin).as_secs_f64() * 1_000.0
    }

    /// Reads a received projection: once it shows a class, the choice is
    /// never sent again, also after a reconnect (the zone keeps it).
    fn observe(&mut self, snapshot: &ZoneSnapshot) {
        self.class_confirmed |= snapshot.viewer.class.is_some();
        self.dead = snapshot.viewer.dead;
    }

    /// The guard lane to send when it differs from the last one sent.
    /// After a resume only a held lane is resent; a released guard was already lowered.
    fn guard(&mut self, input: PlayerInput) -> Option<Option<mmorpg_core::StrikeHeight>> {
        let Some(sent) = self.sent_guard else {
            self.sent_guard = Some(input.guard);
            return input.guard.map(Some);
        };
        (sent != input.guard).then(|| {
            self.sent_guard = Some(input.guard);
            input.guard
        })
    }

    /// Commands for a newly observed input: one command per advanced press
    /// counter, however many presses it merged, then the movement commands.
    fn changes(&mut self, input: PlayerInput, now: Instant) -> Outgoing {
        let handled = self.presses;
        let pressed = Presses::of(input);
        self.presses = pressed;
        // A swing or ability fires where the character faces (#517): it sends the facing now.
        let aimed =
            pressed.swings != handled.swings || pressed.ability_uses != handled.ability_uses;
        let movement = self
            .movement
            .update(movement_input(input), self.ms(now), self.dead, aimed);
        Outgoing {
            // The heartbeat alone retries the class choice.
            choose: None,
            select: (pressed.selections != handled.selections).then_some(input.target),
            swing: (pressed.swings != handled.swings).then_some(input.facing),
            release: pressed.releases != handled.releases,
            cancel: pressed.cancels != handled.cancels,
            ability: (pressed.ability_uses != handled.ability_uses).then_some(input.ability),
            ability_release: (pressed.ability_releases != handled.ability_releases)
                .then_some(input.ability_release),
            interaction: (pressed.interactions != handled.interactions)
                .then_some(input.interaction)
                .flatten(),
            guard: self.guard(input),
            movement,
        }
    }

    /// Periodic resend of the current intent and of an unconfirmed class
    /// choice; pending presses are left to [`Outbox::changes`], so none is
    /// ever sent twice.
    fn heartbeat(&mut self, input: PlayerInput, now: Instant) -> Outgoing {
        Outgoing {
            choose: input.class_choice.filter(|_| !self.class_confirmed),
            guard: self.guard(input),
            movement: self
                .movement
                .heartbeat(movement_input(input), self.ms(now), self.dead),
            ..Outgoing::default()
        }
    }

    /// After resume only current input counts; presses during the outage are dropped.
    fn resume(&mut self, input: PlayerInput) {
        self.presses = Presses::of(input);
        self.movement.resume(movement_input(input));
        // The next heartbeat re-raises a held guard on the resumed connection.
        self.sent_guard = None;
    }
}

/// A roster choice, separate from high-frequency held input.
#[derive(Clone, Debug)]
pub struct CharacterSelection {
    pub local_id: String,
    pub class: mmorpg_core::ClassChoice,
    pub global_id: Option<mmorpg_core::CharacterId>,
}

pub async fn run_session_with_characters(
    mut session: ClientSession,
    input: watch::Receiver<PlayerInput>,
    mut characters: watch::Receiver<Option<CharacterSelection>>,
    updates: &watch::Sender<NetworkUpdate>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), ClientError> {
    let enabled = tokio::select! {
        _ = &mut shutdown => return Ok(()),
        enabled = session.character_service_enabled() => enabled?,
    };
    if !enabled {
        return run_session(session, input, updates, shutdown).await;
    }
    let selected = loop {
        if let Some(selected) = characters.borrow_and_update().clone() {
            break selected;
        }
        tokio::select! {
            _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
            changed = characters.changed() => { changed.map_err(|_| "character selection closed")?; }
        }
    };
    let id = match selected.global_id {
        Some(id) => id,
        None => {
            let directory =
                crate::roster::config_dir().ok_or("no character configuration directory")?;
            tokio::task::spawn_blocking(move || {
                crate::online_identity::for_character(&directory, &selected.local_id)
            })
            .await??
        }
    };
    tokio::select! {
        _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
        claimed = session.claim_character(&id, selected.class) => { claimed?; }
    }
    run_session(session, input, updates, shutdown).await
}

pub async fn run_session(
    mut session: ClientSession,
    mut input: watch::Receiver<PlayerInput>,
    updates: &watch::Sender<NetworkUpdate>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), ClientError> {
    let mut heartbeat = tokio::time::interval(RESEND_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut outbox = Outbox::new(*input.borrow_and_update(), Instant::now());
    outbox.class_confirmed = session.has_durable_character();
    let mut last_snapshot = Instant::now();
    let mut last_tick = None;
    loop {
        let failure = tokio::select! {
            biased;
            _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
            _ = heartbeat.tick() => {
                if last_snapshot.elapsed() > Duration::from_secs(5) {
                    Some(SessionError::SnapshotTimeout)
                } else {
                    // Periodic resend corrects a lost datagram, including a key
                    // release. Peek without marking the value seen, so a pending
                    // jump still reaches the change branch.
                    let current = *input.borrow();
                    outbox.heartbeat(current, Instant::now()).deliver(&mut session).err()
                }
            },
            changed = input.changed() => match changed {
                Ok(()) => {
                    let current = *input.borrow_and_update();
                    outbox.changes(current, Instant::now()).deliver(&mut session).err()
                }
                // The owning window has closed; it also requests shutdown.
                Err(_) => { session.release_character().await?; return Ok(()); },
            },
            snapshot = session.receive_snapshot() => match snapshot {
                Ok(snapshot) => {
                    if last_tick.is_none_or(|tick| snapshot.tick > tick) {
                        outbox.observe(&snapshot);
                        last_tick = Some(snapshot.tick);
                        last_snapshot = Instant::now();
                        publish(updates, &session, snapshot);
                    }
                    None
                }
                Err(error) => Some(error),
            },
        };
        if let Some(SessionError::Redirected(notice)) = &failure {
            updates.send_replace(NetworkUpdate::Travelling(notice.destination));
            tokio::select! {
                biased;
                _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
                followed = session.follow(notice) => followed?,
            }
            // Another zone counts its own ticks; only current input carries over.
            last_tick = None;
            last_snapshot = Instant::now();
            outbox.resume(*input.borrow_and_update());
            heartbeat.reset();
        } else if let Some(error) = failure {
            let recoverable = tokio::select! {
                biased;
                _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
                recoverable = session.can_reconnect(&error) => recoverable,
            };
            if !recoverable {
                return Err(error.into());
            }
            updates.send_replace(NetworkUpdate::Reconnecting);
            let snapshot = tokio::select! {
                biased;
                _ = &mut shutdown => { session.release_character().await?; return Ok(()); },
                snapshot = session.reconnect() => snapshot?,
            };
            outbox.observe(&snapshot);
            last_tick = Some(snapshot.tick);
            last_snapshot = Instant::now();
            publish(updates, &session, snapshot);
            // Read only the current input after recovery; no buffered commands.
            outbox.resume(*input.borrow_and_update());
            heartbeat.reset();
        }
    }
}

fn publish(
    updates: &watch::Sender<NetworkUpdate>,
    session: &ClientSession,
    snapshot: ZoneSnapshot,
) {
    let connection_epoch = session.connection_epoch();
    let refusal = snapshot
        .events
        .iter()
        .rev()
        .find_map(|event| {
            if let mmorpg_core::ZoneEvent::Error { code, target } = event {
                Some(RefusalReceipt {
                    tick: snapshot.tick,
                    acknowledged_sequence: snapshot.acknowledged_sequence,
                    code: *code,
                    target: *target,
                })
            } else {
                None
            }
        })
        .or_else(|| match &*updates.borrow() {
            NetworkUpdate::Snapshot {
                connection_epoch: previous,
                refusal,
                ..
            } if *previous == connection_epoch => *refusal,
            _ => None,
        });
    updates.send_replace(NetworkUpdate::Snapshot {
        connection_epoch,
        snapshot: Arc::new(snapshot),
        refusal,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_client_model::outbox::FACING_INTERVAL_MS;

    const TICK: Duration = Duration::from_nanos((FACING_INTERVAL_MS * 1e6) as u64);

    fn only_move(forward: i8, strafe: i8, facing: u16) -> Outgoing {
        Outgoing {
            movement: MovementCommands {
                movement: Some((forward, strafe, facing)),
                ..MovementCommands::default()
            },
            ..Outgoing::default()
        }
    }

    /// An outbox that has already sent `input` at `now`.
    fn sent(input: PlayerInput, now: Instant) -> Outbox {
        let mut outbox = Outbox::new(input, now);
        assert_eq!(
            outbox.changes(input, now),
            only_move(input.forward, input.strafe, input.facing),
            "the first observation sends the current intent without a jump"
        );
        outbox
    }

    fn snapshot() -> ZoneSnapshot {
        ZoneSnapshot {
            doors: Vec::new(),
            projectiles: Vec::new(),
            debris: Vec::new(),
            pieces: Vec::new(),
            raid: None,
            content_revision: 6,
            acknowledged_sequence: 1,
            viewer_id: 1,
            zone_id: mmorpg_core::ZoneId::new(1),
            tick: 1,
            viewer: mmorpg_core::ViewerState::default(),
            cooldowns: Vec::new(),
            auras: Vec::new(),
            target_of_target: None,
            target_detail: mmorpg_core::TargetDetail::default(),
            inventory_revision: 1,
            inventory: None,
            equipment: None,
            loot: None,
            events: Vec::new(),
            chat: Vec::new(),
            entities: Vec::new(),
            quests: None,
        }
    }

    #[test]
    fn a_swing_or_ability_sends_a_changed_facing_at_once() {
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let turned = PlayerInput {
            facing: 900,
            ..PlayerInput::default()
        };
        assert_eq!(outbox.changes(turned, now), Outgoing::default());
        let swung = PlayerInput {
            swings: 1,
            ..turned
        };
        assert_eq!(
            outbox.changes(swung, now),
            Outgoing {
                swing: Some(900),
                ..only_move(0, 0, 900)
            }
        );
    }

    #[test]
    fn the_dead_hold_still_and_their_jumps_are_dropped() {
        let now = Instant::now();
        let running = PlayerInput {
            forward: 1,
            ..PlayerInput::default()
        };
        let mut outbox = sent(running, now);
        let mut dead = snapshot();
        dead.viewer.dead = true;
        outbox.observe(&dead);
        let pressed = PlayerInput {
            jumps: 1,
            rolls: 1,
            ..running
        };
        assert_eq!(outbox.changes(pressed, now), only_move(0, 0, 0));
        assert_eq!(outbox.heartbeat(pressed, now + TICK), only_move(0, 0, 0));
        outbox.observe(&snapshot());
        assert_eq!(outbox.changes(pressed, now + TICK), only_move(1, 0, 0));
    }

    #[test]
    fn the_class_choice_repeats_with_every_heartbeat_until_a_projection_confirms_it() {
        use mmorpg_core::{ClassChoice, PlayerClass, Sex};
        let now = Instant::now();
        let input = PlayerInput {
            class_choice: Some((2, 0)),
            ..PlayerInput::default()
        };
        let mut outbox = Outbox::new(input, now);
        let expected = Outgoing {
            choose: Some((2, 0)),
            ..only_move(0, 0, 0)
        };
        // A lost datagram is retried: each heartbeat sends it again, and
        // input changes never carry it.
        assert_eq!(outbox.heartbeat(input, now), expected);
        assert_eq!(outbox.heartbeat(input, now + TICK), expected);
        assert_eq!(
            outbox
                .changes(PlayerInput { jumps: 1, ..input }, now)
                .choose,
            None
        );
        let mut snapshot = snapshot();
        outbox.observe(&snapshot);
        assert_eq!(
            outbox.heartbeat(input, now + 2 * TICK),
            expected,
            "still classless"
        );
        snapshot.viewer.class = Some(ClassChoice {
            class: PlayerClass::Arcanist,
            sex: Sex::Female,
        });
        outbox.observe(&snapshot);
        assert_eq!(outbox.heartbeat(input, now + 3 * TICK), only_move(0, 0, 0));
        // A reconnect keeps the confirmation: the zone keeps the class.
        outbox.resume(input);
        assert_eq!(outbox.heartbeat(input, now + 4 * TICK), only_move(0, 0, 0));
    }

    #[test]
    fn ability_uses_and_cast_cancels_are_sent_once_per_press() {
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let input = PlayerInput {
            ability: 9,
            ability_uses: 3,
            cancels: 1,
            ..PlayerInput::default()
        };
        assert_eq!(
            outbox.changes(input, now),
            Outgoing {
                cancel: true,
                ability: Some(9),
                ..Outgoing::default()
            },
            "merged presses send the latest ability once"
        );
        assert_eq!(outbox.changes(input, now), Outgoing::default());
        assert_eq!(outbox.heartbeat(input, now + TICK), only_move(0, 0, 0));
    }

    #[test]
    fn an_ability_release_follows_its_use_once_per_release() {
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let tapped = PlayerInput {
            ability: 9,
            ability_uses: 1,
            ability_release: (9, false),
            ability_releases: 1,
            ..PlayerInput::default()
        };
        assert_eq!(
            outbox.changes(tapped, now),
            Outgoing {
                ability: Some(9),
                ability_release: Some((9, false)),
                ..Outgoing::default()
            },
            "a press and release in one observation send the use, then the release"
        );
        assert_eq!(outbox.changes(tapped, now), Outgoing::default());
        assert_eq!(outbox.heartbeat(tapped, now + TICK), only_move(0, 0, 0));
        let retired = PlayerInput {
            ability_release: (9, true),
            ability_releases: 2,
            ..tapped
        };
        assert_eq!(
            outbox.changes(retired, now).ability_release,
            Some((9, true))
        );
    }

    #[test]
    fn a_held_guard_lane_is_sent_on_change_and_again_after_a_resume() {
        use mmorpg_core::StrikeHeight;
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let high = PlayerInput {
            guard: Some(StrikeHeight::High),
            ..PlayerInput::default()
        };
        assert_eq!(
            outbox.changes(high, now),
            Outgoing {
                guard: Some(Some(StrikeHeight::High)),
                ..Outgoing::default()
            }
        );
        assert_eq!(outbox.changes(high, now), Outgoing::default());
        assert_eq!(outbox.heartbeat(high, now + TICK), only_move(0, 0, 0));
        // A reconnect re-raises the held lane with the next heartbeat.
        outbox.resume(high);
        assert_eq!(
            outbox.heartbeat(high, now + 2 * TICK).guard,
            Some(Some(StrikeHeight::High))
        );
        // Releasing lowers it once.
        let released = PlayerInput::default();
        assert_eq!(outbox.changes(released, now + 3 * TICK).guard, Some(None));
        assert_eq!(outbox.changes(released, now + 3 * TICK).guard, None);
    }

    #[test]
    fn selections_swings_and_releases_are_sent_once_per_press_in_order() {
        use mmorpg_core::CreatureId;
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let wolf = Some(EntityRef::Creature(CreatureId::new(108)));
        let input = PlayerInput {
            target: wolf,
            selections: 2,
            swings: 1,
            ..PlayerInput::default()
        };
        assert_eq!(
            outbox.changes(input, now),
            Outgoing {
                select: Some(wolf),
                swing: Some(0),
                ..Outgoing::default()
            },
            "merged selections send the latest target once, then the swing"
        );
        assert_eq!(outbox.changes(input, now), Outgoing::default());
        assert_eq!(
            outbox.heartbeat(input, now + TICK),
            only_move(0, 0, 0),
            "the heartbeat never repeats a press"
        );
        let released = PlayerInput {
            releases: 1,
            ..input
        };
        assert_eq!(
            outbox.changes(released, now),
            Outgoing {
                release: true,
                ..Outgoing::default()
            }
        );
        let outage = PlayerInput {
            selections: 9,
            swings: 9,
            releases: 9,
            jumps: 9,
            ..released
        };
        outbox.resume(outage);
        assert_eq!(
            outbox.changes(outage, now),
            only_move(0, 0, 0),
            "presses during an outage are dropped on resume"
        );
    }
}

// Independent received-feedback boundary acceptance; no runtime behavior added.
#[cfg(test)]
#[path = "../tests/support/session_receipt_acceptance.rs"]
mod session_receipt_acceptance;
