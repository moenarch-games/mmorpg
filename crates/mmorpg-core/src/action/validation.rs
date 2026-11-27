//! Graph validation (#380, #383): the checks every [`ActionGraph`] the zone runs passes, so
//! an experimental or promoted graph can only author what the zone can apply.
use super::*;

/// A definition problem validation found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphError {
    /// An action id is authored twice.
    DuplicateAction(ActionId),
    /// An edge id is authored twice.
    DuplicateEdge(EdgeId),
    /// An edge buffers longer than its anchor-to-opening lead.
    BufferBeforeAnchor(EdgeId),
    /// A rule covers an input of its own action's region.
    SameRegion(ActionId),
    /// A holding rule covers an action that has no recovery phase (other
    /// than a swim).
    NoRecovery(RuleId),
    /// A duration is shorter than half a tick or too long for a tick timer.
    BadDuration(EdgeId),
    /// The zone has no runtime for this edge's source, input or anchor: a
    /// directed slash answered by Shield Bash, or by Warden melee after a
    /// cleave (#910), from its impact or confirmed result, and a landing
    /// answered by Warden melee, Shield Bash or another ability from the
    /// touchdown, each under its own id.
    UnsupportedEdge(EdgeId),
    /// A landing edge's window closes after the shortest landing recovery
    /// its conditions allow, when the landing it answers is over.
    OutlastsSource(EdgeId),
    /// The directed slash's outgoing edges disagree on when their shared
    /// opening is live (#1014): `SlashToShieldBash` and `CleaveToSlash` read
    /// one slash opening, whose phase, expiry, late-input grace and
    /// interruption the Shield Bash edge times, so their effective windows
    /// (baseline, modifiers and every scoped override) must open and close
    /// together, and the cleave follow-up needs the Shield Bash edge.
    DivergentSlashWindow(EdgeId),
    /// A movement rule covers a reaction or a phase its action never runs
    /// in, or its values are out of range.
    BadMovement(ActionId, Phase),
    /// Two movement rules cover the same action phase.
    DuplicateMovement(ActionId, Phase),
    /// An override names an action phase the graph authors no movement for.
    UnknownMovement(ActionId, Phase),
    /// An edge confirms a target without requiring a damaging confirmed
    /// result.
    TargetUnconfirmed(EdgeId),
    /// A precision window does not fit inside its edge's actionable interval.
    PrecisionOutsideWindow(EdgeId),
    /// A modifier names an edge the graph lacks.
    UnknownEdge(EdgeId),
    /// The edge's modifiers can reduce its actionable duration below zero.
    NegativeWindow(EdgeId),
    /// An effect is malformed (zero refund) or conflicts with another.
    BadEffect(EdgeId),
    /// An interruption's reaction is not a reaction-region action.
    NotAReaction(ActionId),
    /// An interruption names a rule the graph lacks.
    UnknownRule(RuleId),
    /// The zone cannot apply this consequence to this target: only a guard,
    /// a basic strike's wind-up and edge openings can be suspended or
    /// cancelled, a cast only cancelled, and held inputs can only be preserved
    /// or cancelled.
    UnsupportedInterruption(Interrupted),
    /// Two interruptions of the same reaction, target and phase disagree.
    ConflictingInterruption(Interrupted),
    /// The zone has no runtime for this rule: only movement and jumps
    /// cancel, and only a cast; only offense inputs are refused, and only
    /// during a swim.
    UnsupportedRule(RuleId),
    /// The zone does not read this value per class, weapon or ability: only
    /// the directed slash's and the global cooldown's recoveries, edge
    /// windows, rules and movement can be overridden.
    UnsupportedOverride(Key),
    /// An override's value is malformed, or outlasts the baseline value that
    /// canonical snapshots bound the timer by.
    BadOverride(Key),
    /// An override's scope names no class ability or main-hand catalog item.
    BadScope(Scope),
    /// Two overrides of the same value in the same scope.
    ConflictingOverride(Scope, Key),
    /// An authored action is malformed or uses a primitive that does not
    /// exist (#414).
    BadAuthored(u8, AuthoredProblem),
    /// Two authored actions share a slot.
    DuplicateAuthored(u8),
    /// Two authored actions of one class have the same trigger: the first
    /// slot, then the second.
    ConflictingTrigger(u8, u8),
    /// An edge leaves an authored action the graph lacks.
    UnknownAuthored(u8),
}

/// Whether `ms` converts to a tick duration without failing.
pub(super) fn duration_ok(ms: u32) -> bool {
    let ticks = u64::from(ms) * u64::from(crate::TICK_HZ);
    (ms == 0 || ticks + 500 >= 1_000) && (ticks + 500) / 1_000 <= u64::from(u16::MAX)
}

/// The shortest landing recovery, in ticks, for an edge under `conditions`:
/// the lengthened one when the edge requires an airborne melee action.
fn landing_ticks(conditions: &[Condition]) -> u16 {
    if conditions.contains(&Condition::AttackedAirborne) {
        modified_recovery_ticks(ActionId::Landing, Condition::AttackedAirborne)
    } else {
        recovery_ticks(ActionId::Landing)
    }
}

/// Whether movement values are in range.
pub(crate) fn validate_movement_values(movement: Movement) -> bool {
    movement.steering_percent <= 100
        && movement.momentum_percent <= 100
        && movement
            .max_acceleration_cms2
            .is_none_or(|limit| (1..=MAX_ACCELERATION_CMS2).contains(&limit))
}

/// The checks authored or overridden movement passes.
fn validate_movement(id: ActionId, phase: Phase, movement: Movement) -> Result<(), GraphError> {
    // A reaction stops the player by its own rules (a stun stands still);
    // an authored action authors its phases' movement itself.
    if action(id).region == Region::Reaction
        || matches!(id, ActionId::Authored(_))
        || !phases(id).contains(&phase)
        || !validate_movement_values(movement)
    {
        return Err(GraphError::BadMovement(id, phase));
    }
    Ok(())
}

/// Checks the shared actions and the baseline graph for consistency.
pub fn validate() -> Result<(), GraphError> {
    BASELINE.validate()
}

impl ActionGraph {
    /// The definition of `id` in this graph.
    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&'static EdgeDef> {
        self.edges.iter().find(|def| def.id == id)
    }

    /// Checks this graph and the shared actions for consistency.
    pub fn validate(&self) -> Result<(), GraphError> {
        for (index, def) in ACTIONS.iter().enumerate() {
            if ACTIONS[..index].iter().any(|other| other.id == def.id) {
                return Err(GraphError::DuplicateAction(def.id));
            }
        }
        for (index, def) in self.authored.iter().enumerate() {
            self.validate_authored(index, def)?;
        }
        for (index, def) in self.edges.iter().enumerate() {
            self.validate_edge(index, def)?;
        }
        for modifier in self.window_modifiers {
            let Some(def) = self.edge(modifier.edge) else {
                return Err(GraphError::UnknownEdge(modifier.edge));
            };
            if !duration_ok(modifier.actionable_delta_ms.unsigned_abs()) {
                return Err(GraphError::BadDuration(def.id));
            }
        }
        for def in self.edges {
            self.validate_window(def, def.window)?;
        }
        for rule in self.rules {
            if action(rule.during).region == input_region(rule.input) {
                return Err(GraphError::SameRegion(rule.during));
            }
            match rule.effect {
                RuleEffect::Hold
                    if action(rule.during).recovery_ms.is_none() && !lasting(rule.during) =>
                {
                    return Err(GraphError::NoRecovery(rule.id));
                }
                RuleEffect::Refuse
                    if rule.during != ActionId::Swim
                        || input_region(rule.input) != Region::Offense =>
                {
                    return Err(GraphError::UnsupportedRule(rule.id));
                }
                RuleEffect::Cancel
                    if rule.during != ActionId::Cast
                        || !matches!(rule.input, ActionInput::Move | ActionInput::Jump) =>
                {
                    return Err(GraphError::UnsupportedRule(rule.id));
                }
                _ => {}
            }
        }
        for (index, rule) in self.interruptions.iter().enumerate() {
            self.validate_interruption(rule)?;
            if self.interruptions[..index].iter().any(|other| {
                other.reaction == rule.reaction
                    && other.interrupted == rule.interrupted
                    && other.phase == rule.phase
                    && other.consequence != rule.consequence
            }) {
                return Err(GraphError::ConflictingInterruption(rule.interrupted));
            }
        }
        for (index, rule) in self.movement.iter().enumerate() {
            validate_movement(rule.action, rule.phase, rule.movement)?;
            if self.movement[..index]
                .iter()
                .any(|other| other.action == rule.action && other.phase == rule.phase)
            {
                return Err(GraphError::DuplicateMovement(rule.action, rule.phase));
            }
        }
        for (index, entry) in self.overrides.iter().enumerate() {
            self.validate_override(entry)?;
            let key = entry.setting.key();
            if self.overrides[..index]
                .iter()
                .any(|other| other.scope == entry.scope && other.setting.key() == key)
            {
                return Err(GraphError::ConflictingOverride(entry.scope, key));
            }
        }
        self.validate_slash_opening()
    }

    /// The slash's outgoing edges share one opening whose lifetime the
    /// Shield Bash edge times (phase, expiry, late-input grace,
    /// interruptions), so each must be live for exactly that lifetime
    /// (#1014): the same effective window in every context.
    fn validate_slash_opening(&self) -> Result<(), GraphError> {
        let Some(cleave) = self.edge(EdgeId::CleaveToSlash) else {
            return Ok(());
        };
        let Some(bash) = self.edge(EdgeId::SlashToShieldBash) else {
            return Err(GraphError::DivergentSlashWindow(cleave.id));
        };
        let timing = |window: Window| {
            let ticks = window.ticks();
            (ticks.opens, ticks.closes())
        };
        let modifiers = |id: EdgeId| {
            self.window_modifiers
                .iter()
                .filter(move |modifier| modifier.edge == id)
                .map(|modifier| (modifier.conditions, modifier.actionable_delta_ms))
        };
        let windows = |id: EdgeId| {
            self.overrides
                .iter()
                .filter_map(move |entry| match entry.setting {
                    Setting::Window { edge, window } if edge == id => {
                        Some((entry.scope, timing(window)))
                    }
                    _ => None,
                })
        };
        let same_overrides = |from: EdgeId, to: EdgeId| {
            windows(from).all(|entry| windows(to).any(|other| other == entry))
        };
        if timing(bash.window) != timing(cleave.window)
            || !modifiers(bash.id).eq(modifiers(cleave.id))
            || !same_overrides(bash.id, cleave.id)
            || !same_overrides(cleave.id, bash.id)
        {
            return Err(GraphError::DivergentSlashWindow(cleave.id));
        }
        Ok(())
    }

    fn validate_edge(&self, index: usize, def: &EdgeDef) -> Result<(), GraphError> {
        if self.edges[..index].iter().any(|other| other.id == def.id) {
            return Err(GraphError::DuplicateEdge(def.id));
        }
        let mut durations = Vec::new();
        if let Some(precision) = def.precision {
            durations.extend([precision.after_open_ms, precision.length_ms]);
        }
        let effects = def.effects.iter().chain(
            def.precision
                .iter()
                .flat_map(|precision| precision.benefits),
        );
        for effect in effects.clone() {
            match *effect {
                Effect::Recovery { ms } => durations.push(ms),
                Effect::ResourceRefund { amount: 0 } => return Err(GraphError::BadEffect(def.id)),
                _ => {}
            }
        }
        if !durations.into_iter().all(duration_ok) {
            return Err(GraphError::BadDuration(def.id));
        }
        // Two recoveries for one follow-up would be ambiguous.
        if effects
            .clone()
            .filter(|effect| matches!(effect, Effect::Recovery { .. }))
            .count()
            > 1
        {
            return Err(GraphError::BadEffect(def.id));
        }
        if let ActionId::Authored(_) = def.from {
            self.validate_authored_edge(def)?;
        }
        let supported = matches!(def.from, ActionId::Authored(_))
            || matches!(
                (def.id, def.from, def.input, def.anchor),
                (
                    EdgeId::CleaveToSlash,
                    ActionId::DirectedSlash,
                    ActionInput::WardenMelee,
                    Anchor::Impact | Anchor::ConfirmedResult,
                ) | (
                    EdgeId::SlashToShieldBash,
                    ActionId::DirectedSlash,
                    ActionInput::ShieldBash,
                    Anchor::Impact | Anchor::ConfirmedResult,
                ) | (
                    EdgeId::LeapingSlash,
                    ActionId::Landing,
                    ActionInput::WardenMelee,
                    Anchor::Landing,
                ) | (
                    EdgeId::LandingToMelee,
                    ActionId::Landing,
                    ActionInput::WardenMelee,
                    Anchor::Landing,
                ) | (
                    EdgeId::LandingToShieldBash,
                    ActionId::Landing,
                    ActionInput::ShieldBash,
                    Anchor::Landing,
                ) | (
                    EdgeId::LandingToAbility,
                    ActionId::Landing,
                    ActionInput::Ability,
                    Anchor::Landing,
                )
            );
        if !supported {
            return Err(GraphError::UnsupportedEdge(def.id));
        }
        if def.effects.contains(&Effect::TargetConfirmed)
            && !def.conditions.contains(&Condition::PositiveHealthDamage)
        {
            return Err(GraphError::TargetUnconfirmed(def.id));
        }
        // A landing has no target to confirm and no opening to keep: the
        // follow-up ends its recovery.
        if def.from == ActionId::Landing
            && effects
                .clone()
                .any(|effect| matches!(effect, Effect::TargetConfirmed | Effect::KeepOpening))
        {
            return Err(GraphError::BadEffect(def.id));
        }
        let mut impulses = 0;
        for effect in effects {
            if let Effect::Impulse {
                forward_cms,
                up_cms,
            } = *effect
            {
                impulses += 1;
                if forward_cms.unsigned_abs() > MAX_IMPULSE_FORWARD_CMS
                    || up_cms > MAX_IMPULSE_UP_CMS
                    || (forward_cms == 0 && up_cms == 0)
                {
                    return Err(GraphError::BadEffect(def.id));
                }
            }
        }
        if impulses > 1 {
            return Err(GraphError::BadEffect(def.id));
        }
        Ok(())
    }

    /// The checks an edge's window passes, authored or overridden.
    fn validate_window(&self, def: &EdgeDef, window: Window) -> Result<(), GraphError> {
        if ![window.open_after_ms, window.actionable_ms, window.buffer_ms]
            .into_iter()
            .all(duration_ok)
        {
            return Err(GraphError::BadDuration(def.id));
        }
        if window.buffer_ms > window.open_after_ms {
            return Err(GraphError::BufferBeforeAnchor(def.id));
        }
        if let Some(source) = self.source_ticks(def) {
            // Every enlargement applying at once must still end in time.
            let enlargement: u32 = self
                .window_modifiers
                .iter()
                .filter(|modifier| modifier.edge == def.id && modifier.actionable_delta_ms > 0)
                .map(|modifier| {
                    u32::from(crate::ticks_from_ms(
                        modifier.actionable_delta_ms.unsigned_abs(),
                    ))
                })
                .sum();
            if u32::from(window.ticks().closes()) + enlargement >= u32::from(source) {
                return Err(GraphError::OutlastsSource(def.id));
            }
        }
        if let Some(precision) = def.precision {
            let (_, last) = precision.ticks(window.open_after_ticks());
            if precision.length_ms == 0 || last > window.ticks().closes() {
                return Err(GraphError::PrecisionOutsideWindow(def.id));
            }
        }
        // Every reduction applying at once must leave a window.
        let reduction: u32 = self
            .window_modifiers
            .iter()
            .filter(|modifier| modifier.edge == def.id && modifier.actionable_delta_ms < 0)
            .map(|modifier| {
                u32::from(crate::ticks_from_ms(
                    modifier.actionable_delta_ms.unsigned_abs(),
                ))
            })
            .sum();
        if reduction > u32::from(window.actionable_ticks()) {
            return Err(GraphError::NegativeWindow(def.id));
        }
        Ok(())
    }

    fn validate_override(&self, entry: &Override) -> Result<(), GraphError> {
        let scoped = match entry.scope {
            Scope::Class(_) => true,
            Scope::Weapon(item) => crate::item_template(item)
                .is_some_and(|template| template.slot == Some(EquipmentSlot::MainHand)),
            Scope::Ability(id) => crate::ability::ability_by_id(id)
                .is_some_and(|ability| matches!(ability.user, AbilityUser::Class(_))),
        };
        if !scoped {
            return Err(GraphError::BadScope(entry.scope));
        }
        let key = entry.setting.key();
        match entry.setting {
            Setting::Recovery { action: id, ms } => {
                if !matches!(id, ActionId::DirectedSlash | ActionId::GlobalCooldown) {
                    return Err(GraphError::UnsupportedOverride(key));
                }
                // Canonical snapshots bound the timer by the baseline.
                if !duration_ok(ms) || Some(ms) > action(id).recovery_ms {
                    return Err(GraphError::BadOverride(key));
                }
            }
            Setting::Window { edge, window } => {
                let Some(def) = self.edge(edge) else {
                    return Err(GraphError::UnknownEdge(edge));
                };
                self.validate_window(def, window)?;
                // Canonical snapshots bound an opening's age by the baseline.
                let bound = EDGES
                    .iter()
                    .find(|baseline| baseline.id == edge)
                    .map_or(def.window, |baseline| baseline.window);
                if window.ticks().closes() > bound.ticks().closes() {
                    return Err(GraphError::BadOverride(key));
                }
            }
            Setting::Rule { rule, .. } => {
                if !self.rules.iter().any(|other| other.id == rule) {
                    return Err(GraphError::UnknownRule(rule));
                }
            }
            Setting::Movement {
                action: id,
                phase,
                movement,
            } => {
                if self.movement_rule(id, phase).is_none() {
                    return Err(GraphError::UnknownMovement(id, phase));
                }
                validate_movement(id, phase, movement)?;
            }
        }
        Ok(())
    }

    /// The shortest run, in ticks from its anchor, of an edge's source that
    /// ends by itself: a landing's recovery, an authored action after its
    /// use. An edge out of one must close before it ends.
    fn source_ticks(&self, def: &EdgeDef) -> Option<u16> {
        match def.from {
            ActionId::Landing => Some(landing_ticks(def.conditions)),
            ActionId::Authored(slot) => self
                .authored_action(slot)
                .map(AuthoredAction::after_impact_ticks),
            _ => None,
        }
    }

    /// The movement rule of `id` in `phase`.
    #[must_use]
    pub fn movement_rule(&self, id: ActionId, phase: Phase) -> Option<&'static MovementRule> {
        self.movement
            .iter()
            .find(|rule| rule.action == id && rule.phase == phase)
    }

    fn validate_interruption(&self, rule: &InterruptionRule) -> Result<(), GraphError> {
        if action(rule.reaction).region != Region::Reaction {
            return Err(GraphError::NotAReaction(rule.reaction));
        }
        let supported = match rule.interrupted {
            Interrupted::Action(ActionId::Guard) => true,
            // A stun or death always ends an authored action.
            Interrupted::Action(ActionId::Authored(_)) => false,
            // Death always ends a cast; a cast cannot pause.
            Interrupted::Action(ActionId::Cast) => match rule.consequence {
                Consequence::Cancel => true,
                Consequence::Preserve => rule.reaction != ActionId::Death,
                Consequence::Suspend => false,
            },
            // Death always ends a basic strike; another reaction may cancel or pause only its
            // wind-up, since its recovery is the swing timer every press waits out (#1221).
            Interrupted::Action(ActionId::BasicMelee) => match rule.consequence {
                Consequence::Cancel => {
                    rule.reaction == ActionId::Death || rule.phase == Some(Phase::Startup)
                }
                Consequence::Suspend => {
                    rule.reaction != ActionId::Death && rule.phase == Some(Phase::Startup)
                }
                Consequence::Preserve => rule.reaction != ActionId::Death,
            },
            Interrupted::Action(_) => rule.consequence == Consequence::Preserve,
            Interrupted::Edge(id) => {
                let Some(def) = self.edge(id) else {
                    return Err(GraphError::UnknownEdge(id));
                };
                // A landing's recovery and an authored action run on; their
                // edges cannot pause.
                !matches!(def.from, ActionId::Landing | ActionId::Authored(_))
                    || rule.consequence != Consequence::Suspend
            }
            Interrupted::Held(id) => {
                if !self.rules.iter().any(|other| other.id == id) {
                    return Err(GraphError::UnknownRule(id));
                }
                rule.consequence != Consequence::Suspend
                    && self
                        .rules
                        .iter()
                        .any(|other| other.id == id && other.effect == RuleEffect::Hold)
            }
        };
        if supported {
            Ok(())
        } else {
            Err(GraphError::UnsupportedInterruption(rule.interrupted))
        }
    }
}
