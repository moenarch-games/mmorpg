//! One place per record for its bytes (#1088): the [`Codec`] trait puts a record's encoder and
//! decoder side by side, and the [`record!`] macro declares a fixed-width wire layout once, so
//! its size constant is the sum of its fields' widths instead of a hand-written one.
//!
//! A layout record is plain wire data: integers, arrays of them and other layout records, in
//! order, big-endian. The game type it carries implements [`Codec`] through it and owns the
//! meaning: codes, flags and which values are valid. Every field read goes through
//! [`crate::wire::take`], so the layout description `fixtures/protocol/layout.txt` keeps tracing
//! the decoders' real reads.

use crate::ProtocolError;
use crate::wire::take;

/// A record's encoder and decoder. `decode` reads exactly what `encode` wrote and fails closed on
/// anything `encode` never writes.
pub(crate) trait Codec: Sized {
    /// Appends the record, or fails when the value has no wire form.
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError>;
    /// Reads one record at `offset` and advances it past the record.
    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError>;
}

/// A fixed-width wire field: an integer, an array of fields or a [`record!`].
pub(crate) trait Field: Copy {
    /// Bytes on the wire.
    const WIDTH: usize;
    fn put(&self, payload: &mut Vec<u8>);
    fn read(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError>;
}

macro_rules! integer_fields {
    ($($ty:ty),*) => {$(
        impl Field for $ty {
            const WIDTH: usize = std::mem::size_of::<$ty>();

            fn put(&self, payload: &mut Vec<u8>) {
                payload.extend_from_slice(&self.to_be_bytes());
            }

            fn read(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
                Ok(<$ty>::from_be_bytes(take(payload, offset)?))
            }
        }
    )*};
}

integer_fields!(u8, u16, u32, u64, i8, i16, i32);

/// Each element in order, so a vector's components stay one traced field each.
impl<T: Field + Default, const N: usize> Field for [T; N] {
    const WIDTH: usize = T::WIDTH * N;

    fn put(&self, payload: &mut Vec<u8>) {
        for element in self {
            element.put(payload);
        }
    }

    fn read(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let mut array = [T::default(); N];
        for element in &mut array {
            *element = T::read(payload, offset)?;
        }
        Ok(array)
    }
}

/// Declares a fixed-width wire layout: a struct of [`Field`]s in wire order, its `BYTES` (the sum
/// of the fields' widths) and its [`Field`] and [`Codec`] implementations.
macro_rules! record {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($(#[$field_meta:meta])* $field:ident: $ty:ty),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        $vis struct $name {
            $($(#[$field_meta])* pub(crate) $field: $ty),+
        }

        impl $name {
            /// Bytes on the wire: the sum of the fields' widths.
            pub(crate) const BYTES: usize = 0 $(+ <$ty as $crate::codec::Field>::WIDTH)+;
        }

        impl $crate::codec::Field for $name {
            const WIDTH: usize = Self::BYTES;

            fn put(&self, payload: &mut Vec<u8>) {
                $($crate::codec::Field::put(&self.$field, payload);)+
            }

            fn read(
                payload: &[u8],
                offset: &mut usize,
            ) -> Result<Self, $crate::ProtocolError> {
                Ok(Self {
                    $($field: <$ty as $crate::codec::Field>::read(payload, offset)?),+
                })
            }
        }

        impl $crate::codec::Codec for $name {
            fn encode(&self, payload: &mut Vec<u8>) -> Result<(), $crate::ProtocolError> {
                $crate::codec::Field::put(self, payload);
                Ok(())
            }

            fn decode(
                payload: &[u8],
                offset: &mut usize,
            ) -> Result<Self, $crate::ProtocolError> {
                $crate::codec::Field::read(payload, offset)
            }
        }
    };
}

pub(crate) use record;

/// Deterministic generators for the round-trip property tests: SplitMix64, no dependency.
#[cfg(test)]
pub(crate) mod generate {
    use super::Codec;

    pub(crate) struct Gen(u64);

    impl Gen {
        pub(crate) fn new(seed: u64) -> Self {
            Self(seed)
        }

        pub(crate) fn u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        /// Uniform below `bound`.
        pub(crate) fn below(&mut self, bound: u64) -> u64 {
            self.u64() % bound
        }

        pub(crate) fn bool(&mut self) -> bool {
            self.u64() & 1 != 0
        }

        /// Any value of `T`, extremes about one draw in four.
        pub(crate) fn int<T: Bounded>(&mut self) -> T {
            match self.below(8) {
                0 => T::MIN,
                1 => T::MAX,
                _ => T::wrap(self.u64()),
            }
        }

        /// One of `choices`.
        pub(crate) fn pick<T: Copy>(&mut self, choices: &[T]) -> T {
            choices[usize::try_from(self.below(choices.len() as u64)).unwrap()]
        }
    }

    /// Integers the generator can draw.
    pub(crate) trait Bounded: Sized {
        const MIN: Self;
        const MAX: Self;
        fn wrap(bits: u64) -> Self;
    }

    macro_rules! bounded {
        ($($ty:ty),*) => {$(
            impl Bounded for $ty {
                const MIN: Self = <$ty>::MIN;
                const MAX: Self = <$ty>::MAX;
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                fn wrap(bits: u64) -> Self {
                    bits as $ty
                }
            }
        )*};
    }

    bounded!(u8, u16, u32, u64, i8, i16, i32);

    /// Draws this many cases per record.
    pub(crate) const CASES: u64 = 512;

    /// Encodes `value`, checks the width when `bytes` is given, decodes it back from inside a
    /// longer payload and checks both the value and the bytes survive.
    pub(crate) fn round_trip<T: Codec + PartialEq + std::fmt::Debug>(
        value: &T,
        bytes: Option<usize>,
    ) {
        let mut payload = vec![0xa5];
        value.encode(&mut payload).expect("encodes");
        if let Some(bytes) = bytes {
            assert_eq!(payload.len() - 1, bytes, "{value:?}");
        }
        let end = payload.len();
        payload.push(0x5a);
        let mut offset = 1;
        let decoded = T::decode(&payload, &mut offset).expect("decodes");
        assert_eq!(offset, end, "{value:?}");
        assert_eq!(&decoded, value);
        let mut again = vec![0xa5];
        decoded.encode(&mut again).unwrap();
        assert_eq!(again[..], payload[..end]);
    }
}

#[cfg(test)]
mod tests {
    use super::generate::{CASES, Gen, round_trip};
    use super::{Codec, Field};
    use crate::ProtocolError;

    record! {
        /// A record of every field kind, nested record included.
        struct Inner {
            code: u8,
            id: u32,
        }
    }

    record! {
        struct Sample {
            small: i8,
            word: u16,
            signed: i16,
            vector: [i32; 3],
            inner: Inner,
            wide: u64,
            pair: [Inner; 2],
        }
    }

    #[test]
    fn the_size_is_the_sum_of_the_field_widths() {
        assert_eq!(Inner::BYTES, 5);
        assert_eq!(Sample::BYTES, 1 + 2 + 2 + 12 + 5 + 8 + 10);
        assert_eq!(<[u16; 4] as Field>::WIDTH, 8);
    }

    #[test]
    fn records_round_trip_big_endian_in_field_order() {
        let sample = Sample {
            small: -2,
            word: 0x0102,
            vector: [1, -1, 0x0a0b_0c0d],
            inner: Inner { code: 9, id: 7 },
            ..Sample::default()
        };
        let mut payload = Vec::new();
        sample.encode(&mut payload).unwrap();
        assert_eq!(payload[..3], [0xfe, 1, 2]);
        assert_eq!(payload[5..9], [0, 0, 0, 1]);
        assert_eq!(payload[13..17], [0x0a, 0x0b, 0x0c, 0x0d]);
        assert_eq!(payload[17..22], [9, 0, 0, 0, 7]);
        let mut rng = Gen::new(1088);
        for _ in 0..CASES {
            let inner = |rng: &mut Gen| Inner {
                code: rng.int(),
                id: rng.int(),
            };
            let sample = Sample {
                small: rng.int(),
                word: rng.int(),
                signed: rng.int(),
                vector: [rng.int(), rng.int(), rng.int()],
                inner: inner(&mut rng),
                wide: rng.int(),
                pair: [inner(&mut rng), inner(&mut rng)],
            };
            round_trip(&sample, Some(Sample::BYTES));
        }
    }

    #[test]
    fn a_short_payload_fails_closed() {
        let mut payload = Vec::new();
        Sample::default().encode(&mut payload).unwrap();
        for end in 0..payload.len() {
            let mut offset = 0;
            assert_eq!(
                Sample::decode(&payload[..end], &mut offset),
                Err(ProtocolError::new("payload is truncated"))
            );
        }
    }
}
