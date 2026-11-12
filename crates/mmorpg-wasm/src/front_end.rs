//! The browser's thin wrappers over `mmorpg_client_model::front_end`: the
//! selection screen's modes, the game menu and the class cards, so the browser
//! and the native client draw one front end.
use mmorpg_client_model::front_end::{self as model, Platform};
use serde::{Deserialize, Serialize};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

use crate::wire::ClassId;

/// A selection-screen mode, as the page's `?activity=` names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "kebab-case")]
pub enum FrontEndMode {
    #[serde(rename = "world")]
    Greyhaven,
    BattleRoyale,
}

impl From<model::Mode> for FrontEndMode {
    fn from(mode: model::Mode) -> Self {
        match mode {
            model::Mode::Greyhaven => Self::Greyhaven,
            model::Mode::BattleRoyale => Self::BattleRoyale,
        }
    }
}

impl From<FrontEndMode> for model::Mode {
    fn from(mode: FrontEndMode) -> Self {
        match mode {
            FrontEndMode::Greyhaven => Self::Greyhaven,
            FrontEndMode::BattleRoyale => Self::BattleRoyale,
        }
    }
}

/// One tab of the selection screen.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(missing_as_null)]
pub struct ModeCard {
    pub mode: FrontEndMode,
    /// `characters` or `loadout`.
    pub screen: String,
    pub label: String,
    pub heading: String,
    pub tagline: String,
}

/// The selection screen's modes, in tab order.
#[wasm_bindgen(js_name = frontEndModes, unchecked_return_type = "ModeCard[]")]
pub fn front_end_modes() -> Result<JsValue, JsError> {
    let cards: Vec<ModeCard> = model::modes(Platform::Browser)
        .into_iter()
        .filter(|card| card.unavailable.is_none())
        .map(|card| ModeCard {
            mode: card.mode.into(),
            screen: match card.screen {
                model::Screen::Characters => "characters",
                model::Screen::Loadout => "loadout",
            }
            .to_owned(),
            label: card.label.to_owned(),
            heading: card.heading.to_owned(),
            tagline: card.tagline.to_owned(),
        })
        .collect();
    crate::client_model::to_js(&cards)
}

/// One game-menu entry: `resume`, `settings` or `leave` in the browser.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct GameMenuEntry {
    pub action: String,
    pub label: String,
}

/// The game menu in `mode`.
#[wasm_bindgen(js_name = gameMenu, unchecked_return_type = "GameMenuEntry[]")]
pub fn game_menu(mode: Ts<FrontEndMode>) -> Result<JsValue, JsError> {
    let entries: Vec<GameMenuEntry> = model::game_menu(Platform::Browser, mode.to_rust()?.into())
        .into_iter()
        .map(|entry| GameMenuEntry {
            action: match entry.action {
                model::MenuAction::Resume => "resume",
                model::MenuAction::Settings => "settings",
                model::MenuAction::Leave => "leave",
                model::MenuAction::Quit => "quit",
            }
            .to_owned(),
            label: entry.label.to_owned(),
        })
        .collect();
    crate::client_model::to_js(&entries)
}

/// How a class introduces itself on a selection screen.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Tsify)]
pub struct ClassCard {
    pub name: String,
    pub role: String,
    pub description: String,
}

#[wasm_bindgen(js_name = classCard)]
pub fn class_card(class: Ts<ClassId>) -> Result<Ts<ClassCard>, JsError> {
    let card = model::class_card(class.to_rust()?.into());
    Ok(ClassCard {
        name: card.name.to_owned(),
        role: card.role.to_owned(),
        description: card.description.to_owned(),
    }
    .into_ts()?)
}
