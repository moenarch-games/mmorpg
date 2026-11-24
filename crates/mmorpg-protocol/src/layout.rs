//! Where [`crate::PROTOCOL_ID`] comes from (#1066, docs/adr/0006-merge-safe-format-identities.md):
//! a hash of the wire layout description `fixtures/protocol/layout.txt` and of the byte fixtures,
//! taken at compile time.
//!
//! The description is generated from the encoders and decoders by this module's tests, one line
//! per field, and the fixtures carry the ID only as a placeholder ([`crate::fixture`]). A layout
//! change therefore changes some lines, the next build derives a new ID, and two branches that
//! change different messages edit different lines.

/// The committed wire layout description (`MMORPG_UPDATE_FIXTURES=1` regenerates it).
const LAYOUT: &str = include_str!("../../../fixtures/protocol/layout.txt");

/// The byte fixtures with the ID as a placeholder. They pin what the layout's widths cannot: codes,
/// flags and field meaning, so a change that keeps every width still changes the ID.
const FIXTURES: [&str; 5] = [
    include_str!("../../../fixtures/protocol/commands.hex"),
    include_str!("../../../fixtures/protocol/player-snapshot.hex"),
    include_str!("../../../fixtures/protocol/player-loot.hex"),
    include_str!("../../../fixtures/protocol/presentation-observation.hex"),
    include_str!("../../../fixtures/protocol/canonical-snapshot.hex"),
];

pub(crate) const PROTOCOL_ID: u32 = derive_protocol_id(LAYOUT, &FIXTURES);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over `text`, skipping carriage returns so a CRLF checkout derives the same ID.
const fn hash_text(mut hash: u64, text: &str) -> u64 {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\r' {
            hash = (hash ^ bytes[index] as u64).wrapping_mul(FNV_PRIME);
        }
        index += 1;
    }
    // A separator, so moving text from the end of one input to the next changes the hash.
    (hash ^ 0xff).wrapping_mul(FNV_PRIME)
}

/// The 24-bit ID of a layout and its fixtures. The low byte, which leads every command, stays off
/// 1–13, the retired command wire versions, so a command from before ADR 0006 never reads as
/// current: such a hash is rehashed until it does.
const fn derive_protocol_id(layout: &str, fixtures: &[&str]) -> u32 {
    let mut hash = hash_text(FNV_OFFSET, layout);
    let mut index = 0;
    while index < fixtures.len() {
        hash = hash_text(hash, fixtures[index]);
        index += 1;
    }
    loop {
        let id = ((hash ^ (hash >> 24) ^ (hash >> 48)) & 0x00ff_ffff) as u32;
        if !matches!(id & 0xff, 1..=13) {
            return id;
        }
        hash = (hash ^ 0x55).wrapping_mul(FNV_PRIME);
    }
}

/// Records the reads a decoder makes ([`crate::wire::take`]), as byte offsets into the outermost
/// payload, so the layout description is the decoders' own view of the bytes.
#[cfg(test)]
pub(crate) mod trace {
    use std::cell::RefCell;

    struct Trace {
        start: usize,
        end: usize,
        reads: Vec<(usize, usize)>,
    }

    thread_local! {
        static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
    }

    /// Called by every `take`: one field of `width` bytes at `offset` into `payload`. A read
    /// from a copy (a decoder splitting a field it already took) is no field of its own.
    pub(crate) fn read(payload: &[u8], offset: usize, width: usize) {
        TRACE.with(|trace| {
            if let Some(trace) = trace.borrow_mut().as_mut() {
                let at = payload.as_ptr() as usize + offset;
                if at >= trace.start && at + width <= trace.end {
                    trace.reads.push((at - trace.start, width));
                }
            }
        });
    }

    /// The fields `decode` reads from `bytes`, in order.
    pub(crate) fn fields<T>(
        bytes: &[u8],
        decode: impl FnOnce(&[u8]) -> T,
    ) -> (T, Vec<(usize, usize)>) {
        let start = bytes.as_ptr() as usize;
        TRACE.with(|trace| {
            *trace.borrow_mut() = Some(Trace {
                start,
                end: start + bytes.len(),
                reads: Vec::new(),
            });
        });
        let value = decode(bytes);
        let reads = TRACE.with(|trace| trace.borrow_mut().take().expect("trace").reads);
        (value, reads)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mmorpg_core::{TransferId, ZoneId};

    use super::{FNV_OFFSET, derive_protocol_id, hash_text, trace};
    use crate::{
        PROTOCOL_ID, RedirectNotice, decode_canonical_snapshot, decode_redirect_notice,
        decode_snapshot, encode_canonical_snapshot, encode_redirect_notice, encode_snapshot,
    };

    /// One line per field: the message, the field's byte offset and its width. Bytes no `take`
    /// reads (strings, routes) are one `blob` line, so the lines cover every byte exactly.
    fn describe_fields(message: &str, bytes: &[u8], reads: &[(usize, usize)]) -> String {
        let mut lines = String::new();
        let mut next = 0;
        for &(offset, width) in reads {
            assert!(
                offset >= next,
                "{message}: the decoder reads byte {offset} again"
            );
            if offset > next {
                lines.push_str(&format!("{message} @{next} {}B blob\n", offset - next));
            }
            lines.push_str(&format!("{message} @{offset} {width}B\n"));
            next = offset + width;
        }
        if next < bytes.len() {
            lines.push_str(&format!("{message} @{next} {}B blob\n", bytes.len() - next));
        }
        lines
    }

    /// Commands decode by slice patterns, so each is one line: its name and sizes. The command
    /// fixture pins their fields byte by byte, and it feeds the ID too.
    fn describe_commands() -> String {
        let mut sizes = std::collections::BTreeMap::<String, BTreeSet<usize>>::new();
        for line in crate::command::tests::command_fixture()
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            // `<hex> <name> …` or `<hex> observed <tick> <name> …`.
            let words = line.split_whitespace().collect::<Vec<_>>();
            let name = match words[1] {
                "observed" => format!("observed {}", words[3]),
                name => name.to_owned(),
            };
            sizes.entry(name).or_default().insert(words[0].len() / 2);
        }
        sizes
            .into_iter()
            .map(|(name, sizes)| {
                let sizes = sizes
                    .into_iter()
                    .map(|size| format!("{size}B"))
                    .collect::<Vec<_>>();
                format!("command {name} {}\n", sizes.join(" "))
            })
            .collect()
    }

    fn describe_snapshot(message: &str, bytes: &[u8]) -> String {
        let (decoded, reads) = trace::fields(bytes, decode_snapshot);
        decoded.expect(message);
        describe_fields(message, bytes, &reads)
    }

    /// The layout description `fixtures/protocol/layout.txt` holds, from the same samples as the
    /// byte fixtures.
    fn layout() -> String {
        let mut layout = String::from(
            "# Wire layout PROTOCOL_ID is derived from (ADR 0006, #1066): one line per field as\n\
             # `message @offset width`, traced from the decoders over the fixture samples; commands\n\
             # are one line each. Generated by mmorpg-protocol's layout test; rerun it with\n\
             # MMORPG_UPDATE_FIXTURES=1 after a wire change and the next build derives a new ID.\n",
        );
        layout.push_str(&describe_commands());
        let canonical = encode_canonical_snapshot(&crate::canonical::tests::snapshot()).unwrap();
        let (decoded, reads) = trace::fields(&canonical, decode_canonical_snapshot);
        decoded.expect("canonical snapshot");
        layout.push_str(&describe_fields("canonical", &canonical, &reads));
        for (message, snapshot) in [
            (
                "player-snapshot",
                crate::projection::tests::fixture_snapshot(),
            ),
            (
                "player-loot",
                crate::projection::tests::loot_fixture_snapshot(),
            ),
            (
                "presentation-observation",
                crate::projection::tests::observation_fixture_snapshot(),
            ),
        ] {
            layout.push_str(&describe_snapshot(
                message,
                &encode_snapshot(&snapshot).unwrap(),
            ));
        }
        let redirect = encode_redirect_notice(&RedirectNotice {
            zone_id: ZoneId::new(2),
            tick: 90,
            destination: ZoneId::new(3),
            transfer_id: TransferId::new(7).unwrap(),
            cutover_sequence: 4,
            route: "/zone/3".to_owned(),
        })
        .unwrap();
        let (decoded, reads) = trace::fields(&redirect, decode_redirect_notice);
        decoded.expect("redirect notice");
        layout.push_str(&describe_fields("redirect", &redirect, &reads));
        layout
    }

    #[test]
    fn the_layout_description_matches_the_decoders() {
        let layout = layout();
        let checked_in = mmorpg_core::golden::golden(
            mmorpg_core::golden::workspace_root().join("fixtures/protocol/layout.txt"),
            &layout,
        );
        assert_eq!(
            checked_in.replace('\r', ""),
            layout,
            "the wire layout changed; rerun with MMORPG_UPDATE_FIXTURES=1"
        );
    }

    #[test]
    fn the_protocol_id_is_24_bits_and_never_a_retired_command_version() {
        assert_eq!(PROTOCOL_ID >> 24, 0);
        assert!(!matches!(PROTOCOL_ID & 0xff, 1..=13));
    }

    #[test]
    fn any_layout_or_fixture_change_derives_another_id() {
        let id = derive_protocol_id("a @0 1B\n", &["PP01\n"]);
        assert_eq!(id, derive_protocol_id("a @0 1B\r\n", &["PP01\r\n"]));
        assert_ne!(id, derive_protocol_id("a @0 2B\n", &["PP01\n"]));
        assert_ne!(id, derive_protocol_id("a @0 1B\n", &["PP02\n"]));
        assert_ne!(id, derive_protocol_id("a @0 1B\nPP01\n", &[""]));
        assert_ne!(hash_text(FNV_OFFSET, "a"), hash_text(FNV_OFFSET, "b"));
    }
}
