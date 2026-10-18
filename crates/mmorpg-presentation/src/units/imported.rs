//! Which imported model presents a unit, its clip names and the per-unit clip clock.
//!
//! Players use asset-tooling's Ranger humanoid (Atlas 6.3.7); wolves, vermin and boars use the
//! bundled Quaternius CC0 animals (Atlas 6.3.6); humanoid creatures and NPCs keep their
//! procedural models. Each client imports and skins the models with its own provider; this
//! module only decides what to pose. Presentation only, never read by gameplay.
use mmorpg_core::{CreatureFamily, CreatureTemplateId, EntityRef, EntitySnapshot, ZoneContent};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Ranger,
    Wolf,
    Rat,
    Pig,
}

/// Clip names of one model.
#[derive(Clone, Copy, Debug)]
pub struct Clips {
    pub idle: &'static str,
    pub walk: &'static str,
    pub run: &'static str,
    pub attack: &'static str,
    pub death: &'static str,
    /// A standing raised shield, for a model that carries one (#156).
    pub guard: Option<&'static str>,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Ranger, Self::Wolf, Self::Rat, Self::Pig];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ranger => "Ranger",
            Self::Wolf => "Wolf",
            Self::Rat => "Rat",
            Self::Pig => "Pig",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    #[must_use]
    pub const fn clips(self) -> Clips {
        match self {
            Self::Ranger => Clips {
                idle: "Idle_Loop",
                walk: "Walk_Loop",
                run: "Jog_Fwd_Loop",
                attack: "Sword_Attack",
                death: "Death01",
                guard: Some("Idle_Shield_Loop"),
            },
            Self::Wolf => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Gallop",
                attack: "Attack",
                death: "Death",
                guard: None,
            },
            Self::Rat => Clips {
                idle: "Rat_Idle",
                walk: "Rat_Walk",
                run: "Rat_Run",
                attack: "Rat_Attack",
                death: "Rat_Death",
                guard: None,
            },
            // The farm pig has no attack clip; its jump reads as a charge.
            Self::Pig => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Run",
                attack: "Jump",
                death: "Death",
                guard: None,
            },
        }
    }

    /// The model presenting `entity`, if it has one.
    #[must_use]
    pub fn of(entity: EntityRef, record: &EntitySnapshot, content: &ZoneContent) -> Option<Self> {
        match entity {
            EntityRef::Player(_) => Some(Self::Ranger),
            EntityRef::Creature(_) => Self::of_family(
                content
                    .creature_template(CreatureTemplateId::new(record.appearance))?
                    .family,
            ),
            EntityRef::Npc(_) => None,
        }
    }

    /// The model creatures of `family` use, if any.
    #[must_use]
    pub const fn of_family(family: CreatureFamily) -> Option<Self> {
        match family {
            CreatureFamily::Wolf => Some(Self::Wolf),
            CreatureFamily::Vermin => Some(Self::Rat),
            CreatureFamily::Boar => Some(Self::Pig),
            CreatureFamily::Marauder
            | CreatureFamily::Mirefin
            | CreatureFamily::Redbrand
            | CreatureFamily::Prop => None,
        }
    }
}

/// Per-unit clip choice and clock. Presentation-owned and cosmetic.
#[derive(Clone, Debug, Default)]
pub struct Playback {
    clip: &'static str,
    time: f32,
    attack_until: f64,
}

impl Playback {
    /// Starts an observed attack clip of `duration` seconds at `now`.
    pub fn attack(&mut self, duration: f32, now: f64) {
        self.attack_until = now + f64::from(duration.max(0.3));
    }

    /// Advances and returns the clip and time to pose this frame; `duration` gives a clip's
    /// length in seconds (zero when the model lacks it).
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        clips: Clips,
        duration: impl Fn(&str) -> f32,
        speed: f64,
        dead: bool,
        now: f64,
        dt: f64,
        reduced: bool,
        guarding: bool,
    ) -> (&'static str, f32) {
        let (clip, rate, once) = if dead {
            (clips.death, 1., true)
        } else if now < self.attack_until && !reduced {
            (clips.attack, 1., true)
        } else if speed > 3.2 {
            (clips.run, (speed / 5.).clamp(0.7, 1.5), false)
        } else if speed > 0.25 {
            (clips.walk, (speed / 1.8).clamp(0.6, 1.6), false)
        } else {
            // A standing raised guard holds the shield up; walking keeps the walk.
            (
                clips.guard.filter(|_| guarding).unwrap_or(clips.idle),
                1.,
                false,
            )
        };
        if clip != self.clip {
            self.clip = clip;
            self.time = 0.;
        } else if !reduced {
            self.time += (dt * rate) as f32;
        }
        let length = duration(clip);
        let time = if length <= 0. {
            0.
        } else if once {
            self.time.min(length)
        } else {
            self.time % length
        };
        (clip, time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::{CreatureId, NpcId, greyhaven_vale};

    #[test]
    fn players_are_the_ranger_and_only_animal_families_import() {
        let content = greyhaven_vale::content();
        let creature = |family: CreatureFamily| {
            let spawn = content
                .creature_spawns()
                .iter()
                .find(|spawn| content.creature_template(spawn.template).unwrap().family == family)
                .unwrap();
            let mut record = content_record();
            record.appearance = u16::try_from(spawn.template.get()).unwrap();
            Kind::of(EntityRef::Creature(CreatureId::new(1)), &record, &content)
        };
        assert_eq!(
            Kind::of(EntityRef::Player(1), &content_record(), &content),
            Some(Kind::Ranger)
        );
        assert_eq!(creature(CreatureFamily::Wolf), Some(Kind::Wolf));
        assert_eq!(creature(CreatureFamily::Vermin), Some(Kind::Rat));
        assert_eq!(creature(CreatureFamily::Boar), Some(Kind::Pig));
        assert_eq!(creature(CreatureFamily::Redbrand), None);
        assert_eq!(
            Kind::of(EntityRef::Npc(NpcId::new(1)), &content_record(), &content),
            None
        );
    }

    fn content_record() -> EntitySnapshot {
        EntitySnapshot {
            kind: mmorpg_core::EntityKind::Player,
            id: 1,
            appearance: 0,
            position: [0; 3],
            velocity: [0; 3],
            facing: 0,
            level: 1,
            health_percent: 100,
            flags: mmorpg_core::EntityFlags::default(),
        }
    }

    #[test]
    fn clip_choice_follows_guard_speed_attack_and_death() {
        let clips = Kind::Ranger.clips();
        let one = |_: &str| 1.;
        let mut playback = Playback::default();
        assert_eq!(
            playback
                .advance(clips, one, 0., false, 0., 0.1, false, true)
                .0,
            "Idle_Shield_Loop"
        );
        assert_eq!(
            playback
                .advance(clips, one, 1., false, 0., 0.1, false, true)
                .0,
            "Walk_Loop"
        );
        assert_eq!(
            Playback::default()
                .advance(Kind::Wolf.clips(), one, 0., false, 0., 0.1, false, true)
                .0,
            "Idle"
        );
        let mut playback = Playback::default();
        assert_eq!(
            playback
                .advance(clips, one, 0., false, 0., 0.1, false, false)
                .0,
            "Idle_Loop"
        );
        assert_eq!(
            playback
                .advance(clips, one, 4., false, 0., 0.1, false, false)
                .0,
            "Jog_Fwd_Loop"
        );
        playback.attack(1., 0.);
        assert_eq!(
            playback
                .advance(clips, one, 4., false, 0.1, 0.1, false, false)
                .0,
            "Sword_Attack"
        );
        playback.advance(clips, one, 0., true, 1., 0.1, false, false);
        assert_eq!(
            playback.advance(clips, one, 0., true, 5., 5., false, false),
            ("Death01", 1.)
        );
        let mut frozen = Playback::default();
        frozen.advance(clips, one, 0., false, 0., 0.1, true, false);
        assert_eq!(
            frozen
                .advance(clips, one, 0., false, 1., 0.5, true, false)
                .1,
            0.
        );
    }
}
