//! Already-received native interaction feedback must survive GUI snapshot coalescing.
//! This does not promise recovery of lost network datagrams.
use super::*;
use mmorpg_core::{ErrorCode, NpcId, ZoneId, greyhaven_vale};
#[path = "native_pointer_host.rs"]
mod host;

#[tokio::test]
async fn actual_transport_refusal_survives_two_publications_before_gui_consumption() {
    let id = ZoneId::new(1);
    let content = greyhaven_vale::content();
    let host = host::LocalHost::start(
        mmorpg_game_server::ZoneGameServerAdapter::with_content(id, Arc::clone(&content)).unwrap(),
        id,
    );
    let mut session =
        ClientSession::connect(&host.url, Some(&host.certificate), id, content.revision())
            .await
            .unwrap();
    session
        .send_command(ZoneCommand::BuyItem {
            npc: NpcId::new(3),
            offer: 0,
            quantity: 1,
        })
        .unwrap();
    let refusal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let received = session.receive_snapshot().await.unwrap();
            if received.events.iter().any(|e| {
                matches!(
                    e,
                    mmorpg_core::ZoneEvent::Error {
                        code: ErrorCode::OutOfRange,
                        ..
                    }
                )
            }) {
                break received;
            }
        }
    })
    .await
    .expect("real host must emit and real ClientSession must decode range refusal");
    eprintln!(
        "RECEIPT_TRANSPORT tick={} ack={} events={:?}",
        refusal.tick, refusal.acknowledged_sequence, refusal.events
    );
    let quiet = loop {
        let next = session.receive_snapshot().await.unwrap();
        if next.tick > refusal.tick && next.events.is_empty() {
            break next;
        }
    };
    let (updates, mut gui) = watch::channel(NetworkUpdate::Waiting);
    publish(&updates, &session, refusal.clone());
    publish(&updates, &session, quiet.clone());
    let latest = gui.borrow_and_update().clone();
    let NetworkUpdate::Snapshot {
        snapshot,
        refusal: retained,
        ..
    } = latest
    else {
        panic!("GUI must receive latest native world state")
    };
    let expected = RefusalReceipt {
        tick: refusal.tick,
        acknowledged_sequence: refusal.acknowledged_sequence,
        code: ErrorCode::OutOfRange,
        target: Some(mmorpg_core::EntityRef::Npc(NpcId::new(3))),
    };
    assert_eq!(
        snapshot.tick, quiet.tick,
        "motion/state still follows newest projection"
    );
    assert!(
        snapshot.events.is_empty(),
        "quiet projection must not replay transient visual effects"
    );
    assert_eq!(
        retained,
        Some(expected),
        "already decoded refusal was discarded before GUI could observe it"
    );
    publish(&updates, &session, quiet.clone());
    let NetworkUpdate::Snapshot {
        refusal: repeated, ..
    } = gui.borrow_and_update().clone()
    else {
        panic!("snapshot")
    };
    assert_eq!(
        repeated,
        Some(expected),
        "same receipt identity lets App apply feedback only once"
    );
    let mut newer = quiet.clone();
    newer.tick += 1;
    newer.events.push(mmorpg_core::ZoneEvent::Error {
        code: ErrorCode::InvalidVendor,
        target: None,
    });
    publish(&updates, &session, newer.clone());
    let NetworkUpdate::Snapshot {
        refusal: replaced, ..
    } = gui.borrow_and_update().clone()
    else {
        panic!("snapshot")
    };
    assert_eq!(
        replaced,
        Some(RefusalReceipt {
            tick: newer.tick,
            acknowledged_sequence: newer.acknowledged_sequence,
            code: ErrorCode::InvalidVendor,
            target: None
        }),
        "only newest refusal retained"
    );
    let old_epoch = session.connection_epoch();
    let mut resumed = session.reconnect().await.unwrap();
    assert!(session.connection_epoch() > old_epoch);
    resumed.events.clear();
    publish(&updates, &session, resumed);
    let NetworkUpdate::Snapshot {
        refusal: resumed_receipt,
        ..
    } = gui.borrow_and_update().clone()
    else {
        panic!("snapshot")
    };
    assert_eq!(
        resumed_receipt, None,
        "new epoch must clear obsolete refusal"
    );
}
