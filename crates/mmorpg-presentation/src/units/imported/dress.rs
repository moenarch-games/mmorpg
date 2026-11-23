//! What a Ranger figure wears besides its loadout (#374, #376, #1059): authored headwear on the
//! head's armour slot and a hairstyle on its hair slot, the one table both clients dress from.
//! Items name their slot (socket) and come in the bundle's shared tintable materials, so a look
//! is a choice per slot, never a per-figure bundle.

use super::catalogue::{Attachment, Figure};
pub use crate::units::player::Hat;

/// The slot authored headwear rides.
pub const HEADWEAR_SOCKET: &str = "head.armor";
/// The slot a hairstyle rides.
pub const HAIR_SOCKET: &str = "head.hair";
/// The tintable texture variant every hairstyle is fetched in.
pub const HAIR_VARIANT: &str = "hair-1";
/// The figure the authored headwear is fitted to; the other figures go without it.
pub const HEADWEAR_FIGURE: Figure = Figure::Male;
/// Headwear residents wear through their presets rather than the viewer's choice: the farmer's
/// straw hat (#418).
pub const RESIDENT_HEADWEAR: [Attachment; 1] = [Attachment {
    item: "equipment.hat.straw",
    socket: HEADWEAR_SOCKET,
}];

impl Hat {
    /// Every headwear choice, in the order the character creator offers them.
    pub const ALL: [Self; 4] = [Self::Class, Self::Hood, Self::Cap, Self::Helm];

    /// The name both clients key the choice by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Hood => "wayfarer-hood",
            Self::Cap => "ranger-cap",
            Self::Helm => "ironcrest-helm",
        }
    }

    /// The choice named `name`.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hat| hat.name() == name)
    }

    /// The authored items worn for this choice on [`HEADWEAR_SOCKET`]: none for the bundle's
    /// own hood or the class outfit's headwear (#379).
    #[must_use]
    pub const fn attachments(self) -> &'static [Attachment] {
        match self {
            Self::Class | Self::Hood => &[],
            Self::Cap => &[Attachment {
                item: "equipment.cap.ranger",
                socket: HEADWEAR_SOCKET,
            }],
            Self::Helm => &[Attachment {
                item: "equipment.helm.ironcrest",
                socket: HEADWEAR_SOCKET,
            }],
        }
    }
}

/// A bundle hairstyle (#376), worn on [`HAIR_SOCKET`] in [`HAIR_VARIANT`] and tinted by the
/// look; no hairstyle is bald.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HairStyle {
    Buzzed,
    Parted,
    Long,
}

impl HairStyle {
    /// Every hairstyle, in the order the character creator offers them.
    pub const ALL: [Self; 3] = [Self::Buzzed, Self::Parted, Self::Long];

    /// The name both clients key the style by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Buzzed => "buzzed",
            Self::Parted => "parted",
            Self::Long => "long",
        }
    }

    /// Its `humanoid-appearance` hair entry.
    #[must_use]
    pub const fn entry(self) -> &'static str {
        match self {
            Self::Buzzed => "hair.buzzed",
            Self::Parted => "hair.simple-parted",
            Self::Long => "hair.long",
        }
    }
}

/// The starter hairstyle (#376), the native client's only one.
pub const STARTER_HAIR: HairStyle = HairStyle::Parted;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headwear_names_round_trip_and_items_ride_the_head_slot() {
        for hat in Hat::ALL {
            assert_eq!(Hat::of_name(hat.name()), Some(hat));
            assert!(
                hat.attachments()
                    .iter()
                    .all(|at| at.socket == HEADWEAR_SOCKET)
            );
        }
        assert_eq!(Hat::of_name("none"), None);
        assert_eq!(Hat::Cap.attachments()[0].item, "equipment.cap.ranger");
        assert_eq!(STARTER_HAIR.entry(), "hair.simple-parted");
    }
}
