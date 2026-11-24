//! The catalogue: what an imported unit wears, carries and holds, and which resident or figure
//! it is.

use super::GreatWeapon;
use mmorpg_core::{Equipment, EquipmentSlot, NamedWeapon, PlayerClass, WeaponFamily};
use strum::VariantArray;

/// What an imported humanoid carries in its sockets: the class weapons and props (#156, #244,
/// #302, asset-tooling #375 and #379) and the guards' sword.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::VariantArray)]
pub enum Loadout {
    /// The Warden's arming sword and heater shield (#156).
    SwordAndShield,
    /// The arming sword and the Redbrand round shield (#953, asset-tooling #774): the Redbrand
    /// Brute.
    SwordAndRedbrandShield,
    /// The Ranger's longbow and quiver on its back (#244).
    BowAndQuiver,
    /// The arming sword alone: the Greyhaven guards.
    Sword,
    /// One blackened dagger in the right hand: the field marauders.
    Dagger,
    /// A blackened dagger in each hand: the Rogue ([`ROGUE_LOADOUT`], #603).
    TwinDaggers,
    /// The Necromancer's bone staff.
    BoneStaff,
    /// The Arcanist's crystal staff in the right hand and tome in the left (asset-tooling #379).
    CrystalStaffAndTome,
    /// The Cleric's flanged mace (asset-tooling #379).
    FlangedMace,
    /// The Druid's gnarled staff.
    GnarledStaff,
    /// The Redbrand Crossbowmen's crossbow on [`CROSSBOW_SOCKET`], with a bolt (#607).
    Crossbow,
    /// The leaf-bladed spear in both hands (#914): a player whose weapon family is the spear.
    Spear,
    /// Garrick Redbrand's named cleaver in the right hand (#918): a player wielding it.
    GarricksCleaver,
    /// The smith's Hollowpine boar-spear in both hands (#917): a player wielding it.
    HollowpineSpear,
    /// The great weapons in both hands (#1073, asset-tooling #823): maul, great hammer, great
    /// axe and greatsword ([`GreatWeapon`]), held by a player whose projected great weapon is one.
    Maul,
    GreatHammer,
    GreatAxe,
    Greatsword,
}

impl Loadout {
    /// The weapons and props a class carries.
    #[must_use]
    pub const fn of_class(class: PlayerClass) -> Self {
        match class {
            PlayerClass::Warden => Self::SwordAndShield,
            PlayerClass::Ranger => Self::BowAndQuiver,
            PlayerClass::Arcanist => Self::CrystalStaffAndTome,
            PlayerClass::Rogue => ROGUE_LOADOUT,
            PlayerClass::Necromancer => Self::BoneStaff,
            PlayerClass::Cleric => Self::FlangedMace,
            PlayerClass::Druid => Self::GnarledStaff,
        }
    }

    /// The name both clients key their socket items by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SwordAndShield => "sword-and-shield",
            Self::SwordAndRedbrandShield => "sword-and-shield.redbrand",
            Self::BowAndQuiver => "bow-and-quiver",
            Self::Sword => "sword",
            Self::Dagger => "dagger",
            Self::TwinDaggers => "dagger.twin",
            Self::BoneStaff => "staff.bone",
            Self::CrystalStaffAndTome => "staff.crystal-and-tome",
            Self::FlangedMace => "mace.flanged",
            Self::GnarledStaff => "staff.gnarled",
            Self::Crossbow => "crossbow",
            Self::Spear => "spear",
            Self::GarricksCleaver => "sword.garricks-cleaver",
            Self::HollowpineSpear => "spear.hollowpine-hunter",
            Self::Maul => "warhammer.maul",
            Self::GreatHammer => "hammer.great",
            Self::GreatAxe => "axe.great",
            Self::Greatsword => "sword.great",
        }
    }

    /// What a player carries whose projected melee family is `weapon` (#914): its great weapon
    /// in both hands (#1073), a named weapon's own model (#918), a spear in both hands for the
    /// spear family, otherwise the class's own weapons.
    #[must_use]
    pub const fn holding(
        self,
        weapon: Option<WeaponFamily>,
        named: Option<NamedWeapon>,
        great: Option<mmorpg_core::GreatWeapon>,
    ) -> Self {
        use mmorpg_core::GreatWeapon as Great;
        match (great, named, weapon) {
            (Some(Great::Maul), ..) => Self::Maul,
            (Some(Great::GreatHammer), ..) => Self::GreatHammer,
            (Some(Great::GreatAxe), ..) => Self::GreatAxe,
            (Some(Great::Greatsword), ..) => Self::Greatsword,
            (None, Some(NamedWeapon::GarricksCleaver), _) => Self::GarricksCleaver,
            (None, Some(NamedWeapon::HollowpineHunter), _) => Self::HollowpineSpear,
            (None, None, Some(WeaponFamily::Spear)) => Self::Spear,
            _ => self,
        }
    }

    /// What each hand holds, by grip role (asset-tooling #285). The Ranger's bow rests on its
    /// back and comes to hand only through the bow clips, which curl their own fingers (#556),
    /// so its hands hold nothing here. The crossbow is held in both hands, the right on the
    /// stock's wrist and the left on the fore-stock (#607).
    #[must_use]
    pub const fn hands(self) -> Hands {
        let (left, right) = match self {
            Self::SwordAndShield | Self::SwordAndRedbrandShield => {
                (Some(Held::Shield), Some(Held::Weapon))
            }
            Self::BowAndQuiver => (None, None),
            Self::Sword | Self::Dagger | Self::FlangedMace | Self::GarricksCleaver => {
                (None, Some(Held::Weapon))
            }
            Self::TwinDaggers => (Some(Held::Weapon), Some(Held::Weapon)),
            Self::BoneStaff | Self::GnarledStaff => (None, Some(Held::Staff)),
            Self::CrystalStaffAndTome => (Some(Held::Book), Some(Held::Staff)),
            Self::Crossbow => (Some(Held::Crossbow), Some(Held::Crossbow)),
            Self::Spear
            | Self::HollowpineSpear
            | Self::Maul
            | Self::GreatHammer
            | Self::GreatAxe
            | Self::Greatsword => (Some(Held::Staff), Some(Held::Staff)),
        };
        Hands { left, right }
    }

    /// Whether the loadout fights with daggers and so plays the dagger clips (#603).
    #[must_use]
    pub const fn daggers(self) -> bool {
        matches!(self, Self::Dagger | Self::TwinDaggers)
    }

    /// The great weapon the loadout carries in both hands, if any (#1073).
    #[must_use]
    pub const fn great_weapon(self) -> Option<GreatWeapon> {
        match self {
            Self::Maul => Some(GreatWeapon::Maul),
            Self::GreatHammer => Some(GreatWeapon::GreatHammer),
            Self::GreatAxe => Some(GreatWeapon::GreatAxe),
            Self::Greatsword => Some(GreatWeapon::Greatsword),
            _ => None,
        }
    }

    /// The bundle items the loadout wears and the sockets they ride (#1012), as both clients
    /// attach them: the one place a loadout's asset/socket pairs are authored. The stowed bow
    /// rests on the back; the crossbow carries the arrow that stands in for its bolt (#607).
    #[must_use]
    pub const fn attachments(self) -> &'static [Attachment] {
        const SWORD: &str = "equipment.sword.arming";
        const DAGGER: &str = "equipment.dagger.blackened";
        match self {
            Self::SwordAndShield => &[
                Attachment {
                    item: SWORD,
                    socket: "hand_r.weapon",
                },
                Attachment {
                    item: "equipment.shield.heater",
                    socket: "hand_l.shield",
                },
            ],
            // The Redbrand Brute's round plank shield (#953, asset-tooling #774).
            Self::SwordAndRedbrandShield => &[
                Attachment {
                    item: SWORD,
                    socket: "hand_r.weapon",
                },
                Attachment {
                    item: "equipment.shield.redbrand",
                    socket: "hand_l.shield",
                },
            ],
            Self::BowAndQuiver => &[
                Attachment {
                    item: "equipment.bow.long",
                    socket: "back.bow.stowed",
                },
                Attachment {
                    item: "equipment.quiver.back",
                    socket: "back.quiver",
                },
            ],
            Self::Sword => &[Attachment {
                item: SWORD,
                socket: "hand_r.weapon",
            }],
            Self::Dagger => &[Attachment {
                item: DAGGER,
                socket: "hand_r.weapon",
            }],
            Self::TwinDaggers => &[
                Attachment {
                    item: DAGGER,
                    socket: "hand_r.weapon",
                },
                Attachment {
                    item: DAGGER,
                    socket: "hand_l.weapon",
                },
            ],
            Self::BoneStaff => &[Attachment {
                item: "equipment.staff.bone",
                socket: "hand_r.staff",
            }],
            // The tome is held like a shield (asset-tooling #379): spine along the forearm.
            Self::CrystalStaffAndTome => &[
                Attachment {
                    item: "equipment.staff.crystal",
                    socket: "hand_r.staff",
                },
                Attachment {
                    item: "equipment.book.tome",
                    socket: "hand_l.shield",
                },
            ],
            Self::FlangedMace => &[Attachment {
                item: "equipment.mace.flanged",
                socket: "hand_r.weapon",
            }],
            Self::GnarledStaff => &[Attachment {
                item: "equipment.staff.gnarled",
                socket: "hand_r.staff",
            }],
            // A player of the spear family (#914).
            Self::Spear => &[Attachment {
                item: "equipment.spear.leaf",
                socket: "hand_r.staff",
            }],
            // Garrick Redbrand's named cleaver (#918, asset-tooling #761).
            Self::GarricksCleaver => &[Attachment {
                item: "equipment.sword.garricks-cleaver",
                socket: "hand_r.weapon",
            }],
            // The smith's Hollowpine boar-spear (#917).
            Self::HollowpineSpear => &[Attachment {
                item: "equipment.spear.hollowpine-hunter",
                socket: "hand_r.staff",
            }],
            // A great weapon rides the right hand's weapon socket, as its run clips carry it
            // (asset-tooling #823); the clip brings the left hand to the haft.
            Self::Maul => &[Attachment {
                item: "equipment.warhammer.maul",
                socket: "hand_r.weapon",
            }],
            Self::GreatHammer => &[Attachment {
                item: "equipment.hammer.great",
                socket: "hand_r.weapon",
            }],
            Self::GreatAxe => &[Attachment {
                item: "equipment.axe.great",
                socket: "hand_r.weapon",
            }],
            Self::Greatsword => &[Attachment {
                item: "equipment.sword.great",
                socket: "hand_r.weapon",
            }],
            // The crossbow and the arrow standing in for its bolt (#607).
            Self::Crossbow => &[
                Attachment {
                    item: CROSSBOW_ITEM,
                    socket: CROSSBOW_SOCKET,
                },
                Attachment {
                    item: BOLT_ITEM,
                    socket: BOLT_HAND_SOCKET,
                },
            ],
        }
    }
}

/// One bundle item a [`Loadout`] wears (#1012): its `humanoid-equipment` entry key and the
/// figure's `humanoid-sockets` socket it rides.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attachment {
    /// The `humanoid-equipment` entry key, e.g. `equipment.sword.arming`.
    pub item: &'static str,
    /// The socket, e.g. `hand_r.weapon`.
    pub socket: &'static str,
}

/// The item standing in for the crossbow's bolt on [`BOLT_HAND_SOCKET`] (#607), scaled by
/// [`BOLT_SCALE`].
pub const BOLT_ITEM: &str = "equipment.arrow";

/// The Rogue's weapons (#603): a dagger in each hand, as asset-tooling #418 authored the
/// Rogue's clips. Dual-wielding awaits the owner's confirmation (question on #603); set this to
/// [`Loadout::Dagger`] for a single-dagger Rogue, whose auto-attack then only stabs.
pub const ROGUE_LOADOUT: Loadout = Loadout::TwinDaggers;

/// Where a great weapon rides while sheathed (#1073): across the back, as `TwoHand_Draw` and
/// `TwoHand_Sheathe` author its `stowSocket`.
pub const GREAT_WEAPON_STOW_SOCKET: &str = "back.stowed";

/// Where `item`, held on the `hand` socket, rides while its holder's weapon is sheathed (#802):
/// a great weapon across the back (#1073), another hand weapon at that side's hip, a shield,
/// staff or crossbow on the back; `None` keeps it where it is. Both clients stow by it.
#[must_use]
pub fn stow_socket(item: &str, hand: &str) -> Option<&'static str> {
    if GreatWeapon::VARIANTS
        .iter()
        .any(|great| great.item() == item)
    {
        return Some(GREAT_WEAPON_STOW_SOCKET);
    }
    match hand {
        "hand_r.weapon" => Some("hip_r.stowed"),
        "hand_l.weapon" => Some("hip_l.stowed"),
        "hand_l.shield" | "hand_r.staff" | CROSSBOW_SOCKET => Some("back.stowed"),
        _ => None,
    }
}

/// Where a sheathed dagger rides (asset-tooling #418), left hand then right: through a throw
/// the right one, through a bandage both ([`Carry`]).
pub const DAGGER_SHEATH_SOCKETS: [&str; 2] = ["hip_l.stowed", "hip_r.stowed"];
/// The socket a drawn throwable rides in (asset-tooling #418).
pub const THROWABLE_SOCKET: &str = "hand_r.weapon";
/// The socket a throw's item waits on until the clip's draw (asset-tooling #418): the right
/// hand's grip frame at `drawSeconds`, so the hand takes it where it hangs.
pub const THROWABLE_BELT_SOCKET: &str = "belt.throwable";

/// A thrown weapon in the right hand (#603), drawn from the belt for a throw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Throwable {
    /// The Throwing Knife.
    Knife,
    /// The Venom Flask.
    Flask,
    /// The Blasting Bomb.
    Bomb,
}

impl Throwable {
    /// The name both clients key the throwable by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Knife => "knife",
            Self::Flask => "flask",
            Self::Bomb => "bomb",
        }
    }

    /// The right hand's grip pose round it (asset-tooling #418).
    #[must_use]
    pub const fn grip(self) -> &'static str {
        match self {
            Self::Knife => "Grip_Knife_Pinch_R",
            Self::Flask | Self::Bomb => "Grip_Flask_R",
        }
    }

    /// Its bundle item (asset-tooling #467), whose origin is its grip frame.
    #[must_use]
    pub const fn item(self) -> &'static str {
        match self {
            Self::Knife => "equipment.throwing-knife",
            Self::Flask => "equipment.venom-flask",
            Self::Bomb => "equipment.blasting-bomb",
        }
    }

    /// Every throwable.
    pub const ALL: [Self; 3] = [Self::Knife, Self::Flask, Self::Bomb];
}

/// Where a dagger holder's daggers ride and what its right hand throws this frame (#603): a
/// throw shows its item on the belt until the draw, sheathes the right dagger from its draw to
/// the clip's end and holds the item from the draw to the release; a bandage sheathes both.
/// Everything else keeps the daggers in hand.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Carry {
    /// Explicit player weapon mode; `None` preserves non-player clip-authored carry (#802).
    pub weapon_drawn: Option<bool>,
    /// Each hand's dagger sheathed on [`DAGGER_SHEATH_SOCKETS`], left then right.
    pub sheathed: [bool; 2],
    /// The throwable in [`THROWABLE_SOCKET`].
    pub throwable: Option<Throwable>,
    /// A throw's item on [`THROWABLE_BELT_SOCKET`] before the clip's draw (#611).
    pub belted: Option<Throwable>,
    /// Where a crossbowman's crossbow and bolt ride (#607); `None` for anyone else.
    pub crossbow: Option<CrossbowCarry>,
    /// A resident's chore item on its socket (#904), while its chore clips (or the ordinary
    /// idle and walk) play; `None` keeps the hands empty.
    pub chore: Option<crate::units::chores::Chore>,
}

/// The crossbowman's crossbow (#607).
pub const CROSSBOW_ITEM: &str = "equipment.crossbow";
/// Where the crossbow rides: the right hand's [`CROSSBOW_SOCKET`] (#607).
pub const CROSSBOW_SOCKET: &str = "hand_r.crossbow";
/// The joint the crossbow rides through the reload's `leftHandSeconds`, kept where it was in the
/// world when the hand-off began (asset-tooling #419).
pub const CROSSBOW_LEFT_JOINT: &str = "hand_l";
/// The socket the bolt rides from the reload's `boltDrawnSeconds` to its `reloadedSeconds`.
pub const BOLT_HAND_SOCKET: &str = "hand_r.arrow";
/// The bundle has no bolt item: the arrow (`equipment.arrow`, 0.76 m from its nock at the origin
/// along +Z) scaled by this stands in for one (#607) …
pub const BOLT_SCALE: f32 = 0.46;
/// … and lies in the groove at this rigid transform in the crossbow's grip frame (translation
/// in metres, rotation `[x, y, z, w]`): where the reload's right hand lays it down at
/// `reloadedSeconds` on the male figure, lifted 1.5 cm out of the stock onto it (the native
/// client's `a_reload_hands_the_crossbow_off_where_it_was_and_lays_the_bolt_in_the_groove`
/// checks it).
pub const BOLT_GROOVE: ([f32; 3], [f32; 4]) = ([0.006, 0.028, 0.054], [0., 0.0118, 0.0275, 0.9996]);
/// The left hand's grip on the crank while the reload winds it (`crankSeconds`).
pub const CRANK_GRIP: &str = "Grip_Crank_L";

/// A crossbowman's crossbow this frame (#607): through the reload's `leftHandSeconds` it rides
/// the left hand, the right drawing the next bolt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CrossbowCarry {
    /// The crossbow rides [`CROSSBOW_LEFT_JOINT`] instead of [`CROSSBOW_SOCKET`].
    pub left_hand: bool,
    /// The left hand winds the crank ([`CRANK_GRIP`]).
    pub crank: bool,
    /// Where the bolt is; `None` between the shot and the draw of the next.
    pub bolt: Option<Bolt>,
}

/// Where a crossbowman's bolt is (#607).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bolt {
    /// In the right hand's [`BOLT_HAND_SOCKET`], drawn from over the shoulder.
    Hand,
    /// Laid in the crossbow's groove ([`BOLT_GROOVE`]), loaded.
    Groove,
}

impl Bolt {
    /// The name both clients key it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hand => "hand",
            Self::Groove => "groove",
        }
    }
}

/// A hand, for its finger joints and grip pose (asset-tooling #285).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    pub const BOTH: [Self; 2] = [Self::Left, Self::Right];

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    /// The hand whose finger or thumb `joint` is (`thumb_01_l` … `pinky_04_leaf_r`).
    #[must_use]
    pub fn of_finger(joint: &str) -> Option<Self> {
        const FINGERS: [&str; 5] = ["thumb_", "index_", "middle_", "ring_", "pinky_"];
        if !FINGERS.iter().any(|finger| joint.starts_with(finger)) {
            return None;
        }
        if joint.ends_with("_l") {
            Some(Self::Left)
        } else if joint.ends_with("_r") {
            Some(Self::Right)
        } else {
            None
        }
    }
}

/// The grip role of an item in hand: which `Grip_*` pose closes the fingers round it
/// (asset-tooling #285, `grip.role`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Held {
    /// A one-hand weapon's grip: sword, dagger or mace.
    Weapon,
    /// The heater shield's hand handle.
    Shield,
    /// The tome, resting on the back of a relaxed hand.
    Book,
    /// A staff's shaft.
    Staff,
    /// The crossbow's stock: the wrist in the right hand, the fore-stock in the left (#607).
    Crossbow,
}

impl Held {
    /// The bundle's grip pose for this item in `hand`.
    #[must_use]
    pub const fn grip(self, hand: Hand) -> &'static str {
        match (self, hand) {
            (Self::Weapon, Hand::Left) => "Grip_Weapon_L",
            (Self::Weapon, Hand::Right) => "Grip_Weapon_R",
            (Self::Shield, Hand::Left) => "Grip_Shield_L",
            (Self::Shield, Hand::Right) => "Grip_Shield_R",
            (Self::Book, Hand::Left) => "Grip_Book_L",
            (Self::Book, Hand::Right) => "Grip_Book_R",
            (Self::Staff, Hand::Left) => "Grip_Staff_L",
            (Self::Staff, Hand::Right) => "Grip_Staff_R",
            (Self::Crossbow, Hand::Left) => "Grip_Crossbow_L",
            (Self::Crossbow, Hand::Right) => "Grip_Crossbow_R",
        }
    }
}

/// What a unit holds in each hand.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Hands {
    pub left: Option<Held>,
    pub right: Option<Held>,
}

impl Hands {
    #[must_use]
    pub const fn get(self, hand: Hand) -> Option<Held> {
        match hand {
            Hand::Left => self.left,
            Hand::Right => self.right,
        }
    }
}

/// The armour slots an equipped drop covers, and the `humanoid-sockets` armour sockets whose
/// class outfit pieces it replaces. Hands, forearms and the belt belong to no slot and stay.
pub const ARMOR_SOCKETS: [(EquipmentSlot, &[&str]); 4] = [
    (EquipmentSlot::Head, &["head.armor"]),
    (
        EquipmentSlot::Chest,
        &[
            "spine_03.armor",
            "clavicle_l.armor",
            "clavicle_r.armor",
            "upperarm_l.armor",
            "upperarm_r.armor",
        ],
    ),
    (
        EquipmentSlot::Legs,
        &[
            "thigh_l.armor",
            "thigh_r.armor",
            "calf_l.armor",
            "calf_r.armor",
        ],
    ),
    (EquipmentSlot::Feet, &["foot_l.armor", "foot_r.armor"]),
];

/// The bundle outfit whose armour pieces draw an equipped drop. Catalog items carry no art of
/// their own, so every drop wears the knight's plate on the sockets its slot covers.
pub const DROP_ARMOR_OUTFIT: &str = "outfit.knight";

/// A player's class starter outfit (asset-tooling #379): the Ranger bundle's `class.<id>`
/// preset, its `classTrim` in the class colour, and the armour slots an equipped drop covers.
/// Drops replace the outfit's pieces on their sockets; the trimmed tabard, sash or robe stays,
/// so the class still reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Outfit {
    pub class: PlayerClass,
    /// Which of [`ARMOR_SOCKETS`] hold an equipped item. Only the viewer's equipment is
    /// projected, so everyone else wears the starter outfit whole.
    pub worn: [bool; 4],
}

impl Outfit {
    /// The class's starter outfit, `equipment` (the viewer's) covering its slots.
    #[must_use]
    pub fn new(class: PlayerClass, equipment: Option<&Equipment>) -> Self {
        Self {
            class,
            worn: ARMOR_SOCKETS
                .map(|(slot, _)| equipment.is_some_and(|equipment| equipment.item(slot).is_some())),
        }
    }

    /// The bundle's `humanoid-appearance.presets` id.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self.class {
            PlayerClass::Warden => "class.warden",
            PlayerClass::Ranger => "class.ranger",
            PlayerClass::Arcanist => "class.arcanist",
            PlayerClass::Necromancer => "class.necromancer",
            PlayerClass::Rogue => "class.rogue",
            PlayerClass::Cleric => "class.cleric",
            PlayerClass::Druid => "class.druid",
        }
    }

    /// The armour sockets whose outfit pieces the equipped drops replace.
    #[must_use]
    pub fn replaced(self) -> Vec<&'static str> {
        ARMOR_SOCKETS
            .iter()
            .zip(self.worn)
            .filter(|(_, worn)| *worn)
            .flat_map(|((_, sockets), _)| sockets.iter().copied())
            .collect()
    }

    /// The class colour on the outfit's `classTrim` materials as a linear base colour: the
    /// class accent ([`crate::units::player::Look::shoulder`], `shoulderColor` in the browser's
    /// `character-visuals.ts`).
    #[must_use]
    pub fn trim(self) -> [f32; 3] {
        let hex = crate::units::player::Look::new(self.class.code(), false, None, false).shoulder;
        let linear = |byte: u32| {
            let c = (byte & 0xff) as f32 / 255.;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        [linear(hex >> 16), linear(hex >> 8), linear(hex)]
    }
}

/// The Ranger figure a character is drawn on. Each is its own asset-tooling bundle with its own
/// skeleton, clips, sockets and `humanoid-appearance`: the male carries the players, the class
/// outfits and the male residents and enemies; the female (asset-tooling #378) carries only her
/// own presets, with no headwear, beards, head skins or variety options.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
pub enum Figure {
    Male,
    Female,
}

impl Figure {
    /// The bundle's character key, also its acquisition directory under `characters/`.
    #[must_use]
    pub const fn character(self) -> &'static str {
        match self {
            Self::Male => "humanoid.ranger.male",
            Self::Female => "humanoid.ranger.female",
        }
    }

    /// The figure whose character key is `character`.
    #[must_use]
    pub fn of_character(character: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|figure| figure.character() == character)
    }
}

/// A character drawn on a Ranger figure in a `humanoid-appearance` preset: a named Greyhaven
/// resident in the bundle's own preset (#418, #493), or a humanoid enemy in a game-authored
/// preset over the male bundle's outfits (#39; [`crate::units::presets::enemy_presets`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::VariantArray)]
pub enum Resident {
    Villager,
    VillagerWoman,
    Marshal,
    Guard,
    Innkeeper,
    Farmer,
    Healer,
    /// Tanner Hilda Brook (#493), on the female figure.
    Tanner,
    /// The Millbrook field marauders: ragged farmhands turned raiders.
    Marauder,
    /// The Redbrand rank and file in the band's red.
    RedbrandBandit,
    /// The Greyfang outriders (#286), with crossbows (#607).
    RedbrandCrossbowman,
    /// The Greyfang cavern's shield-bearers (#160): plate, helm, sword and shield.
    RedbrandBrute,
    /// Captain Ulric Thorn (#404), a recoloured Garrick: the depths' veteran in dark plate.
    Thorn,
    /// Garrick Redbrand (#276), the band's bareheaded leader in plate.
    Garrick,
}

impl Resident {
    /// The resident an NPC of this canonical name is, if it has a preset.
    #[must_use]
    pub fn of_name(name: &str) -> Option<Self> {
        match name {
            "Marshal Elden Greywatch" => Some(Self::Marshal),
            "Greyhaven Guard" | "Watchman Garth" | "Watchman Pell" | "Guardsman Tam Hollis" => {
                Some(Self::Guard)
            }
            "Innkeeper Bram Tolliver" => Some(Self::Innkeeper),
            "Farmer Osric Mill" => Some(Self::Farmer),
            "Brother Aldous" => Some(Self::Healer),
            "Tanner Hilda Brook" => Some(Self::Tanner),
            "Weaver Lysa Reed" | "Mara Wells" | "Baker Anwen" | "Herbalist Nessa"
            | "Fisher Ada" | "Washer Elin"
            // The region's people (#819).
            | "Alewife Gwen Barrow" | "Old Maud" | "Pip" | "Netter Ilsa" | "Trapper Sela"
            | "Goodwife Ena" | "Shepherd Lise" | "Miller Hanne" | "Trader Maren"
            // Millbrook's farmfolk (#1019).
            | "Marta Mill" | "Farmhand Edda" => {
                Some(Self::VillagerWoman)
            }
            "Carpenter Owen Alder"
            | "Davin Moss"
            | "Miller Tomas"
            | "Peddler Rowan"
            | "Porter Ben"
            | "Apprentice Finn"
            | "Shepherd Corin"
            | "Farmer Tobin Hale"
            | "Innkeeper Hob Ferris"
            | "Miller Wynne"
            | "Ferryman Osk"
            | "Fisher Brann"
            | "Huntsman Edric Vale"
            | "Ploughman Garr"
            | "Old Bertil"
            | "Farmhand Jory"
            | "Smith Wulfric Hale"
            | "Courier Wat Ferrow"
            | "Farmhand Cole" => Some(Self::Villager),
            _ => None,
        }
    }

    /// The humanoid enemy a creature template of this name is, if it has a preset.
    #[must_use]
    pub fn of_creature(name: &str) -> Option<Self> {
        match name {
            "Field Marauder" => Some(Self::Marauder),
            "Redbrand Bandit" | "Redbrand Courier" => Some(Self::RedbrandBandit),
            "Redbrand Crossbowman" => Some(Self::RedbrandCrossbowman),
            "Redbrand Brute" => Some(Self::RedbrandBrute),
            "Garrick Redbrand" => Some(Self::Garrick),
            "Captain Ulric Thorn" => Some(Self::Thorn),
            _ => None,
        }
    }

    /// The `humanoid-appearance.presets` id: the bundle's for a resident, the game's for an
    /// enemy.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::Marshal => "resident.marshal",
            Self::Guard => "resident.guard",
            Self::Innkeeper => "resident.innkeeper",
            Self::Farmer | Self::Villager => "resident.farmer",
            Self::Healer => "resident.healer",
            Self::Tanner | Self::VillagerWoman => "resident.tanner",
            Self::Marauder => "enemy.marauder",
            Self::RedbrandBandit => "enemy.redbrand.bandit",
            Self::RedbrandCrossbowman => "enemy.redbrand.crossbowman",
            Self::RedbrandBrute => "enemy.redbrand.brute",
            Self::Garrick => "enemy.redbrand.garrick",
            Self::Thorn => "enemy.redbrand.thorn",
        }
    }

    /// Whether the units sharing this preset each wear their own fixed variation of it
    /// (asset-tooling #380): the guards and the rank-and-file enemies are different men in the
    /// same gear, while the named residents and Garrick keep their authored looks.
    #[must_use]
    pub const fn varied(self) -> bool {
        matches!(
            self,
            Self::Guard
                | Self::Villager
                | Self::VillagerWoman
                | Self::Marauder
                | Self::RedbrandBandit
                | Self::RedbrandCrossbowman
                | Self::RedbrandBrute
        )
    }

    /// What the character carries: the guards, bandits and Garrick a sword, the brute a sword
    /// and shield, a crossbowman a crossbow (#607), a marauder a dagger.
    #[must_use]
    pub const fn loadout(self) -> Option<Loadout> {
        match self {
            Self::Guard | Self::RedbrandBandit | Self::Garrick | Self::Thorn => {
                Some(Loadout::Sword)
            }
            Self::Marauder => Some(Loadout::Dagger),
            Self::RedbrandCrossbowman => Some(Loadout::Crossbow),
            Self::RedbrandBrute => Some(Loadout::SwordAndRedbrandShield),
            Self::Marshal
            | Self::Innkeeper
            | Self::Farmer
            | Self::Healer
            | Self::Tanner
            | Self::Villager
            | Self::VillagerWoman => None,
        }
    }

    /// The figure the character is drawn on: Hilda on the female, everyone else on the male.
    #[must_use]
    pub const fn figure(self) -> Figure {
        match self {
            Self::Tanner | Self::VillagerWoman => Figure::Female,
            _ => Figure::Male,
        }
    }

    /// The class whose authored strike the character plays ([`Kind::strike`]): a crossbowman
    /// shoots like a Ranger, with the crossbow clips where the bundle has them (#607) and the
    /// bow release otherwise; everyone else swings.
    #[must_use]
    pub const fn strike_class(self) -> Option<PlayerClass> {
        match self {
            Self::RedbrandCrossbowman => Some(PlayerClass::Ranger),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_core::greyhaven_vale;
    use strum::VariantArray;

    /// A projected great weapon (#1073) picks its own loadout, which carries that weapon in both
    /// hands and stows it across the back; other weapons keep their hand sockets' stow.
    #[test]
    fn a_projected_great_weapon_is_held_in_both_hands_and_stowed_on_the_back() {
        use mmorpg_core::GreatWeapon as Core;
        let warden = Loadout::of_class(PlayerClass::Warden);
        for (&core, great) in Core::VARIANTS.iter().zip(GreatWeapon::VARIANTS) {
            let held = warden.holding(Some(WeaponFamily::TwoHanded), None, Some(core));
            assert_eq!(held.great_weapon(), Some(*great), "{core:?}");
            assert_eq!(held.attachments()[0].item, great.item());
            assert_eq!(held.hands().left, held.hands().right);
            assert_eq!(
                stow_socket(great.item(), held.attachments()[0].socket),
                Some(GREAT_WEAPON_STOW_SOCKET)
            );
        }
        assert_eq!(
            warden.holding(Some(WeaponFamily::Sword), None, None),
            warden
        );
        assert_eq!(
            stow_socket("equipment.sword.arming", "hand_r.weapon"),
            Some("hip_r.stowed")
        );
        assert_eq!(
            stow_socket("equipment.shield.heater", "hand_l.shield"),
            Some("back.stowed")
        );
    }

    #[test]
    fn loadout_attachments_keep_the_pairs_both_clients_drew() {
        // The browser's former `LOADOUT_ITEMS` and the native `Equipment::loadout` match, which
        // agreed item for item (#1012).
        let before: &[(&str, &[(&str, &str)])] = &[
            (
                "sword-and-shield",
                &[
                    ("equipment.sword.arming", "hand_r.weapon"),
                    ("equipment.shield.heater", "hand_l.shield"),
                ],
            ),
            (
                "sword-and-shield.redbrand",
                &[
                    ("equipment.sword.arming", "hand_r.weapon"),
                    ("equipment.shield.redbrand", "hand_l.shield"),
                ],
            ),
            (
                "bow-and-quiver",
                &[
                    ("equipment.bow.long", "back.bow.stowed"),
                    ("equipment.quiver.back", "back.quiver"),
                ],
            ),
            ("sword", &[("equipment.sword.arming", "hand_r.weapon")]),
            ("dagger", &[("equipment.dagger.blackened", "hand_r.weapon")]),
            (
                "dagger.twin",
                &[
                    ("equipment.dagger.blackened", "hand_r.weapon"),
                    ("equipment.dagger.blackened", "hand_l.weapon"),
                ],
            ),
            ("staff.bone", &[("equipment.staff.bone", "hand_r.staff")]),
            (
                "staff.crystal-and-tome",
                &[
                    ("equipment.staff.crystal", "hand_r.staff"),
                    ("equipment.book.tome", "hand_l.shield"),
                ],
            ),
            (
                "mace.flanged",
                &[("equipment.mace.flanged", "hand_r.weapon")],
            ),
            (
                "staff.gnarled",
                &[("equipment.staff.gnarled", "hand_r.staff")],
            ),
            ("spear", &[("equipment.spear.leaf", "hand_r.staff")]),
            (
                "sword.garricks-cleaver",
                &[("equipment.sword.garricks-cleaver", "hand_r.weapon")],
            ),
            (
                "spear.hollowpine-hunter",
                &[("equipment.spear.hollowpine-hunter", "hand_r.staff")],
            ),
            // The great weapons (#1073) are new: their items on the run clips' socket.
            (
                "warhammer.maul",
                &[("equipment.warhammer.maul", "hand_r.weapon")],
            ),
            (
                "hammer.great",
                &[("equipment.hammer.great", "hand_r.weapon")],
            ),
            ("axe.great", &[("equipment.axe.great", "hand_r.weapon")]),
            ("sword.great", &[("equipment.sword.great", "hand_r.weapon")]),
            (
                "crossbow",
                &[
                    ("equipment.crossbow", "hand_r.crossbow"),
                    ("equipment.arrow", "hand_r.arrow"),
                ],
            ),
        ];
        assert_eq!(Loadout::VARIANTS.len(), before.len());
        for loadout in Loadout::VARIANTS {
            let (_, items) = before
                .iter()
                .find(|(name, _)| *name == loadout.name())
                .unwrap_or_else(|| panic!("{} drew no items before", loadout.name()));
            let now: Vec<_> = loadout
                .attachments()
                .iter()
                .map(|at| (at.item, at.socket))
                .collect();
            assert_eq!(now, *items, "{}", loadout.name());
        }
    }

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
                Some("resident.tanner"),
                Some("resident.innkeeper"),
                Some("resident.farmer"),
                Some("resident.healer"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
            ]
            .into_iter()
            .chain([
                // The region's people (#819), NPCs 22–41 …
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.farmer"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.guard"),
                Some("resident.guard"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.farmer"),
                Some("resident.tanner"),
                Some("resident.tanner"),
                Some("resident.farmer"),
                // Trader Maren (#826).
                Some("resident.tanner"),
                // Smith Wulfric Hale (#917).
                Some("resident.farmer"),
                // Guardsman Tam Hollis and Courier Wat Ferrow (#1019).
                Some("resident.guard"),
                Some("resident.farmer"),
                // Millbrook's farmfolk: Marta Mill, Farmhand Cole and Farmhand Edda (#1019).
                Some("resident.tanner"),
                Some("resident.farmer"),
                Some("resident.tanner"),
            ])
            .collect::<Vec<_>>()
        );
        let names: Vec<_> = PlayerClass::VARIANTS
            .iter()
            .copied()
            .map(|class| Loadout::of_class(class).name())
            .collect();
        assert_eq!(
            names,
            [
                "sword-and-shield",
                "bow-and-quiver",
                "staff.crystal-and-tome",
                "staff.bone",
                ROGUE_LOADOUT.name(),
                "mace.flanged",
                "staff.gnarled",
            ]
        );
    }

    #[test]
    fn humanoid_enemies_have_presets() {
        let content = greyhaven_vale::content();
        let presets: Vec<_> = content
            .creature_templates()
            .iter()
            .filter_map(|template| Resident::of_creature(&template.name).map(Resident::preset))
            .collect();
        assert_eq!(
            presets,
            [
                "enemy.marauder",
                "enemy.redbrand.bandit",
                "enemy.redbrand.garrick",
                "enemy.redbrand.crossbowman",
                "enemy.redbrand.brute",
                // The Redbrand Courier (#1019) wears the bandit's kit.
                "enemy.redbrand.bandit",
            ]
        );
        // Each is authored in the shared presets both clients draw.
        assert!(
            presets
                .iter()
                .all(|id| crate::units::presets::ENEMY_PRESET_IDS.contains(id))
        );
    }

    #[test]
    fn classes_wear_their_starter_outfit_and_drops_replace_its_slot_pieces() {
        let presets: Vec<_> = PlayerClass::VARIANTS
            .iter()
            .copied()
            .map(|class| Outfit::new(class, None).preset())
            .collect();
        assert_eq!(
            presets,
            [
                "class.warden",
                "class.ranger",
                "class.arcanist",
                "class.necromancer",
                "class.rogue",
                "class.cleric",
                "class.druid",
            ]
        );
        assert!(Outfit::new(PlayerClass::Warden, None).replaced().is_empty());
        // The Cloth Hood (6) and Worn Boots (9) cover the head and feet sockets only.
        let mut slots = [None; mmorpg_core::EQUIPMENT_SLOTS];
        slots[EquipmentSlot::Head.index() as usize] = Some(mmorpg_core::ItemId::new(6));
        slots[EquipmentSlot::Feet.index() as usize] = Some(mmorpg_core::ItemId::new(9));
        let equipment = Equipment::from_slots(slots).unwrap();
        assert_eq!(
            Outfit::new(PlayerClass::Warden, Some(&equipment)).replaced(),
            ["head.armor", "foot_l.armor", "foot_r.armor"]
        );
        // The Cleric's gold accent (#d9b44a), linear.
        let [r, g, b] = Outfit::new(PlayerClass::Cleric, None).trim();
        assert!(r > g && g > b && (r - 0.693).abs() < 0.01);
    }
}
