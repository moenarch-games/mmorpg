//! The content revision covers private homes and seats on their own (#1002).

use mmorpg_core::{Seat, SeatId, ZoneContent, ZoneDefinition, greyhaven_vale};

#[test]
fn private_homes_change_the_revision() {
    let content = (*greyhaven_vale::content()).clone();
    let homes = content.private_homes().to_vec();
    assert!(!homes.is_empty(), "Greyhaven authors a private home");
    let revision = content.revision();

    let same = content.clone().with_private_homes(homes.clone()).unwrap();
    assert_eq!(same.revision(), revision, "identical content, identical identity");

    let removed = content.clone().with_private_homes(Vec::new()).unwrap();
    assert_ne!(removed.revision(), revision, "removing the homes");

    let mut changed = homes;
    changed[0].woken_line.push('!');
    let changed = content.with_private_homes(changed).unwrap();
    assert_ne!(changed.revision(), revision, "changing a home");
}

#[test]
fn seats_change_the_revision_without_routines() {
    let seat = |facing| Seat {
        id: SeatId::new(1),
        stand: [100, 200],
        root: [100, 220],
        facing,
    };
    let bare = ZoneContent::from_definition(ZoneDefinition::default());
    let seated = bare.clone().with_seats(vec![seat(0)]).unwrap();
    let turned = bare.clone().with_seats(vec![seat(90)]).unwrap();
    assert!(bare.routines().is_empty());
    assert_ne!(seated.revision(), bare.revision(), "adding a seat");
    assert_ne!(turned.revision(), seated.revision(), "changing a seat");
    assert_eq!(
        bare.clone().with_seats(vec![seat(0)]).unwrap().revision(),
        seated.revision()
    );
}
