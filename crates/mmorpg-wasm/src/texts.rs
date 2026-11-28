//! The browser's words for the zone's answers (#1278): the combat HUD's status and feedback lines
//! and the target's name, read once per projection, and the panels' refusal lines, read once
//! per refusal. `mmorpg-client-model`'s `texts` owns every wording; the native client calls it
//! directly.
use mmorpg_client_model::texts;
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::client_model::{core_snapshot, to_js};
use crate::host::hosted_content;
use crate::wire::{ErrorCode, ZoneSnapshot};

/// The panel a refusal answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum RefusalContext {
    Vendor,
    Loot,
    Bag,
    Equip,
    Unequip,
    Quest,
}

impl From<RefusalContext> for texts::RefusalContext {
    fn from(context: RefusalContext) -> Self {
        match context {
            RefusalContext::Vendor => Self::Vendor,
            RefusalContext::Loot => Self::Loot,
            RefusalContext::Bag => Self::Bag,
            RefusalContext::Equip => Self::Equip,
            RefusalContext::Unequip => Self::Unequip,
            RefusalContext::Quest => Self::Quest,
        }
    }
}

/// What one projection's combat HUD says.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[tsify(missing_as_null)]
#[serde(rename_all = "camelCase")]
pub struct HudTexts {
    /// The viewer's status line.
    pub status: String,
    /// The line of the projection's last event (empty when that event shows on the body
    /// instead), or `null` for a projection without events.
    pub feedback: Option<String>,
    /// The viewer's target's name, or `null` without one.
    pub target_name: Option<String>,
}

/// The combat HUD's lines for one projection; the page reads it once per projection.
#[wasm_bindgen(js_name = hudTexts, unchecked_return_type = "HudTexts")]
pub fn hud_texts(snapshot: Ts<ZoneSnapshot>) -> Result<JsValue, JsError> {
    let snapshot = core_snapshot(&snapshot)?;
    let content = hosted_content();
    to_js(&HudTexts {
        status: texts::status_line(&snapshot, &content),
        feedback: snapshot.events.last().map(|event| {
            texts::event_line(event, &snapshot, &content).map_or_else(String::new, |(_, text)| text)
        }),
        target_name: snapshot
            .viewer
            .target
            .map(|target| texts::unit_name_in(target, &snapshot, &content)),
    })
}

/// What refusal `code` says in the `context` panel, or `null` when that panel ignores it.
#[wasm_bindgen(js_name = panelRefusal)]
pub fn panel_refusal(
    code: Ts<ErrorCode>,
    context: Ts<RefusalContext>,
) -> Result<Option<String>, JsError> {
    Ok(texts::panel_refusal(
        code.to_rust()?.into(),
        context.to_rust()?.into(),
    ))
}
