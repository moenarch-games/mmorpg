//! The front end both clients show before and around the world: which modes
//! the selection screen offers, what each mode's screen is for, what the game
//! menu holds and how each class introduces itself. Both clients render this
//! one description in their own toolkit, so the screens cannot drift apart.
//!
//! Greyhaven's **Characters** screen shapes a lasting character (roster,
//! appearance); Battle Royale's **Loadout** screen picks only what one match
//! needs (class, equipment, a look) and changes no roster character.
//!
//! Nothing here is gameplay: the zone validates every class choice and build.
use mmorpg_core::PlayerClass;
use serde::Serialize;

/// The client drawing the front end. Platforms differ only in what they can
/// do today, never in layout or wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Browser,
    Native,
}

/// What the player chooses to play from the selection screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, strum::VariantArray)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Greyhaven,
    BattleRoyale,
}

/// The selection screen a mode opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Screen {
    /// Create, choose and shape a lasting character.
    Characters,
    /// Pick a match-local build; nothing is saved.
    Loadout,
}

/// One tab of the selection screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeCard {
    pub mode: Mode,
    pub screen: Screen,
    /// The tab label.
    pub label: &'static str,
    /// The screen heading.
    pub heading: &'static str,
    /// One line under the heading saying what the screen is for.
    pub tagline: &'static str,
    /// Why this platform cannot play the mode yet; `None` when it can.
    pub unavailable: Option<&'static str>,
}

/// Every mode, in tab order, as `platform` offers it.
#[must_use]
pub fn modes(platform: Platform) -> [ModeCard; 2] {
    [
        ModeCard {
            mode: Mode::Greyhaven,
            screen: Screen::Characters,
            label: "Greyhaven Vale",
            heading: "Choose your character",
            tagline: "Your characters and how they look",
            unavailable: None,
        },
        ModeCard {
            mode: Mode::BattleRoyale,
            screen: Screen::Loadout,
            label: "Battle Royale",
            heading: "Pick your loadout",
            tagline: "Class, equipment and a look for this match",
            unavailable: match platform {
                Platform::Browser => None,
                Platform::Native => Some("Battle Royale runs in the browser for now."),
            },
        },
    ]
}

/// One game-menu entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MenuAction {
    Resume,
    Settings,
    /// Leave the world for the selection screen of the current mode.
    Leave,
    /// Close the application.
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct MenuEntry {
    pub action: MenuAction,
    pub label: &'static str,
}

/// The game menu (#636) in `mode`: Resume, Settings, then the way out this
/// platform has. The browser returns to the mode's selection screen; the
/// native window claims its character for the whole session, so it quits.
#[must_use]
pub fn game_menu(platform: Platform, mode: Mode) -> Vec<MenuEntry> {
    let mut entries = vec![
        MenuEntry {
            action: MenuAction::Resume,
            label: "Resume",
        },
        MenuEntry {
            action: MenuAction::Settings,
            label: "Settings",
        },
    ];
    entries.push(match platform {
        Platform::Browser => MenuEntry {
            action: MenuAction::Leave,
            label: match mode {
                Mode::Greyhaven => "Characters",
                Mode::BattleRoyale => "Change loadout",
            },
        },
        Platform::Native => MenuEntry {
            action: MenuAction::Quit,
            label: "Quit game",
        },
    });
    entries
}

/// How a class introduces itself on both selection screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ClassCard {
    pub name: &'static str,
    /// A short role for a tile.
    pub role: &'static str,
    /// One sentence for the chosen class.
    pub description: &'static str,
}

#[must_use]
pub const fn class_card(class: PlayerClass) -> ClassCard {
    let (name, role, description) = match class {
        PlayerClass::Warden => (
            "Warden",
            "Armored frontline fighter",
            "A shielded melee fighter who builds rage.",
        ),
        PlayerClass::Ranger => (
            "Ranger",
            "Mobile ranged scout",
            "A ranged archer who spends focus.",
        ),
        PlayerClass::Arcanist => (
            "Arcanist",
            "Arcane ranged spellcaster",
            "A spellcaster who spends mana.",
        ),
        PlayerClass::Necromancer => (
            "Necromancer",
            "Shadow caster who drains life",
            "A shadow caster who drains life and spends mana.",
        ),
        PlayerClass::Rogue => (
            "Rogue",
            "Dagger fighter who poisons and stuns",
            "A dagger fighter who poisons, stuns and throws knives, flasks and bombs with focus.",
        ),
        PlayerClass::Cleric => (
            "Cleric",
            "Holy caster who heals and shields allies",
            "A holy caster who smites, heals and shields allies with faith.",
        ),
        PlayerClass::Druid => (
            "Druid",
            "Nature caster who roots foes and heals allies",
            "A nature caster who roots foes and heals allies with mana.",
        ),
    };
    ClassCard {
        name,
        role,
        description,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantArray;

    #[test]
    fn every_mode_has_its_own_screen_and_only_native_lacks_the_arena() {
        for platform in [Platform::Browser, Platform::Native] {
            let cards = modes(platform);
            assert_eq!(
                cards.map(|card| card.mode).as_slice(),
                Mode::VARIANTS,
                "tabs follow the mode order"
            );
            assert_eq!(cards[0].screen, Screen::Characters);
            assert_eq!(cards[1].screen, Screen::Loadout);
            assert!(cards[0].unavailable.is_none());
        }
        assert!(modes(Platform::Browser)[1].unavailable.is_none());
        assert!(modes(Platform::Native)[1].unavailable.is_some());
    }

    #[test]
    fn the_game_menu_differs_only_in_its_way_out() {
        let labels = |platform, mode| {
            game_menu(platform, mode)
                .iter()
                .map(|entry| entry.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(Platform::Browser, Mode::Greyhaven),
            ["Resume", "Settings", "Characters"]
        );
        assert_eq!(
            labels(Platform::Browser, Mode::BattleRoyale),
            ["Resume", "Settings", "Change loadout"]
        );
        assert_eq!(
            labels(Platform::Native, Mode::Greyhaven),
            ["Resume", "Settings", "Quit game"]
        );
    }

    #[test]
    fn every_class_introduces_itself() {
        for &class in PlayerClass::VARIANTS {
            let card = class_card(class);
            assert!(!card.name.is_empty() && !card.role.is_empty());
            assert!(card.description.ends_with('.'));
        }
    }
}
