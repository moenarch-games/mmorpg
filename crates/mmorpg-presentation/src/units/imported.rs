//! Which imported model presents a unit, its clip names and the per-unit clip clock.
//!
//! Players use asset-tooling's Ranger humanoid (Atlas 6.3.7); wolves, vermin and boars use the
//! bundled Quaternius CC0 animals (Atlas 6.3.6); the named Greyhaven residents wear the Ranger in
//! their appearance presets (#418) where the client draws them; humanoid creatures and other
//! NPCs keep their procedural models. Each client imports and skins the models with its own
//! provider; this module only decides what to pose. Presentation only, never read by gameplay.
use mmorpg_core::{
    CreatureFamily, CreatureTemplateId, EntityRef, EntitySnapshot, PlayerClass, ZoneContent,
};

/// What an imported humanoid carries in its sockets: the class weapons (#156, #244, #302,
/// asset-tooling #375) and the guards' sword.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Loadout {
    /// The Warden's arming sword and heater shield (#156).
    SwordAndShield,
    /// The Ranger's longbow and quiver on its back (#244).
    BowAndQuiver,
    /// The arming sword alone: the Greyhaven guards.
    Sword,
    /// The Rogue's blackened dagger.
    Dagger,
    /// The Necromancer's bone staff.
    BoneStaff,
    /// The Cleric's gilded staff.
    GildedStaff,
    /// The Druid's gnarled staff.
    GnarledStaff,
}

impl Loadout {
    /// The weapons a class carries. The Arcanist has no authored staff yet.
    #[must_use]
    pub const fn of_class(class: PlayerClass) -> Option<Self> {
        match class {
            PlayerClass::Warden => Some(Self::SwordAndShield),
            PlayerClass::Ranger => Some(Self::BowAndQuiver),
            PlayerClass::Rogue => Some(Self::Dagger),
            PlayerClass::Necromancer => Some(Self::BoneStaff),
            PlayerClass::Cleric => Some(Self::GildedStaff),
            PlayerClass::Druid => Some(Self::GnarledStaff),
            PlayerClass::Arcanist => None,
        }
    }

    /// The name both clients key their socket items by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SwordAndShield => "sword-and-shield",
            Self::BowAndQuiver => "bow-and-quiver",
            Self::Sword => "sword",
            Self::Dagger => "dagger",
            Self::BoneStaff => "staff.bone",
            Self::GildedStaff => "staff.gilded",
            Self::GnarledStaff => "staff.gnarled",
        }
    }
}

/// A named Greyhaven resident drawn in the Ranger bundle's `humanoid-appearance` preset (#418).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resident {
    Marshal,
    Guard,
    Innkeeper,
    Farmer,
    Healer,
}

impl Resident {
    /// The resident an NPC of this canonical name is, if it has a preset. Tanner Hilda Brook has
    /// none yet (the resident outfits exist on the male figure only).
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "Marshal Elden Greywatch" => Some(Self::Marshal),
            "Greyhaven Guard" => Some(Self::Guard),
            "Innkeeper Bram Tolliver" => Some(Self::Innkeeper),
            "Farmer Osric Mill" => Some(Self::Farmer),
            "Brother Aldous" => Some(Self::Healer),
            _ => None,
        }
    }

    /// The bundle's `humanoid-appearance.presets` id.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::Marshal => "resident.marshal",
            Self::Guard => "resident.guard",
            Self::Innkeeper => "resident.innkeeper",
            Self::Farmer => "resident.farmer",
            Self::Healer => "resident.healer",
        }
    }

    /// What the resident carries: the guards keep their sword.
    #[must_use]
    pub const fn loadout(self) -> Option<Loadout> {
        match self {
            Self::Guard => Some(Loadout::Sword),
            _ => None,
        }
    }
}

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
    pub jump: Option<JumpClips>,
}

/// In-place authored traversal; these clips never move the presentation root.
#[derive(Clone, Copy, Debug)]
pub struct JumpClips {
    pub start: &'static str,
    pub airborne: &'static str,
    pub land: &'static str,
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
                jump: Some(JumpClips {
                    start: "Jump_Start",
                    airborne: "Jump_Loop",
                    land: "Jump_Land",
                }),
            },
            Self::Wolf => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Gallop",
                attack: "Attack",
                death: "Death",
                guard: None,
                jump: None,
            },
            Self::Rat => Clips {
                idle: "Rat_Idle",
                walk: "Rat_Walk",
                run: "Rat_Run",
                attack: "Rat_Attack",
                death: "Rat_Death",
                guard: None,
                jump: None,
            },
            // The farm pig has no attack clip; its jump reads as a charge.
            Self::Pig => Clips {
                idle: "Idle",
                walk: "Walk",
                run: "Run",
                attack: "Jump",
                death: "Death",
                guard: None,
                jump: None,
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
            // A Druid's companion (#296) borrows the CC0 Quaternius wolf as its placeholder;
            // skeletons (#295) are procedural placeholders until their art exists.
            EntityRef::Minion(_) => {
                mmorpg_core::is_companion(mmorpg_core::minion_ability(record.appearance))
                    .then_some(Self::Wolf)
            }
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
            | CreatureFamily::Ogre
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
    flight: Flight,
}

#[derive(Clone, Copy, Debug, Default)]
enum Flight {
    #[default]
    Ground,
    Rising,
    Airborne,
    Falling,
    Landing,
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
        vertical_speed: f64,
        dead: bool,
        now: f64,
        dt: f64,
        reduced: bool,
        guarding: bool,
    ) -> (&'static str, f32) {
        // The projection already supplies cosmetic vertical velocity. Retain the
        // flight phase through the apex instead of treating its zero velocity as
        // a landing. This is a visual cue, not a grounded/physics decision.
        self.flight = if dead {
            Flight::Ground
        } else if vertical_speed > 0.3 {
            match self.flight {
                Flight::Rising | Flight::Airborne => self.flight,
                _ => Flight::Rising,
            }
        } else if vertical_speed < -0.3 {
            Flight::Falling
        } else {
            match self.flight {
                Flight::Falling if reduced => Flight::Ground,
                Flight::Falling => Flight::Landing,
                Flight::Landing if self.time >= duration(self.clip) => Flight::Ground,
                other => other,
            }
        };
        if let Some(jump) = clips.jump
            && matches!(self.flight, Flight::Rising)
            && self.clip == jump.start
            && self.time >= duration(jump.start)
        {
            self.flight = Flight::Airborne;
        }
        let jump = clips.jump.and_then(|jump| {
            let (clip, once) = match self.flight {
                Flight::Ground => return None,
                Flight::Rising if !reduced => (jump.start, true),
                Flight::Rising | Flight::Airborne | Flight::Falling => (jump.airborne, false),
                Flight::Landing if !reduced => (jump.land, true),
                Flight::Landing => return None,
            };
            (duration(clip) > 0.).then_some((clip, 1., once))
        });
        let (clip, rate, once) = if dead {
            (clips.death, 1., true)
        } else if now < self.attack_until && !reduced {
            (clips.attack, 1., true)
        } else if let Some(jump) = jump {
            jump
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
    fn named_greyhaven_residents_have_presets_and_classes_their_weapons() {
        let content = greyhaven_vale::content();
        let presets: Vec<_> = content
            .npcs()
            .iter()
            .map(|npc| Resident::of_name(&npc.name).map(Resident::preset))
            .collect();
        assert_eq!(
            presets,
            [
                Some("resident.marshal"),
                None,
                Some("resident.innkeeper"),
                Some("resident.farmer"),
                Some("resident.healer"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
            ]
        );
        let names: Vec<_> = PlayerClass::ALL
            .into_iter()
            .map(|class| Loadout::of_class(class).map(Loadout::name))
            .collect();
        assert_eq!(
            names,
            [
                Some("sword-and-shield"),
                Some("bow-and-quiver"),
                None,
                Some("staff.bone"),
                Some("dagger"),
                Some("staff.gilded"),
                Some("staff.gnarled"),
            ]
        );
    }

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
                .advance(clips, one, 0., 0., false, 0., 0.1, false, true)
                .0,
            "Idle_Shield_Loop"
        );
        assert_eq!(
            playback
                .advance(clips, one, 1., 0., false, 0., 0.1, false, true)
                .0,
            "Walk_Loop"
        );
        assert_eq!(
            Playback::default()
                .advance(Kind::Wolf.clips(), one, 0., 0., false, 0., 0.1, false, true)
                .0,
            "Idle"
        );
        let mut playback = Playback::default();
        assert_eq!(
            playback
                .advance(clips, one, 0., 0., false, 0., 0.1, false, false)
                .0,
            "Idle_Loop"
        );
        assert_eq!(
            playback
                .advance(clips, one, 4., 0., false, 0., 0.1, false, false)
                .0,
            "Jog_Fwd_Loop"
        );
        playback.attack(1., 0.);
        assert_eq!(
            playback
                .advance(clips, one, 4., 0., false, 0.1, 0.1, false, false)
                .0,
            "Sword_Attack"
        );
        playback.advance(clips, one, 0., 0., true, 1., 0.1, false, false);
        assert_eq!(
            playback.advance(clips, one, 0., 0., true, 5., 5., false, false),
            ("Death01", 1.)
        );
        let mut frozen = Playback::default();
        frozen.advance(clips, one, 0., 0., false, 0., 0.1, true, false);
        assert_eq!(
            frozen
                .advance(clips, one, 0., 0., false, 1., 0.5, true, false)
                .1,
            0.
        );
    }
    #[test]
    fn authored_jump_tracks_ascent_apex_descent_and_landing() {
        let mut playback = Playback::default();
        let clips = Kind::Ranger.clips();
        let duration = |_: &str| 0.2;
        let mut pose = |vertical, dt| {
            playback.advance(clips, duration, 0., vertical, false, 0., dt, false, false)
        };
        assert_eq!(pose(4.8, 0.1).0, "Jump_Start");
        pose(3., 0.2);
        assert_eq!(pose(1., 0.1).0, "Jump_Loop");
        assert_eq!(pose(0., 0.1).0, "Jump_Loop");
        assert_eq!(pose(-2., 0.1).0, "Jump_Loop");
        assert_eq!(pose(0., 0.1).0, "Jump_Land");
        pose(0., 0.2);
        assert_eq!(pose(0., 0.1).0, "Idle_Loop");
    }

    #[test]
    fn jump_respects_missing_clips_reduced_motion_and_death() {
        let mut playback = Playback::default();
        let clips = Kind::Ranger.clips();
        let missing = |name: &str| if name.starts_with("Jump_") { 0. } else { 1. };
        assert_eq!(
            playback
                .advance(clips, missing, 4., 4.8, false, 0., 0.1, false, false)
                .0,
            "Jog_Fwd_Loop"
        );
        let one = |_: &str| 1.;
        assert_eq!(
            playback.advance(clips, one, 0., -2., false, 0., 0.1, true, false),
            ("Jump_Loop", 0.)
        );
        assert_eq!(
            playback
                .advance(clips, one, 0., -2., true, 0., 0.1, false, false)
                .0,
            "Death01"
        );
    }
}
