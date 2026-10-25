//! #379's concrete timing scenario: an authored 50 ms buffer and 200 ms actionable window,
//! resolved on the authoritative 60 Hz tick (16⅔ ms) rather than rounded to 30 Hz. The
//! effective boundaries are documented in `docs/ARCHITECTURE.md` (Simulation rate).
use mmorpg_core::TICK_HZ;
use mmorpg_core::action::{BlockReason, EdgeStatus, Window};

/// Opens 200 ms after its anchor, buffers inputs 50 ms ahead, then stays open 200 ms.
const WINDOW: Window = Window {
    open_after_ms: 200,
    actionable_ms: 200,
    buffer_ms: 50,
};

#[test]
fn a_50_ms_buffer_and_200_ms_window_resolve_on_whole_60_hz_ticks() {
    assert_eq!(TICK_HZ, 60);
    // 50 ms is exactly 3 ticks and 200 ms exactly 12: nothing rounds.
    assert_eq!(WINDOW.buffer_ticks(), 3);
    assert_eq!(WINDOW.open_after_ticks(), 12);
    assert_eq!(WINDOW.actionable_ticks(), 12);
    let statuses: Vec<_> = (0..=26).map(|elapsed| WINDOW.status_at(elapsed)).collect();
    let too_early = EdgeStatus::Blocked(BlockReason::TooEarly);
    let expired = EdgeStatus::Blocked(BlockReason::Expired);
    // Ticks 0–8 (0–133 ms after the anchor) are too early.
    assert!(statuses[..9].iter().all(|&status| status == too_early));
    // Ticks 9–11 (150–183 ms) buffer the input: the 50 ms before opening.
    assert!(
        statuses[9..12]
            .iter()
            .all(|&status| status == EdgeStatus::Buffering)
    );
    // Ticks 12–24 (200–400 ms) are actionable, the closing tick included.
    assert!(
        statuses[12..=24]
            .iter()
            .all(|&status| status == EdgeStatus::Open)
    );
    // From tick 25 (417 ms) the window has expired.
    assert!(statuses[25..].iter().all(|&status| status == expired));
}
