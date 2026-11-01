//! Client-only Battle Royale arena (#552): the shared symmetric multilevel fortress that
//! battle-royale hosts, drawn as graybox, and the practice arena (#550) the page hosts locally.
//!
//! `battle_royale_core::arena` is the one definition of the fortress. A hosted match supplies
//! every physical/combat projection itself and must carry [`fortress`]'s content revision. The
//! practice arena is the same fortress bodies (colliders, ramps and lifts, unchanged) with its
//! own spawn lattice in the south courtyard and stationary training targets, simulated by the
//! shared `mmorpg-core` rules and Rapier in [`crate::host::LocalZoneHost::practice_arena`]. Both
//! share this content's character, ability, item and target descriptions; no Greyhaven props or
//! NPCs. The renderer's geometry is a projection of the same boxes, never gameplay truth.
use std::sync::{Arc, OnceLock};

use battle_royale_core::arena::{self, ArenaPart};
use mmorpg_core::{
    CharacterPersistence, ClassChoice, CreatureBehaviour, CreatureFamily, CreatureId,
    CreatureSpawn, CreatureTemplate, CreatureTemplateId, EphemeralCharacterBuild, MAX_PLAYER_LEVEL,
    PlayerClass, PropKind, Sex, SpawnGrid, ZoneAreas, ZoneContent, ZoneDefinition,
};
use mmorpg_presentation::StaticScene;
use mmorpg_scenery::Scenery;
use serde::Serialize;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

use crate::static_scene::SceneBuffers;

/// The practice arena's training target (#550): a breakable prop at the maximum player level,
/// so it never moves or fights and keeps no threat, but takes real damage and breaks.
const TRAINING_TARGET: CreatureTemplateId = CreatureTemplateId::new(1);
/// Health of a training target: about twenty max-level weapon hits, a few ability rotations.
const TRAINING_TARGET_HEALTH: u32 = 300;
/// Practice spawn slot 0: just inside the south gate, facing +Z across the courtyard to the keep.
const PRACTICE_SPAWN: [i32; 2] = [0, -7_600];
/// Feet XZ of the training targets relative to [`PRACTICE_SPAWN`]: three 8–9 m ahead for melee,
/// two 21 m ahead for ranged abilities, all between spawn slots.
const TRAINING_TARGETS: [[i32; 2]; 5] = [
    [-300, 800],
    [0, 900],
    [300, 800],
    [-600, 2_100],
    [600, 2_100],
];
/// Practice slots: 16 columns along +X and 32 rows toward the keep, 2 m apart, inside the south
/// courtyard and clear of its ramps, lifts and walls.
const PRACTICE_SPAWN_GRID: SpawnGrid = SpawnGrid {
    origin: PRACTICE_SPAWN,
    columns: 16,
    spacing: 200,
};

/// The shared fortress content a hosted match runs.
pub(crate) fn fortress() -> Arc<ZoneContent> {
    arena::content().expect("the pinned battle-royale fortress content is valid")
}

/// The practice arena: the fortress's own bodies, a courtyard spawn lattice and training targets.
pub(crate) fn content() -> Arc<ZoneContent> {
    static CONTENT: OnceLock<Arc<ZoneContent>> = OnceLock::new();
    Arc::clone(CONTENT.get_or_init(|| {
        let fortress = fortress();
        let definition = ZoneDefinition::with_spawn_grid(
            fortress.definition().gravity(),
            PRACTICE_SPAWN_GRID,
            fortress.definition().colliders().to_vec(),
        )
        .expect("practice spawn lattice is clear of the fortress");
        let target = CreatureTemplate {
            id: TRAINING_TARGET,
            name: "Training Target".to_owned(),
            family: CreatureFamily::Prop,
            behaviour: CreatureBehaviour::Prop(PropKind::Barrel),
            min_level: MAX_PLAYER_LEVEL,
            max_level: MAX_PLAYER_LEVEL,
            elite: false,
            health: TRAINING_TARGET_HEALTH,
            health_per_level: 0,
            damage: [1, 1],
            damage_per_level: 0,
            swing_ticks: mmorpg_core::ticks_from_ms(2_000),
            respawn_ticks: mmorpg_core::unit::CORPSE_TICKS,
            half_extents: [30, 45, 30],
        };
        let spawns = (1..)
            .zip(TRAINING_TARGETS)
            .map(|(id, [x, z])| CreatureSpawn {
                id: CreatureId::new(id),
                template: TRAINING_TARGET,
                position: [PRACTICE_SPAWN[0] + x, PRACTICE_SPAWN[1] + z],
                facing: 32_768,
                wander_radius: 0,
                levels: None,
            })
            .collect();
        let content = ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![target],
            spawns,
            Vec::new(),
            PRACTICE_SPAWN,
        )
        .expect("practice arena content is valid");
        Arc::new(
            content
                .with_character_persistence(CharacterPersistence::Ephemeral)
                .with_creature_abilities(Vec::new())
                .and_then(|content| content.with_platforms(fortress.platforms().to_vec()))
                .and_then(|content| content.with_ramps(fortress.ramps().to_vec()))
                .expect("the fortress's lifts and ramps bind to the practice arena"),
        )
    }))
}

/// The graybox look of a fortress body, or `None` for the ground (the flat terrain) and the
/// lifts (drawn by the unit presenter where their schedule puts them) and ramps (wedges).
const fn graybox_kind(part: ArenaPart) -> Option<mmorpg_scenery::PropKind> {
    use mmorpg_scenery::PropKind as Kind;
    match part {
        ArenaPart::Wall | ArenaPart::Parapet | ArenaPart::Column => Some(Kind::GrayboxWall),
        ArenaPart::Tower => Some(Kind::GrayboxTower),
        ArenaPart::Floor | ArenaPart::Bridge => Some(Kind::GrayboxWalkway),
        ArenaPart::Ground | ArenaPart::Ramp | ArenaPart::Lift => None,
    }
}

/// The fortress drawn from its own collider boxes and ramp wedges, keyed by its revision, and
/// the fortress body of each of its props (boxes, then ramps).
fn graybox() -> &'static (Arc<Scenery>, Vec<Option<u32>>) {
    static SCENERY: OnceLock<(Arc<Scenery>, Vec<Option<u32>>)> = OnceLock::new();
    SCENERY.get_or_init(|| {
        let fortress = fortress();
        let parts = arena::parts();
        let boxes: Vec<_> = fortress
            .definition()
            .colliders()
            .iter()
            .filter_map(|collider| {
                let (_, part) = parts.iter().find(|(body, _)| *body == collider.id)?;
                graybox_kind(*part).map(|kind| (collider.clone(), kind))
            })
            .collect();
        let bodies = boxes
            .iter()
            .map(|(collider, _)| Some(collider.id))
            .chain(fortress.ramps().iter().map(|ramp| Some(ramp.body)))
            .collect();
        let scenery = Scenery::graybox(
            fortress.revision(),
            &boxes,
            fortress.ramps(),
            fortress.platforms(),
        );
        (Arc::new(scenery), bodies)
    })
}

pub(crate) fn shared_scenery() -> Arc<Scenery> {
    Arc::clone(&graybox().0)
}

fn scene() -> &'static StaticScene {
    static SCENE: OnceLock<StaticScene> = OnceLock::new();
    SCENE.get_or_init(|| StaticScene::new(&shared_scenery()))
}

fn buffers() -> &'static SceneBuffers {
    static BUFFERS: OnceLock<SceneBuffers> = OnceLock::new();
    BUFFERS.get_or_init(|| crate::static_scene::encode(scene()))
}

/// The fortress kit's pieces over [`fortress`] (#591), as the page draws them.
#[derive(Serialize)]
struct DressingEntry {
    /// Every bundle entry key drawn, sorted.
    entries: Vec<String>,
    /// Every piece, as [`ArenaPresentation::kit_update`] indexes them: the static pieces, then
    /// the lift pieces at their platform's low stop.
    placements: Vec<PlacementEntry>,
    /// The lift pieces, in placement order after the statics.
    lifts: Vec<LiftEntry>,
    /// Per entry its variants, finest first.
    lods: Vec<LodEntry>,
}

#[derive(Serialize)]
struct PlacementEntry {
    key: String,
    /// Column-major entry-to-world matrix, metres.
    matrix: [f32; 16],
}

#[derive(Serialize)]
struct LiftEntry {
    /// The platform whose `UnitPresenter` frame `platforms` entry places it.
    platform: u8,
    /// Its index into the placements.
    placement: usize,
}

#[derive(Serialize)]
struct LodEntry {
    key: String,
    variants: Vec<String>,
}

/// The dressed kit: its pieces and their level selection.
struct Kit {
    dressing: mmorpg_presentation::composition::Dressing,
    lods: mmorpg_presentation::lod::Lods,
    /// Each lift piece's base where it was last placed, metres.
    bases: Vec<[f64; 3]>,
}

/// Reuses the same Rust scenery meshes and character catalog as the ordinary client.
#[derive(Default)]
#[wasm_bindgen]
pub struct ArenaPresentation {
    /// The graybox props the fortress kit dresses (sorted), once [`Self::dress`] read it.
    dressed: Vec<usize>,
    kit: Option<Kit>,
}

impl ArenaPresentation {
    fn dressing(&mut self, descriptor: &str) -> Result<DressingEntry, String> {
        let dressing =
            mmorpg_presentation::composition::Descriptor::parse(descriptor)?.dress(&fortress());
        self.dressed = dressing.props(&graybox().1);
        let statics = dressing.statics.len();
        let entry = DressingEntry {
            entries: dressing.entries().into_iter().map(str::to_owned).collect(),
            placements: dressing
                .statics
                .iter()
                .map(|piece| PlacementEntry {
                    key: piece.entry.clone(),
                    matrix: piece.model,
                })
                .chain(dressing.lifts.iter().map(|piece| PlacementEntry {
                    key: piece.entry.clone(),
                    matrix: piece.model(piece.low),
                }))
                .collect(),
            lifts: dressing
                .lifts
                .iter()
                .enumerate()
                .map(|(index, piece)| LiftEntry {
                    platform: piece.platform.get(),
                    placement: statics + index,
                })
                .collect(),
            lods: dressing
                .lods
                .iter()
                .map(|entry| LodEntry {
                    key: entry.entry.clone(),
                    variants: entry
                        .levels
                        .iter()
                        .map(|(variant, _)| variant.clone())
                        .collect(),
                })
                .collect(),
        };
        self.kit = Some(Kit {
            lods: dressing.lods(),
            bases: dressing.lifts.iter().map(|lift| lift.low).collect(),
            dressing,
        });
        Ok(entry)
    }
}

#[wasm_bindgen]
impl ArenaPresentation {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// The fortress kit (#591) over the fortress, from its descriptor JSON: the pieces whose
    /// every body is exactly one of [`fortress`]'s and each entry's variants. The graybox of
    /// the bodies they dress is hidden while the page draws the kit
    /// ([`Self::static_scene_shown`]); [`Self::kit_update`] picks each piece's level.
    ///
    /// # Errors
    ///
    /// For a malformed descriptor.
    pub fn dress(&mut self, descriptor: &str) -> Result<String, JsError> {
        let entry = self
            .dressing(descriptor)
            .map_err(|error| JsError::new(&error))?;
        Ok(serde_json::to_string(&entry).expect("the dressing is serializable data"))
    }

    /// Moves the lift piece of `platform` to the platform's base at `(x, y, z)` metres: its
    /// 16 column-major model floats, or none when the kit draws no piece for that platform or
    /// it has not moved.
    #[wasm_bindgen(js_name = kitPlace)]
    pub fn kit_place(&mut self, platform: u8, x: f64, y: f64, z: f64) -> Option<Vec<f32>> {
        let kit = self.kit.as_mut()?;
        let index = kit
            .dressing
            .lifts
            .iter()
            .position(|lift| lift.platform.get() == platform)?;
        if kit.bases[index] == [x, y, z] {
            return None;
        }
        kit.bases[index] = [x, y, z];
        let placed = kit.dressing.lifts[index].placed([x, y, z]);
        kit.lods.place(kit.dressing.statics.len() + index, placed);
        Some(placed.model.to_vec())
    }

    /// Advances the kit's level selection (`mmorpg_presentation::lod::Lods::update`) for a
    /// camera at `(x, y, z)` metres with vertical field of view `fov_y` radians and
    /// column-major `clip_from_world` (16 floats), `seconds` after the previous update: per
    /// drawn level its placement, level and fade, three floats each, or `undefined` when that
    /// is unchanged (the camera rests, nothing fades and no lift moved).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = kitUpdate)]
    pub fn kit_update(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        fov_y: f64,
        clip_from_world: &[f32],
        seconds: f64,
        bias: f64,
        fade: bool,
    ) -> Option<Vec<f32>> {
        let lods = &mut self.kit.as_mut()?.lods;
        let frustum = <[f32; 16]>::try_from(clip_from_world).ok();
        lods.update([x, y, z], fov_y, seconds, bias, fade, frustum);
        lods.changed().then(|| crate::shown_floats(lods.shown()))
    }

    /// One model matrix (16 floats) per animated part of the static scene: each body's graybox.
    #[wasm_bindgen(js_name = staticSceneMotion)]
    pub fn static_scene_motion(&self, seconds: f64, animate: bool) -> Vec<f32> {
        scene()
            .motion(seconds, animate)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Per animated part 1 (drawn) or 0: a body's graybox is hidden while the page draws the
    /// fortress kit (`fortress_kit`) and a piece of it dresses that body.
    #[wasm_bindgen(js_name = staticSceneShown)]
    pub fn static_scene_shown(&self, viewer: &[f64], fortress_kit: bool) -> Vec<u8> {
        let viewer = (viewer.len() == 3).then(|| [viewer[0], viewer[1], viewer[2]]);
        let kits = mmorpg_presentation::Kits {
            dressed: if fortress_kit { &self.dressed } else { &[] },
            ..mmorpg_presentation::Kits::default()
        };
        scene()
            .shown(viewer, kits)
            .into_iter()
            .map(u8::from)
            .collect()
    }
    /// The fortress content revision a hosted match's projections must carry.
    #[wasm_bindgen(js_name = contentRevision)]
    pub fn content_revision(&self) -> u64 {
        fortress().revision()
    }
    pub fn scenery(&self) -> String {
        // Fortress boxes export their exact colliders, so the camera treats them as structures.
        let export = crate::scenery::export(
            &shared_scenery(),
            fortress().definition().colliders(),
            &mmorpg_core::ZoneAreas::default(),
        );
        serde_json::to_string(&export).expect("scenery export is serializable data")
    }
    pub fn catalog(&self) -> String {
        serde_json::to_string(&crate::catalog::catalog(&content()))
            .expect("character catalog is serializable data")
    }
    #[wasm_bindgen(js_name = maximumLevel)]
    pub fn maximum_level(&self) -> u8 {
        MAX_PLAYER_LEVEL
    }
    /// Default choices come from the actual admission API; the UI can choose other legal slots.
    #[wasm_bindgen(js_name = standardEquipment)]
    pub fn standard_equipment(&self, class: u8) -> Result<Vec<u16>, JsError> {
        let class = PlayerClass::from_code(class).ok_or_else(|| JsError::new("unknown class"))?;
        let build = EphemeralCharacterBuild::standard(ClassChoice {
            class,
            sex: Sex::Female,
        })
        .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(build
            .equipment()
            .slots()
            .iter()
            .map(|slot| slot.map_or(0, |id| id.get()))
            .collect())
    }
    #[wasm_bindgen(js_name = staticScene)]
    pub fn static_scene(&self) -> String {
        buffers().0.clone()
    }
    #[wasm_bindgen(js_name = staticSceneVertices)]
    pub fn static_scene_vertices(&self) -> Vec<f32> {
        buffers().1.clone()
    }
    #[wasm_bindgen(js_name = staticSceneIndices)]
    pub fn static_scene_indices(&self) -> Vec<u32> {
        buffers().2.clone()
    }
    #[wasm_bindgen(js_name = staticSceneModels)]
    pub fn static_scene_models(&self) -> Vec<f32> {
        buffers().3.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::LocalZoneHost;
    use mmorpg_core::{EntityKind, PlatformPhase, PlayerId, ZoneCommand};
    use mmorpg_protocol::{decode_snapshot, encode_command};

    /// The fetched `fortress-kit` bundle's descriptor (`scripts/fetch-bundles.sh`).
    fn kit_descriptor() -> String {
        let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public/scenery/fortress.kit");
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(bundle.join("manifest.json")).expect("run scripts/fetch-bundles.sh"),
        )
        .unwrap();
        let path = manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|asset| asset["attachments"].as_array().unwrap())
            .find(|attachment| attachment["name"] == "fortress-descriptor")
            .and_then(|attachment| attachment["path"].as_str())
            .expect("the kit carries its descriptor");
        std::fs::read_to_string(bundle.join(path)).unwrap()
    }

    #[test]
    fn the_fortress_kit_dresses_every_body_but_the_ground_and_hides_its_graybox() {
        let mut presentation = ArenaPresentation::new();
        let dressing = presentation.dressing(&kit_descriptor()).unwrap();
        let fortress = fortress();
        assert_eq!(dressing.lifts.len(), fortress.platforms().len());
        // Every graybox prop is dressed: the whole fortress is drawn from the kit.
        let props = shared_scenery().props.len();
        assert_eq!(presentation.dressed, (0..props).collect::<Vec<_>>());
        let shown = |kit| presentation.static_scene_shown(&[], kit);
        assert!(shown(false).iter().all(|&shown| shown == 1));
        assert!(shown(true).iter().all(|&shown| shown == 0));
        // Each drawn entry has its levels, the finest first.
        assert_eq!(dressing.entries.len(), dressing.lods.len());
        for entry in &dressing.lods {
            assert_eq!(entry.variants[0], "default", "{}", entry.key);
        }
        // From the courtyard every piece is drawn once, settled (no frustum: all in view);
        // the same camera again changes nothing until a lift moves.
        let fov = 50_f64.to_radians();
        let update = |presentation: &mut ArenaPresentation| {
            presentation.kit_update(0., 2., -60., fov, &[], 0.016, 1., true)
        };
        let drawn = update(&mut presentation).expect("the first update draws");
        assert_eq!(drawn.len(), 3 * dressing.placements.len());
        assert!(drawn.chunks(3).all(|shown| shown[2] == 1.));
        assert_eq!(update(&mut presentation), None);
        // A lift piece starts at its low stop and follows its platform.
        let lift = &dressing.lifts[0];
        let low = dressing.placements[lift.placement].matrix;
        let [x, y, z] = [12, 13, 14].map(|i| f64::from(low[i]));
        let raised = presentation
            .kit_place(lift.platform, x, y + 3., z)
            .expect("the lift moved");
        assert!((f64::from(raised[13]) - (y + 3.)).abs() < 1e-4);
        assert_eq!(presentation.kit_place(lift.platform, x, y + 3., z), None);
        // The courtyard lifts rest flush with the ground: the terrain leaves their footprints.
        assert_eq!(shared_scenery().terrain_holes.len(), 4);
    }

    #[test]
    fn a_body_the_kit_does_not_fit_exactly_keeps_its_graybox() {
        let mut descriptor: serde_json::Value = serde_json::from_str(&kit_descriptor()).unwrap();
        // The first curtain wall's body one unit taller than the fortress's.
        let wall = &mut descriptor["composition"]["items"][0]["bodies"][0];
        let id = wall["body"].as_u64().unwrap() as u32;
        wall["halfExtents"][1] = (wall["halfExtents"][1].as_i64().unwrap() + 1).into();
        let mut presentation = ArenaPresentation::new();
        let dressing = presentation.dressing(&descriptor.to_string()).unwrap();
        let scenery = shared_scenery();
        let prop = scenery
            .props
            .iter()
            .position(|prop| prop.collider == Some(id))
            .unwrap();
        assert!(!presentation.dressed.contains(&prop));
        assert_eq!(presentation.dressed.len(), scenery.props.len() - 1);
        assert_eq!(
            presentation
                .static_scene_shown(&[], true)
                .iter()
                .filter(|&&shown| shown == 1)
                .count(),
            scene()
                .animated
                .iter()
                .filter(|part| matches!(
                    part.motion,
                    mmorpg_presentation::Motion::Fallback {
                        kit: mmorpg_presentation::Kit::Graybox(index),
                        ..
                    } if index == prop
                ))
                .count()
        );
        assert!(dressing.placements.len() - dressing.lifts.len() < 82 - 8);
    }

    #[test]
    fn practice_runs_the_fortress_bodies_and_draws_each_one() {
        let (fortress, practice) = (fortress(), content());
        assert_eq!(
            practice.definition().colliders(),
            fortress.definition().colliders()
        );
        assert_eq!(practice.ramps(), fortress.ramps());
        assert_eq!(practice.platforms(), fortress.platforms());
        let scenery = shared_scenery();
        assert_eq!(scenery.content_revision, fortress.revision());
        let drawn = |id| scenery.props.iter().any(|prop| prop.collider == Some(id));
        for collider in fortress.definition().colliders() {
            let ground = arena::parts().contains(&(collider.id, ArenaPart::Ground));
            assert_eq!(drawn(collider.id), !ground, "collider {}", collider.id);
        }
        let wedges = scenery
            .props
            .iter()
            .filter(|prop| prop.kind == mmorpg_scenery::PropKind::GrayboxRamp)
            .count();
        assert_eq!(wedges, fortress.ramps().len());
    }

    /// One practice character steered through the shared zone, commands at every tick.
    struct Walker {
        host: LocalZoneHost,
        player: PlayerId,
        sequence: u32,
    }

    impl Walker {
        fn new() -> Self {
            let build = EphemeralCharacterBuild::standard(ClassChoice {
                class: PlayerClass::Warden,
                sex: Sex::Female,
            })
            .unwrap();
            let (host, player) = LocalZoneHost::practice_arena(&build).unwrap();
            Self {
                host,
                player,
                sequence: 0,
            }
        }

        fn position(&self) -> [i32; 3] {
            let snapshot = decode_snapshot(&self.host.projection(self.player).unwrap()).unwrap();
            snapshot
                .entities
                .iter()
                .find(|entity| entity.kind == EntityKind::Player && entity.id == self.player)
                .unwrap()
                .position
        }

        fn command(&mut self, forward: i8, facing: u16) {
            self.sequence += 1;
            let command = encode_command(ZoneCommand::Move {
                forward,
                strafe: 0,
                facing,
            });
            self.host
                .submit(self.player, self.sequence, &command)
                .unwrap();
        }

        fn idle(&mut self, ticks: u32) {
            self.command(0, 0);
            for _ in 0..ticks {
                self.host.tick().unwrap();
            }
        }

        fn walk_to(&mut self, x: i32, z: i32) -> [i32; 3] {
            for _ in 0..2_000 {
                let at = self.position();
                let (dx, dz) = (f64::from(x - at[0]), f64::from(z - at[2]));
                if dx.hypot(dz) < 25. {
                    self.idle(20);
                    return self.position();
                }
                let yaw = (dx.atan2(dz) / std::f64::consts::TAU * 65_536.).round() as i32;
                self.command(1, yaw.rem_euclid(65_536) as u16);
                self.host.tick().unwrap();
            }
            panic!("stuck at {:?} on the way to ({x}, {z})", self.position());
        }
    }

    /// Whether a body centre `at` stands on a surface at `y`.
    fn standing_at(at: [i32; 3], y: i32) -> bool {
        (at[1] - mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[1] - y).abs() <= 10
    }

    #[test]
    fn a_practice_walk_reaches_the_wall_walk_a_corner_tower_and_the_keep_roof() {
        let mut walker = Walker::new();
        assert_eq!(walker.position(), [0, 90, -7_600]);
        // Courtyard ramp up to the wall walk, then the wall-walk ramp onto the corner tower.
        walker.walk_to(-4_400, -6_800);
        let wall_walk = walker.walk_to(-4_400, -8_370);
        assert!(standing_at(wall_walk, arena::WALL_WALK_Y), "{wall_walk:?}");
        walker.walk_to(-6_600, -8_370);
        let tower = walker.walk_to(-8_400, -8_400);
        assert!(standing_at(tower, arena::TOWER_TOP_Y), "{tower:?}");
        // Back down, through the keep's south door and up its interior ramp onto the roof.
        walker.walk_to(-6_600, -8_370);
        walker.walk_to(-4_400, -8_370);
        walker.walk_to(-4_400, -6_800);
        walker.walk_to(450, -1_400);
        let corridor = walker.walk_to(450, -990);
        assert!(standing_at(corridor, 0), "{corridor:?}");
        walker.walk_to(200, -990);
        let roof = walker.walk_to(-1_200, -990);
        assert!(standing_at(roof, arena::WALL_WALK_Y), "{roof:?}");
    }

    #[test]
    fn a_practice_character_rides_a_courtyard_lift_onto_the_keep_roof() {
        let mut walker = Walker::new();
        let lift = *fortress()
            .platforms()
            .iter()
            .find(|lift| lift.low[0] == -800 && lift.low[2] == -1_410)
            .expect("the south courtyard lift");
        walker.walk_to(-800, -1_900);
        // Board just as the lift comes to rest at the courtyard, flush with the ground.
        loop {
            let tick = walker.host.zone().current_tick();
            if lift.phase_at(tick) == (PlatformPhase::AtLow, 0) {
                break;
            }
            walker.idle(1);
        }
        walker.walk_to(-800, -1_410);
        walker.idle(lift.dwell + lift.travel_ticks() + 10);
        let ridden = walker.position();
        assert!(standing_at(ridden, arena::WALL_WALK_Y), "{ridden:?}");
        let roof = walker.walk_to(-800, -1_200);
        assert!(standing_at(roof, arena::WALL_WALK_Y), "{roof:?}");
    }
}
