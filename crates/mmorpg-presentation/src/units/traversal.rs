//! Authored climbing, ladder, rope and mantle motion on the humanoid (#500, Atlas 6.6.43).
//!
//! The zone decides every climb and mantle and moves the body (#490); the projection says what
//! a climber holds and how high the mantled ledge is. The clips are authored in place
//! (asset-tooling #416), so the presentation only picks and paces them
//! ([`super::imported::Playback`]) and, through a mantle or a top-out, holds the drawn body on the
//! clip's authored root path ([`Align`]) so the hands meet the edge about when the clip plants
//! them. That placement is cosmetic and bounded: the drawn root only ever trails the projected
//! body, by at most [`MAX_LAG_METERS`] up or toward the ledge, and settles back onto it after.

use std::collections::BTreeMap;

use serde_json::Value;

/// A traversal clip's `humanoid-clips` motion data (asset-tooling #416).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Motion {
    /// The speed along the face in metres per second at which a climb loop plays at its
    /// authored rate (`climbSpeedMetersPerSecond`).
    pub climb_speed: Option<f64>,
    /// The ledge or edge height in metres a mantle or top-out was authored against
    /// (`ledgeHeightMeters`, `edgeHeightMeters`).
    pub ledge_height: Option<f64>,
    /// The root path the in-place clip was authored against (`travelSamples`).
    pub travel: Option<Travel>,
    /// When a strike's weapon hand meets its target, in seconds into the clip
    /// (`contact.seconds`, asset-tooling #447); a strike's wind-up holds before it (#582).
    pub contact: Option<f64>,
    /// When a throw's hand reaches the belt and takes the throwable, and when it lets go, in
    /// seconds into the clip (`drawSeconds`, `releaseSeconds`, asset-tooling #418).
    pub draw: Option<f64>,
    pub release: Option<f64>,
    /// A crossbow clip's marks (`crossbow`, asset-tooling #419).
    pub crossbow: Option<CrossbowMarks>,
    /// A caster, staff or mace clip's marks (`cast`, asset-tooling #420).
    pub cast: Option<CastMarks>,
}

/// A caster clip's `humanoid-clips` `cast` object (asset-tooling #420), in seconds into the
/// clip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CastMarks {
    /// The spell leaves the hand, the staff stamps, the blow lands or the ally is touched
    /// (`releaseSeconds`).
    pub release: Option<f64>,
    /// Starts in the first pose of the charge loop: crossfade into it from the charge
    /// (`startsFrom: "Cast_Charge_Loop"`).
    pub from_charge: bool,
    /// Where the spell starts (`channelJoint`, `channelPoint`).
    pub channel: Option<super::imported::Channel>,
    /// The cone's sweep, from and to (`sweepSeconds`).
    pub sweep: Option<[f64; 2]>,
    /// The drain's pulls in one loop (`pulseSeconds`).
    pub pulses: [Option<f64>; 2],
    /// The summoning's kneel, from and to (`kneelSeconds`).
    pub kneel: Option<[f64; 2]>,
}

impl CastMarks {
    fn parse(marks: &Value) -> Self {
        let span = |key: &str| {
            let span = marks[key].as_array().filter(|span| span.len() == 2)?;
            Some([span[0].as_f64()?, span[1].as_f64()?])
        };
        let hand = match marks["channelJoint"].as_str() {
            Some("hand_l") => Some(super::imported::Hand::Left),
            Some("hand_r") => Some(super::imported::Hand::Right),
            _ => None,
        };
        let point = marks["channelPoint"]
            .as_array()
            .filter(|point| point.len() == 3)
            .and_then(|point| Some([point[0].as_f64()?, point[1].as_f64()?, point[2].as_f64()?]));
        let pulse = |index: usize| marks["pulseSeconds"][index].as_f64();
        Self {
            release: marks["releaseSeconds"].as_f64(),
            from_charge: marks["startsFrom"].as_str() == Some("Cast_Charge_Loop"),
            channel: hand
                .zip(point)
                .map(|(hand, point)| super::imported::Channel { hand, point }),
            sweep: span("sweepSeconds"),
            pulses: [pulse(0), pulse(1)],
            kneel: span("kneelSeconds"),
        }
    }
}

/// A crossbow clip's `humanoid-clips` `crossbow` object (asset-tooling #419), in seconds into
/// the clip: the fire's release, and the reload's winding, hand-off and bolt.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrossbowMarks {
    /// The bolt leaves the groove (`Crossbow_Fire`).
    pub release: Option<f64>,
    /// The left hand winds the crank, from and to (`crankSeconds`).
    pub crank: Option<[f64; 2]>,
    /// The left hand holds the crossbow alone, from and to (`leftHandSeconds`).
    pub left_hand: Option<[f64; 2]>,
    /// The right hand has drawn the bolt over the shoulder (`boltDrawnSeconds`) …
    pub bolt_drawn: Option<f64>,
    /// … and laid it in the groove (`reloadedSeconds`).
    pub reloaded: Option<f64>,
}

/// An authored root path: one glTF translation (+Y up, +Z the facing) per frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Travel {
    pub sample_rate: f64,
    pub translations: Vec<[f64; 3]>,
}

impl Travel {
    /// The root's authored displacement from its first frame at `time` seconds, linear between
    /// frames and held at either end.
    #[must_use]
    pub fn at(&self, time: f64) -> [f64; 3] {
        let (Some(first), Some(last)) = (
            self.translations.first(),
            self.translations.len().checked_sub(1),
        ) else {
            return [0.; 3];
        };
        #[allow(clippy::cast_precision_loss)]
        let frame = (time * self.sample_rate).clamp(0., last as f64);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let below = frame.floor() as usize;
        let above = (below + 1).min(last);
        #[allow(clippy::cast_precision_loss)]
        let u = frame - below as f64;
        let (a, b) = (self.translations[below], self.translations[above]);
        std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * u - first[axis])
    }

    /// Seconds from the first frame to the last.
    #[must_use]
    pub fn duration(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let frames = self.translations.len().saturating_sub(1) as f64;
        if self.sample_rate > 0. {
            frames / self.sample_rate
        } else {
            0.
        }
    }
}

/// Every clip's traversal and strike-contact motion data, by clip name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Motions {
    clips: BTreeMap<String, Motion>,
}

impl Motions {
    /// Reads a `humanoid-clips-v1` descriptor. Clips without traversal or contact data are left
    /// out.
    ///
    /// # Errors
    ///
    /// For malformed JSON or another schema.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if value["schema"] != "humanoid-clips-v1" {
            return Err("the humanoid-clips descriptor is not humanoid-clips-v1".into());
        }
        let clips = value["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|clip| {
                let name = clip["name"].as_str()?;
                let travel = &clip["travelSamples"];
                let motion = Motion {
                    climb_speed: clip["climbSpeedMetersPerSecond"].as_f64(),
                    ledge_height: clip["ledgeHeightMeters"]
                        .as_f64()
                        .or_else(|| clip["edgeHeightMeters"].as_f64()),
                    travel: travel["sampleRate"].as_f64().and_then(|sample_rate| {
                        let translations = travel["translations"]
                            .as_array()?
                            .iter()
                            .map(|t| {
                                let t = t.as_array().filter(|t| t.len() == 3)?;
                                Some([t[0].as_f64()?, t[1].as_f64()?, t[2].as_f64()?])
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(Travel {
                            sample_rate,
                            translations,
                        })
                    }),
                    contact: clip["contact"]["seconds"].as_f64(),
                    draw: clip["drawSeconds"].as_f64(),
                    release: clip["releaseSeconds"].as_f64(),
                    crossbow: clip["crossbow"].as_object().map(|_| {
                        let marks = &clip["crossbow"];
                        let span = |key: &str| {
                            let span = marks[key].as_array().filter(|span| span.len() == 2)?;
                            Some([span[0].as_f64()?, span[1].as_f64()?])
                        };
                        CrossbowMarks {
                            release: marks["releaseSeconds"].as_f64(),
                            crank: span("crankSeconds"),
                            left_hand: span("leftHandSeconds"),
                            bolt_drawn: marks["boltDrawnSeconds"].as_f64(),
                            reloaded: marks["reloadedSeconds"].as_f64(),
                        }
                    }),
                    cast: clip["cast"]
                        .as_object()
                        .map(|_| CastMarks::parse(&clip["cast"])),
                };
                (motion != Motion::default()).then(|| (name.to_owned(), motion))
            })
            .collect();
        Ok(Self { clips })
    }

    #[must_use]
    pub fn get(&self, clip: &str) -> Option<&Motion> {
        self.clips.get(clip)
    }
}

/// The cloth chains' authored rotations through a clip (`secondaryPose`, asset-tooling #416):
/// one local rotation `[x, y, z, w]` per frame for each named joint.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondaryPose {
    pub sample_rate: f32,
    pub joints: Vec<(String, Vec<[f32; 4]>)>,
}

/// Each clip's `secondaryPose` from a `humanoid-clips-v1` descriptor, by clip name. No cloth
/// spring exists yet (#498 sways only hanging gear), so both clients play these as ordinary
/// rotation channels of the clip, on whichever outfit has the chains.
#[must_use]
pub fn secondary_poses(descriptor: &Value) -> BTreeMap<String, SecondaryPose> {
    descriptor["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|clip| {
            let pose = &clip["secondaryPose"];
            #[allow(clippy::cast_possible_truncation)]
            let sample_rate = pose["sampleRate"].as_f64()? as f32;
            let joints = pose["joints"]
                .as_object()?
                .iter()
                .map(|(joint, rotations)| {
                    let rotations = rotations
                        .as_array()?
                        .iter()
                        .map(|q| {
                            let q = q.as_array().filter(|q| q.len() == 4)?;
                            #[allow(clippy::cast_possible_truncation)]
                            let at = |i: usize| q[i].as_f64().map(|v| v as f32);
                            Some([at(0)?, at(1)?, at(2)?, at(3)?])
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((joint.clone(), rotations))
                })
                .collect::<Option<Vec<_>>>()?;
            Some((
                clip["name"].as_str()?.to_owned(),
                SecondaryPose {
                    sample_rate,
                    joints,
                },
            ))
        })
        .collect()
}

/// The farthest the drawn root trails the projected body through a mantle, up and toward the
/// ledge, in metres.
pub const MAX_LAG_METERS: f64 = 0.6;
/// Seconds over which the drawn root settles back onto the projected body.
pub const SETTLE_SECONDS: f64 = 0.2;

/// Holds one unit's drawn root on a mantle's or top-out's authored path (see the module
/// documentation).
#[derive(Clone, Copy, Debug, Default)]
pub struct Align {
    /// The path being followed: its clip, the drawn feet and yaw it began at, and the clip
    /// time last frame.
    path: Option<(&'static str, [f64; 3], f64, f32)>,
    offset: [f64; 3],
}

impl Align {
    /// The offset to draw the root by this frame, for a unit whose legs show `lower` (clip and
    /// time) with its projected feet at `feet`, facing `yaw`.
    pub fn advance(
        &mut self,
        motions: Option<&Motions>,
        lower: (&'static str, f32),
        feet: [f64; 3],
        yaw: f64,
        dt: f64,
        reduced: bool,
    ) -> [f64; 3] {
        let (clip, time) = lower;
        let travel = motions
            .and_then(|motions| motions.get(clip))
            .and_then(|motion| motion.travel.as_ref())
            .filter(|travel| !reduced && f64::from(time) < travel.duration());
        let Some(travel) = travel else {
            self.path = None;
            let keep = if reduced || SETTLE_SECONDS <= 0. {
                0.
            } else {
                (1. - dt / SETTLE_SECONDS).max(0.)
            };
            self.offset = self.offset.map(|axis| axis * keep);
            return self.offset;
        };
        let restarted = self
            .path
            .is_none_or(|(on, _, _, last)| on != clip || time + 1e-3 < last);
        if restarted {
            let drawn = std::array::from_fn(|axis| feet[axis] + self.offset[axis]);
            self.path = Some((clip, drawn, yaw, time));
        }
        let Some((_, start, start_yaw, last)) = &mut self.path else {
            return self.offset;
        };
        *last = time;
        let heading = [start_yaw.sin(), start_yaw.cos()];
        let moved = travel.at(f64::from(time));
        let target = [
            start[0] + heading[0] * moved[2],
            start[1] + moved[1],
            start[2] + heading[1] * moved[2],
        ];
        let up = (target[1] - feet[1]).clamp(-MAX_LAG_METERS, 0.);
        let toward = ((target[0] - feet[0]) * heading[0] + (target[2] - feet[2]) * heading[1])
            .clamp(-MAX_LAG_METERS, 0.);
        self.offset = [heading[0] * toward, up, heading[1] * toward];
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTOR: &str = r#"{
        "schema": "humanoid-clips-v1",
        "clips": [
            {"name": "Idle_Loop", "durationSeconds": 2.5},
            {"name": "Climb_Up_Loop", "climbSpeedMetersPerSecond": 0.9,
             "secondaryPose": {"sampleRate": 30, "joints": {"skirt_f_01": [[0, 0, 0, 1], [0.1, 0, 0, 0.995]]}}},
            {"name": "Mantle_Mid", "ledgeHeightMeters": 1.3,
             "travelSamples": {"sampleRate": 2, "translations": [[0, 0, 0], [0, 1.3, 0], [0, 1.3, 1]]}}
        ]
    }"#;

    #[test]
    fn reads_traversal_motion_and_cloth_poses_by_clip() {
        let motions = Motions::parse(DESCRIPTOR).expect("valid descriptor");
        assert!(motions.get("Idle_Loop").is_none());
        assert_eq!(motions.get("Climb_Up_Loop").unwrap().climb_speed, Some(0.9));
        let mantle = motions.get("Mantle_Mid").unwrap();
        assert_eq!(mantle.ledge_height, Some(1.3));
        let travel = mantle.travel.as_ref().unwrap();
        assert!((travel.duration() - 1.).abs() < 1e-9);
        assert_eq!(travel.at(0.25), [0., 0.65, 0.]);
        assert_eq!(travel.at(9.), [0., 1.3, 1.]);
        let poses = secondary_poses(&serde_json::from_str(DESCRIPTOR).unwrap());
        assert_eq!(poses["Climb_Up_Loop"].joints[0].0, "skirt_f_01");
        assert_eq!(poses["Climb_Up_Loop"].joints[0].1.len(), 2);
    }

    #[test]
    fn reads_the_caster_clips_cast_marks() {
        let motions = Motions::parse(
            r#"{"schema": "humanoid-clips-v1", "clips": [
                {"name": "Cast_Channel_Drain_Loop", "cast": {"channelJoint": "hand_l",
                 "channelPoint": [0.02, 0.045, 0], "pulseSeconds": [0.375, 1.125]}},
                {"name": "Cast_Release_Push", "cast": {"releaseSeconds": 0.22,
                 "startsFrom": "Cast_Charge_Loop", "channelJoint": "hand_r",
                 "channelPoint": [0.1, -0.8, 0.5]}},
                {"name": "Cast_Summon_Raise", "cast": {"releaseSeconds": 0.93,
                 "kneelSeconds": [0.45, 1.05]}}
            ]}"#,
        )
        .expect("valid descriptor");
        let cast = |clip| motions.get(clip).unwrap().cast.unwrap();
        let drain = cast("Cast_Channel_Drain_Loop");
        assert_eq!(drain.pulses, [Some(0.375), Some(1.125)]);
        assert_eq!(
            drain.channel,
            Some(super::super::imported::Channel {
                hand: super::super::imported::Hand::Left,
                point: [0.02, 0.045, 0.],
            })
        );
        let push = cast("Cast_Release_Push");
        assert_eq!(push.release, Some(0.22));
        assert!(push.from_charge);
        assert_eq!(
            push.channel.map(|channel| channel.hand),
            Some(super::super::imported::Hand::Right)
        );
        let raise = cast("Cast_Summon_Raise");
        assert_eq!(raise.kneel, Some([0.45, 1.05]));
        assert_eq!(raise.channel, None);
    }

    #[test]
    fn a_mantle_trails_the_body_on_its_authored_path_then_settles() {
        let motions = Motions::parse(DESCRIPTOR).expect("valid descriptor");
        let mut align = Align::default();
        // Facing +Z, the zone has lifted the body 1 m while the clip's root has risen 0.65 m.
        let start = align.advance(Some(&motions), ("Mantle_Mid", 0.), [0.; 3], 0., 0.1, false);
        assert_eq!(start, [0.; 3]);
        let rising = align.advance(
            Some(&motions),
            ("Mantle_Mid", 0.25),
            [0., 1., 0.],
            0.,
            0.1,
            false,
        );
        assert!((rising[1] + 0.35).abs() < 1e-9, "{rising:?}");
        // Far ahead of the authored path, the lag is bounded.
        let ahead = align.advance(
            Some(&motions),
            ("Mantle_Mid", 0.3),
            [0., 1.3, 2.],
            0.,
            0.1,
            false,
        );
        assert!((ahead[2] + MAX_LAG_METERS).abs() < 1e-9, "{ahead:?}");
        // Never above or ahead of the projected body.
        let low = align.advance(
            Some(&motions),
            ("Mantle_Mid", 0.5),
            [0., 0.5, -1.],
            0.,
            0.1,
            false,
        );
        assert_eq!(low, [0.; 3]);
        // Another clip settles the offset back onto the body.
        align.advance(
            Some(&motions),
            ("Mantle_Mid", 0.6),
            [0., 1.3, 2.],
            0.,
            0.1,
            false,
        );
        let settling = align.advance(
            Some(&motions),
            ("Idle_Loop", 0.),
            [0., 1.3, 2.],
            0.,
            0.1,
            false,
        );
        assert!(settling[2] > -MAX_LAG_METERS && settling[2] < 0.);
        let settled = align.advance(None, ("Idle_Loop", 0.), [0.; 3], 0., SETTLE_SECONDS, false);
        assert_eq!(settled, [0.; 3]);
    }
}
