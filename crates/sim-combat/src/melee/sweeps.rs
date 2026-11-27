//! Each basic strike's blade sweep (#1249), measured from the clip that shows it with its
//! family's weapon. Generated: `MMORPG_UPDATE_FIXTURES=1 cargo test -p mmorpg-client
//! blade_sweeps` measures the clips again; do not edit by hand.

use super::{BladeSweep, StrikeVariant, WeaponFamily};

/// The sweep of `family`'s `variant`.
#[must_use]
pub const fn blade_sweep(family: WeaponFamily, variant: StrikeVariant) -> BladeSweep {
    use StrikeVariant::{Finisher, First, Jumping, Running, Second};
    use WeaponFamily::{Bow, Dagger, Mace, Spear, Staff, Sword, TwoHanded, Unarmed};
    match (family, variant) {
        // Unarmed_Jab
        (Unarmed, First) => BladeSweep::new(77, 10, 139, 141),
        // Unarmed_Cross
        (Unarmed, Second) => BladeSweep::new(77, 15, 134, 139),
        // Unarmed_Hook
        (Unarmed, Finisher) => BladeSweep::new(57, 54, 138, 145),
        // Unarmed_Run_Strike
        (Unarmed, Running) => BladeSweep::new(28, 12, 117, 144),
        // Unarmed_Jump_Strike
        (Unarmed, Jumping) => BladeSweep::new(73, 26, 96, 163),
        // Sword_Regular_A
        (Sword, First) => BladeSweep::new(125, 37, 42, 221),
        // Sword_Regular_B
        (Sword, Second) => BladeSweep::new(129, 60, 125, 193),
        // Sword_Regular_C
        (Sword, Finisher) => BladeSweep::new(143, 22, 28, 240),
        // Sword_Run_Strike
        (Sword, Running) => BladeSweep::new(125, 55, 65, 226),
        // Sword_Jump_Strike
        (Sword, Jumping) => BladeSweep::new(126, 42, 29, 216),
        // Mace_Swing_A
        (Mace, First) => BladeSweep::new(102, 32, 52, 211),
        // Mace_Swing_B
        (Mace, Second) => BladeSweep::new(97, 64, 96, 177),
        // Mace_Swing_C
        (Mace, Finisher) => BladeSweep::new(110, 24, 49, 228),
        // Mace_Run_Strike
        (Mace, Running) => BladeSweep::new(103, 40, 62, 213),
        // Mace_Jump_Strike
        (Mace, Jumping) => BladeSweep::new(100, 19, 26, 224),
        // Dagger_Stab_R
        (Dagger, First) => BladeSweep::new(106, 23, 115, 123),
        // Dagger_Slash_L
        (Dagger, Second) => BladeSweep::new(85, 65, 78, 175),
        // Dagger_Sinister
        (Dagger, Finisher) => BladeSweep::new(100, 24, 112, 124),
        // Dagger_Run_Strike
        (Dagger, Running) => BladeSweep::new(102, 30, 110, 120),
        // Dagger_Jump_Strike
        (Dagger, Jumping) => BladeSweep::new(86, 37, 54, 201),
        // Staff_Strike_A
        (Staff, First) => BladeSweep::new(162, 9, 158, 164),
        // Staff_Strike_B
        (Staff, Second) => BladeSweep::new(79, 87, 22, 192),
        // Staff_Strike_C
        (Staff, Finisher) => BladeSweep::new(157, 10, 30, 238),
        // Staff_Run_Strike
        (Staff, Running) => BladeSweep::new(162, 90, 118, 192),
        // Staff_Jump_Strike
        (Staff, Jumping) => BladeSweep::new(133, 19, -8, 243),
        // Spear_Thrust_A
        (Spear, First) => BladeSweep::new(203, 2, 115, 124),
        // Spear_Thrust_A
        (Spear, Second) => BladeSweep::new(203, 2, 115, 124),
        // Spear_Thrust_B
        (Spear, Finisher) => BladeSweep::new(224, 5, 130, 154),
        // Spear_Run_Strike
        (Spear, Running) => BladeSweep::new(202, 5, 90, 130),
        // Spear_Jump_Strike
        (Spear, Jumping) => BladeSweep::new(127, 8, -18, 85),
        // Bow_Bash_A
        (Bow, First) => BladeSweep::new(58, 47, 127, 136),
        // Bow_Bash_A
        (Bow, Second) => BladeSweep::new(58, 47, 127, 136),
        // Bow_Bash_B
        (Bow, Finisher) => BladeSweep::new(55, 45, 129, 135),
        // Bow_Run_Strike
        (Bow, Running) => BladeSweep::new(61, 36, 131, 144),
        // Bow_Jump_Strike
        (Bow, Jumping) => BladeSweep::new(60, 36, 98, 165),
        // TwoHand_Swing_A
        (TwoHanded, First) => BladeSweep::new(156, 34, -8, 249),
        // TwoHand_Swing_B
        (TwoHanded, Second) => BladeSweep::new(164, 71, 102, 130),
        // TwoHand_Swing_C
        (TwoHanded, Finisher) => BladeSweep::new(170, 11, -10, 272),
        // TwoHand_Run_Strike
        (TwoHanded, Running) => BladeSweep::new(182, 24, 16, 269),
        // TwoHand_Jump_Strike
        (TwoHanded, Jumping) => BladeSweep::new(170, 25, -23, 279),
    }
}
