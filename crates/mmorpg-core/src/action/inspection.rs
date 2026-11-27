//! The read-only inspection the Action Lab reads (#381): every region's action and phase,
//! each edge's window, the rules and interruptions in effect, effective values and movement,
//! all from the same runtime state the zone runs on.
use super::*;

/// One region's running action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionReport {
    pub region: Region,
    pub action: Option<ActionId>,
    pub phase: Option<Phase>,
    /// Ticks of the committed recovery left.
    pub recovery_left: Option<u16>,
    /// The ability the action runs: a cast's, or Heroic Strike for the
    /// directed slash.
    pub ability: Option<AbilityId>,
    /// Ticks of a cast or channel left.
    pub active_left: Option<u16>,
    /// A basic strike's variant: the pending one, else the one that struck (#1221).
    pub strike: Option<crate::StrikeVariant>,
    /// Ticks left in which a basic press follows the strike up: the hit's follow-up (#928)
    /// during its recovery, the chain's window after it.
    pub follow_up_left: Option<u16>,
}

/// One edge's state, in absolute ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeReport {
    pub edge: EdgeId,
    pub priority: u8,
    pub status: EdgeStatus,
    /// The tick of the authoritative anchor.
    pub anchor_tick: Option<u64>,
    /// The first tick an input is buffered.
    pub buffer_from: Option<u64>,
    /// The first tick the edge resolves an input.
    pub open_from: Option<u64>,
    /// The last tick the edge resolves an input, after window modifiers.
    pub open_until: Option<u64>,
    /// The first and last tick of the precision window.
    pub precision: Option<(u64, u64)>,
    /// Where the edge's window comes from.
    pub origin: Origin,
}

/// One compatibility rule's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleReport {
    pub rule: RuleId,
    pub input: ActionInput,
    pub effect: RuleEffect,
    /// The first tick the rule no longer applies to the input: the held
    /// input is released, or the cast it would cancel has ended. `None`
    /// while a swim holds or refuses it: until the swimmer is ashore.
    pub released_at: Option<u64>,
    /// Why the rule applies to this player.
    pub origin: Origin,
}

/// How the reactions in effect treat one running target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptionReport {
    pub interrupted: Interrupted,
    /// The reaction that decides the consequence.
    pub reaction: ActionId,
    pub consequence: Consequence,
    /// An authored rule decided it; otherwise the default preserves.
    pub authored: bool,
}

/// A read-only view of one player's action graph (#380, #381).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInspection {
    /// The last completed tick.
    pub tick: u64,
    pub regions: [RegionReport; 4],
    pub edges: Vec<EdgeReport>,
    /// The rules that currently apply.
    pub rules: Vec<RuleReport>,
    /// What the reactions in effect do to each running action, opening and
    /// buffer.
    pub interruptions: Vec<InterruptionReport>,
    /// Every overridable value as it applies to this player now, with its
    /// origin: the directed slash's and the global cooldown's recoveries,
    /// each edge's window and whether each rule applies. An ability override
    /// of the global cooldown shows while that ability casts; it applies
    /// whenever that ability starts the cooldown. Each authored movement
    /// rule's value is listed too.
    pub effective: Vec<Effective>,
    /// The movement the next tick's controller velocity follows.
    pub movement: MovementReport,
    /// Each authored action, and whether its trigger would start it now
    /// (#414).
    pub authored: Vec<AuthoredReport>,
}

impl ZoneSimulation {
    /// The player's action graph after the last completed tick.
    #[must_use]
    pub fn action_inspection(&self, player_id: PlayerId) -> Option<ActionInspection> {
        let state = self.players.get(&player_id)?;
        let graph = self.action_graph;
        let tick = self.tick;
        // Intents of the next tick resolve before its timers advance.
        let next = tick + 1;
        let edges = graph
            .edges
            .iter()
            .map(|def| {
                let eval = evaluate(graph, state, def, 0);
                // The source resolved during the intents of `anchor`; ticks
                // since then are counted at the end of each tick. A
                // suspended clock reports the ticks it would resume with.
                let anchor = eval
                    .elapsed
                    .map(|elapsed| next.saturating_sub(u64::from(elapsed)));
                let window = eval.window;
                let at = |offset: u16| anchor.map(|at| at + u64::from(offset));
                EdgeReport {
                    edge: def.id,
                    priority: def.priority,
                    status: eval.status,
                    anchor_tick: anchor,
                    buffer_from: at(window.opens - window.buffer),
                    open_from: at(window.opens),
                    open_until: at(window.closes()),
                    precision: def.precision.and_then(|precision| {
                        let (from, until) = precision.ticks(window.opens);
                        Some((at(from)?, at(until.min(window.closes()))?))
                    }),
                    origin: window_for(graph, state, def).1,
                }
            })
            .collect();
        let rules = graph
            .rules
            .iter()
            .filter_map(|rule| {
                // Recovery counts down after the next tick's intents and
                // combat resolve; movement cancels a cast before it advances.
                let left = match (rule.effect, rule.during) {
                    (_, id) if lasting(id) => running(state, id).then_some(None)?,
                    (RuleEffect::Hold, _) => Some(recovery_left(state, rule.during)?),
                    (RuleEffect::Cancel, _) => Some(
                        cast_left(state)
                            .filter(|_| rule.during == ActionId::Cast)?
                            .1,
                    ),
                    (RuleEffect::Refuse, _) => return None,
                };
                applies(graph, state, rule).then(|| RuleReport {
                    rule: rule.id,
                    input: rule.input,
                    effect: rule.effect,
                    released_at: left.map(|left| next + u64::from(left)),
                    origin: rule_applies(graph, state, rule).origin,
                })
            })
            .collect();
        let targets = ACTIONS
            .iter()
            .filter(|def| def.region != Region::Reaction)
            .map(|def| Interrupted::Action(def.id))
            .chain(graph.edges.iter().map(|def| Interrupted::Edge(def.id)))
            .chain(graph.rules.iter().map(|rule| Interrupted::Held(rule.id)));
        let mut interruptions: Vec<InterruptionReport> = Vec::new();
        for target in targets {
            if interruptions
                .iter()
                .any(|known| known.interrupted == target)
            {
                continue;
            }
            if let Some((consequence, reaction, rule)) = interruption(graph, state, target) {
                interruptions.push(InterruptionReport {
                    interrupted: target,
                    reaction,
                    consequence,
                    authored: rule.is_some(),
                });
            }
        }
        let sources = movement_sources(graph, state);
        Some(ActionInspection {
            tick,
            regions: region_actions(graph, state),
            edges,
            rules,
            interruptions,
            effective: effective_values(graph, state),
            movement: MovementReport {
                movement: sources
                    .iter()
                    .map(|source| source.movement)
                    .reduce(Movement::combine)
                    .unwrap_or(Movement::FREE),
                sources,
            },
            authored: authored::reports(graph, state),
        })
    }

    /// Explains how `input` would be arbitrated at the player's next tick.
    #[must_use]
    pub fn arbitrate_input(&self, player_id: PlayerId, input: ActionInput) -> Option<Arbitration> {
        self.arbitrate_late_input(player_id, input, 0)
    }

    /// [`Self::arbitrate_input`] for an input `late` ticks late; at most
    /// [`LATE_INPUT_GRACE_TICKS`] count.
    #[must_use]
    pub fn arbitrate_late_input(
        &self,
        player_id: PlayerId,
        input: ActionInput,
        late: u8,
    ) -> Option<Arbitration> {
        self.players
            .get(&player_id)
            .map(|state| arbitrate(self.action_graph, state, input, late))
    }
}

/// See [`ActionInspection::effective`].
fn effective_values(graph: &ActionGraph, state: &PlayerState) -> Vec<Effective> {
    let casting = state.cast.map(|cast| cast.ability);
    let mut keys = vec![
        (
            Key::Recovery(ActionId::DirectedSlash),
            Some(ids::HEROIC_STRIKE),
        ),
        (Key::Recovery(ActionId::GlobalCooldown), casting),
    ];
    keys.extend(
        graph
            .edges
            .iter()
            .map(|def| (Key::Window(def.id), action_ability(state, def.from))),
    );
    for rule in graph.rules {
        if !keys.iter().any(|(key, _)| *key == Key::Rule(rule.id)) {
            keys.push((Key::Rule(rule.id), action_ability(state, rule.during)));
        }
    }
    keys.extend(graph.movement.iter().map(|rule| {
        (
            Key::Movement(rule.action, rule.phase),
            action_ability(state, rule.action),
        )
    }));
    let mut values: Vec<Effective> = keys
        .into_iter()
        .filter_map(|(key, ability)| graph.resolve(key, Context::of(state, ability)))
        .collect();
    // Each authored action's phases move as it authors them (#414).
    for def in graph.authored {
        for phase in [Phase::Startup, Phase::Active, Phase::Recovery] {
            values.push(Effective {
                setting: Setting::Movement {
                    action: ActionId::Authored(def.slot),
                    phase,
                    movement: def.movement(phase),
                },
                origin: Origin::Authored(def.slot),
            });
        }
    }
    values
}
