//! The browser's minimap, tab targeting and HUD panel rules (#1280), all `mmorpg-client-model`'s
//! and called directly by the native client. Per-projection views cross once per projection
//! ([`panel_views`], the `panel-views` work counter); the minimap's constants once per page
//! ([`minimap_rules`]); the tab target once per press and the vendor's sales once per bag sheet.
//! The minimap's per-frame projection stays in the page, held to [`minimap_offset`] by a test.
use mmorpg_client_model::{
    hud::{self, AuraPolarity},
    items,
    minimap::{self, DotClass},
    panels::{self, LootAction},
    quest,
};
use mmorpg_core as core;
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::client_model::{core_snapshot, to_js};
use crate::host::hosted_content;
use crate::wire::{EntityRef, ZoneSnapshot};

/// Whether an aura helps its bearer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum AuraSide {
    Buff,
    Debuff,
}

/// One aura icon of a unit frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct AuraRow {
    pub ability: u32,
    pub polarity: AuraSide,
    /// The tooltip: the ability, its effect and the time left.
    pub label: String,
    /// The corner timer.
    pub timer: String,
}

/// A unit frame's buffs and debuffs, each longest first.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct AuraRows {
    pub buffs: Vec<AuraRow>,
    pub debuffs: Vec<AuraRow>,
}

fn aura_rows(auras: &[core::AuraView]) -> AuraRows {
    let rows = hud::aura_rows(auras);
    let convert = |rows: Vec<hud::AuraRow>| {
        rows.into_iter()
            .map(|row| AuraRow {
                ability: row.ability,
                polarity: match row.polarity {
                    AuraPolarity::Buff => AuraSide::Buff,
                    AuraPolarity::Debuff => AuraSide::Debuff,
                },
                label: row.label,
                timer: row.timer,
            })
            .collect()
    };
    AuraRows {
        buffs: convert(rows.buffs),
        debuffs: convert(rows.debuffs),
    }
}

/// The class of dot a projected unit shows on the minimap.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum MinimapDotClass {
    Player,
    Friendly,
    Hostile,
    Neutral,
    Minion,
    Dead,
}

impl From<DotClass> for MinimapDotClass {
    fn from(class: DotClass) -> Self {
        match class {
            DotClass::Player => Self::Player,
            DotClass::Friendly => Self::Friendly,
            DotClass::Hostile => Self::Hostile,
            DotClass::Neutral => Self::Neutral,
            DotClass::Minion => Self::Minion,
            DotClass::Dead => Self::Dead,
        }
    }
}

/// A projected unit's minimap dot class.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct MinimapDot {
    pub entity: EntityRef,
    pub class: MinimapDotClass,
}

/// A night raid's disturbance circle (#530), centre and radius in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Tsify)]
pub struct DisturbanceArea {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
}

/// An unlocked stone the waystone panel offers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct WaystoneDestination {
    pub id: u32,
    pub name: String,
}

/// The waystone panel at the stone in reach.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct WaystoneOffer {
    pub id: u32,
    pub name: String,
    pub destinations: Vec<WaystoneDestination>,
    /// The panel's footer: a hint without destinations, else the undiscovered count, else empty.
    pub status: String,
}

/// What a loot row's button sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum LootButtonAction {
    Need,
    Greed,
    Pass,
    Claim,
}

/// One button of a loot row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct LootButton {
    pub action: LootButtonAction,
    pub label: String,
    pub enabled: bool,
}

/// One drop of the shared loot window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct DepthsLootRow {
    pub drop: u8,
    pub item_name: String,
    pub line: String,
    pub buttons: Vec<LootButton>,
}

/// The shared loot window (#404): its run and one row per drop.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct DepthsLootRows {
    #[tsify(type = "bigint")]
    pub run_id: u64,
    pub rows: Vec<DepthsLootRow>,
}

/// What one projection's minimap, unit frames, waystone panel and loot window show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct PanelViews {
    /// The viewer's auras.
    pub player_auras: AuraRows,
    /// The viewer's target's auras.
    pub target_auras: AuraRows,
    /// Every projected unit but the viewer, in projection order.
    pub minimap_dots: Vec<MinimapDot>,
    pub disturbance: Option<DisturbanceArea>,
    pub waystone: Option<WaystoneOffer>,
    pub depths_loot: Option<DepthsLootRows>,
}

fn depths_loot(view: &core::DepthsLootView, tick: u64) -> DepthsLootRows {
    DepthsLootRows {
        run_id: view.run_id,
        rows: panels::depths_loot_rows(view, tick)
            .into_iter()
            .map(|row| DepthsLootRow {
                drop: row.drop,
                item_name: row.item_name.to_owned(),
                line: row.line,
                buttons: row
                    .buttons
                    .into_iter()
                    .map(|button| LootButton {
                        action: match button.action {
                            LootAction::Roll(core::RollChoice::Need) => LootButtonAction::Need,
                            LootAction::Roll(core::RollChoice::Greed) => LootButtonAction::Greed,
                            LootAction::Roll(core::RollChoice::Pass) => LootButtonAction::Pass,
                            LootAction::Claim => LootButtonAction::Claim,
                        },
                        label: button.label.to_owned(),
                        enabled: button.enabled,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The panel views of one projection.
#[must_use]
pub fn views_of(snapshot: &core::ZoneSnapshot, content: &core::ZoneContent) -> PanelViews {
    PanelViews {
        player_auras: aura_rows(&snapshot.auras),
        target_auras: aura_rows(&snapshot.target_detail.auras),
        minimap_dots: minimap::dots(snapshot)
            .into_iter()
            .map(|(entity, class)| MinimapDot {
                entity: entity.into(),
                class: class.into(),
            })
            .collect(),
        disturbance: minimap::disturbance(snapshot, content).map(|area| DisturbanceArea {
            x: area.x,
            z: area.z,
            radius: area.radius,
        }),
        waystone: panels::waystone_offer(snapshot, content).map(|offer| WaystoneOffer {
            id: offer.here.get(),
            name: offer.name.to_owned(),
            destinations: offer
                .destinations
                .into_iter()
                .map(|(id, name)| WaystoneDestination {
                    id: id.get(),
                    name: name.to_owned(),
                })
                .collect(),
            status: offer.status,
        }),
        depths_loot: snapshot
            .depths_loot
            .as_ref()
            .map(|view| depths_loot(view, snapshot.tick)),
    }
}

/// The minimap, unit-frame, waystone and loot views of one projection; the page reads it once
/// per projection, however many frames and panels show it.
#[wasm_bindgen(js_name = panelViews, unchecked_return_type = "PanelViews")]
pub fn panel_views(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    // The run ID is a `bigint`, as on the wire.
    Ok(views_of(&snapshot, &hosted_content()).serialize(
        &serde_wasm_bindgen::Serializer::new()
            .serialize_missing_as_null(true)
            .serialize_large_number_types_as_bigints(true),
    )?)
}

/// What a press of Tab selects in `snapshot`, or `null` without a candidate.
#[wasm_bindgen(js_name = nextTabTarget, unchecked_return_type = "EntityRef | null")]
pub fn next_tab_target(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    to_js(&mmorpg_client_model::targeting::next_tab_target(&snapshot).map(EntityRef::from))
}

/// The glyph each quest marker shows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub struct MarkerGlyphs {
    pub available: String,
    pub complete: String,
    pub in_progress: String,
}

/// The minimap's constants, read once per page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct MinimapRules {
    /// Zoom levels: metres from the centre to the rim, nearest first.
    pub radii_metres: Vec<f32>,
    /// The zoom level a minimap opens at.
    pub default_zoom: usize,
    /// How far from the viewer discovered landmarks show, in metres.
    pub landmark_range_metres: f32,
    /// The scenery prop kinds drawn as building footprints.
    pub building_kinds: Vec<String>,
    pub marker_glyphs: MarkerGlyphs,
}

/// The minimap's constants.
#[wasm_bindgen(js_name = minimapRules)]
pub fn minimap_rules() -> Result<Ts<MinimapRules>, JsError> {
    Ok(MinimapRules {
        radii_metres: minimap::RADII_METRES.to_vec(),
        default_zoom: minimap::DEFAULT_ZOOM,
        landmark_range_metres: minimap::LANDMARK_RANGE_METRES,
        building_kinds: mmorpg_scenery::PropKind::all()
            .iter()
            .filter(|kind| kind.on_minimap())
            .map(|kind| kind.name().to_owned())
            .collect(),
        marker_glyphs: MarkerGlyphs {
            available: quest::marker_glyph(core::QuestMarker::Available).to_owned(),
            complete: quest::marker_glyph(core::QuestMarker::Complete).to_owned(),
            in_progress: quest::marker_glyph(core::QuestMarker::InProgress).to_owned(),
        },
    }
    .into_ts()?)
}

/// `mmorpg_client_model::minimap::offset`: the map offset of a world offset in metres. The page
/// projects each frame's dots itself; its test holds that projection to this one.
#[wasm_bindgen(js_name = minimapOffset)]
#[must_use]
pub fn minimap_offset(dx: f32, dz: f32, heading: f32, heading_up: bool, pixels_per_metre: f32) -> Vec<f32> {
    minimap::offset(dx, dz, heading, heading_up, pixels_per_metre).to_vec()
}

/// A bag stack the vendor may buy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct BagStack {
    pub item_id: u32,
    pub quantity: u16,
}

/// What selling one bag slot's stack does: always the whole stack.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct VendorSale {
    pub slot: u8,
    pub quantity: u16,
    pub value: u32,
    pub text: String,
    pub label: String,
}

/// The sale of each occupied bag slot, in slot order; the page reads it once per bag sheet.
#[wasm_bindgen(js_name = vendorSales, unchecked_return_type = "VendorSale[]")]
pub fn vendor_sales(
    #[wasm_bindgen(unchecked_param_type = "(BagStack | null)[]")] slots: JsValue,
) -> Result<JsValue, JsError> {
    let slots: Vec<Option<BagStack>> = serde_wasm_bindgen::from_value(slots)?;
    let sales: Vec<VendorSale> = slots
        .into_iter()
        .enumerate()
        .filter_map(|(slot, stack)| {
            let stack = stack?;
            let sold = items::sale(core::ItemId::new(stack.item_id), stack.quantity);
            Some(VendorSale {
                slot: u8::try_from(slot).ok()?,
                quantity: sold.quantity,
                value: sold.value,
                text: sold.text,
                label: sold.label,
            })
        })
        .collect();
    to_js(&sales)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projection_without_panels_shows_empty_views() {
        let content = mmorpg_core::greyhaven_vale::content();
        let mut zone =
            mmorpg_core::ZoneSimulation::with_content(core::ZoneId::new(1), content.clone())
                .unwrap();
        zone.add_player(1).unwrap();
        let snapshot = zone.snapshot_for_player(1).unwrap();
        let views = views_of(&snapshot, &content);
        assert!(views.player_auras.buffs.is_empty());
        assert_eq!(views.disturbance, None);
        assert_eq!(views.depths_loot, None);
        assert_eq!(views.minimap_dots.len() + 1, snapshot.entities.len());
    }
}
