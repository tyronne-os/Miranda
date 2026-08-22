//! Binary packet encoding for the data plane.
//!
//! Every data-plane WebSocket message is a 312-byte packet laid out as
//! documented in the [`crate`] module docs. This module is the single place
//! that knows that layout, so the encoder and the decoder stay in sync.

use bytes::{BufMut, Bytes, BytesMut};
use miranda_core::{BlendshapeFrame, KinematicTransformFrame};

/// Magic bytes — first four bytes of every data-plane packet.
///
/// `MRD1` = Miranda Data Protocol version 1.
/// A receiver that does not see this prefix should drop the packet and log
/// a version-mismatch event rather than misinterpreting the payload.
pub const PACKET_MAGIC: &[u8; 4] = b"MRD1";

/// Total packet size in bytes.
///
/// 4 (magic) + 2 (blend_sz) + 2 (kin_sz) + 216 (BlendshapeFrame) +
/// 88 (KinematicTransformFrame) = 312.
pub const PACKET_SIZE: usize = 4 + 2 + 2
    + std::mem::size_of::<BlendshapeFrame>()
    + std::mem::size_of::<KinematicTransformFrame>();

/// Encodes one data-plane packet into `dst`.
///
/// `dst` is grown by exactly [`PACKET_SIZE`] bytes. Calling this in the
/// broadcast loop with a pre-allocated `BytesMut` avoids a per-frame heap
/// allocation.
pub fn encode(
    blend: &BlendshapeFrame,
    kin: &KinematicTransformFrame,
    dst: &mut BytesMut,
) {
    dst.put_slice(PACKET_MAGIC);
    dst.put_u16_le(std::mem::size_of::<BlendshapeFrame>() as u16);
    dst.put_u16_le(std::mem::size_of::<KinematicTransformFrame>() as u16);
    // SAFETY: BlendshapeFrame and KinematicTransformFrame are both
    // repr(C) + bytemuck::Pod, which means:
    //   - No padding bytes (Pod requires no uninit bytes).
    //   - No invalid bit patterns (Pod requires every bit pattern valid).
    //   - Size_of gives the exact number of meaningful bytes.
    // Casting a Pod value to &[u8] via bytemuck::bytes_of is the canonical
    // safe pattern — no raw pointer arithmetic, no manual transmute.
    dst.put_slice(bytemuck::bytes_of(blend));
    dst.put_slice(bytemuck::bytes_of(kin));
    debug_assert_eq!(
        dst.len() % PACKET_SIZE,
        0,
        "packet encode produced wrong size"
    );
}

/// Encodes one data-plane packet and returns it as an owned [`Bytes`].
///
/// Allocates. Use the `encode` form with a scratch `BytesMut` in the hot
/// loop; use this form in tests and slow paths.
pub fn encode_to_bytes(blend: &BlendshapeFrame, kin: &KinematicTransformFrame) -> Bytes {
    let mut buf = BytesMut::with_capacity(PACKET_SIZE);
    encode(blend, kin, &mut buf);
    buf.freeze()
}

/// Decodes a data-plane packet from `src`.
///
/// Returns `Err` if the packet is the wrong size, the magic bytes do not
/// match, or the declared payload sizes disagree with the compiled sizes.
/// Never panics.
pub fn decode(src: &[u8]) -> Result<(BlendshapeFrame, KinematicTransformFrame), DecodeError> {
    if src.len() != PACKET_SIZE {
        return Err(DecodeError::WrongLength {
            got: src.len(),
            expected: PACKET_SIZE,
        });
    }
    if &src[..4] != PACKET_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let blend_sz = u16::from_le_bytes([src[4], src[5]]) as usize;
    let kin_sz = u16::from_le_bytes([src[6], src[7]]) as usize;
    if blend_sz != std::mem::size_of::<BlendshapeFrame>() {
        return Err(DecodeError::SizeMismatch {
            field: "BlendshapeFrame",
            got: blend_sz,
            expected: std::mem::size_of::<BlendshapeFrame>(),
        });
    }
    if kin_sz != std::mem::size_of::<KinematicTransformFrame>() {
        return Err(DecodeError::SizeMismatch {
            field: "KinematicTransformFrame",
            got: kin_sz,
            expected: std::mem::size_of::<KinematicTransformFrame>(),
        });
    }
    let blend_start = 8;
    let kin_start = blend_start + blend_sz;
    // SAFETY: same as encode — bytemuck::from_bytes is safe for Pod types.
    let blend: BlendshapeFrame = *bytemuck::from_bytes(&src[blend_start..blend_start + blend_sz]);
    let kin: KinematicTransformFrame =
        *bytemuck::from_bytes(&src[kin_start..kin_start + kin_sz]);
    Ok((blend, kin))
}

/// Errors that can occur when decoding a data-plane packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    WrongLength { got: usize, expected: usize },
    BadMagic,
    SizeMismatch {
        field: &'static str,
        got: usize,
        expected: usize,
    },
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLength { got, expected } => {
                write!(f, "packet wrong length: got {got}, expected {expected}")
            }
            Self::BadMagic => write!(f, "packet magic bytes do not match MRD1"),
            Self::SizeMismatch { field, got, expected } => {
                write!(
                    f,
                    "size field for {field} says {got} bytes, compiled size is {expected}"
                )
            }
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use miranda_core::{BLENDSHAPE_COUNT, KINEMATIC_JOINT_COUNT, Quaternion};

    fn sample_blend(ts: u64) -> BlendshapeFrame {
        BlendshapeFrame {
            timestamp_us: ts,
            weights: {
                let mut w = [0.0f32; BLENDSHAPE_COUNT];
                w[17] = 0.5; // jawOpen
                w
            },
        }
    }

    fn sample_kin(ts: u64) -> KinematicTransformFrame {
        KinematicTransformFrame {
            timestamp_us: ts,
            joints: [Quaternion::IDENTITY; KINEMATIC_JOINT_COUNT],
            head_pitch_deg: 0.4,
            clavicle_rise: 0.2,
            _reserved: [0; 8],
        }
    }

    #[test]
    fn packet_size_constant_is_correct() {
        // Verify the constant agrees with the actual sizes.
        assert_eq!(
            PACKET_SIZE,
            4 + 2 + 2 + 216 + 88,
            "PACKET_SIZE constant is wrong"
        );
    }

    #[test]
    fn encode_produces_exactly_packet_size_bytes() {
        let b = sample_blend(1);
        let k = sample_kin(1);
        let pkt = encode_to_bytes(&b, &k);
        assert_eq!(pkt.len(), PACKET_SIZE);
    }

    #[test]
    fn magic_bytes_are_present() {
        let pkt = encode_to_bytes(&sample_blend(0), &sample_kin(0));
        assert_eq!(&pkt[..4], PACKET_MAGIC);
    }

    #[test]
    fn size_fields_are_little_endian_and_correct() {
        let pkt = encode_to_bytes(&sample_blend(0), &sample_kin(0));
        let blend_sz = u16::from_le_bytes([pkt[4], pkt[5]]) as usize;
        let kin_sz = u16::from_le_bytes([pkt[6], pkt[7]]) as usize;
        assert_eq!(blend_sz, 216);
        assert_eq!(kin_sz, 88);
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let b = sample_blend(99_000);
        let k = KinematicTransformFrame::from_breath(99_000, 0.6, 0.3);
        let pkt = encode_to_bytes(&b, &k);
        let (b2, k2) = decode(&pkt).expect("decode failed");
        assert_eq!(b2.timestamp_us, b.timestamp_us);
        assert_eq!(b2.weights[17], 0.5);
        assert_eq!(k2.timestamp_us, k.timestamp_us);
        assert!((k2.head_pitch_deg - 0.6).abs() < 1e-6);
        assert!((k2.clavicle_rise - 0.3).abs() < 1e-6);
    }

    #[test]
    fn decode_rejects_wrong_length() {
        let pkt = encode_to_bytes(&sample_blend(0), &sample_kin(0));
        let short = &pkt[..PACKET_SIZE - 1];
        assert!(matches!(decode(short), Err(DecodeError::WrongLength { .. })));
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut pkt = encode_to_bytes(&sample_blend(0), &sample_kin(0)).to_vec();
        pkt[0] = 0xFF;
        assert!(matches!(decode(&pkt), Err(DecodeError::BadMagic)));
    }

    #[test]
    fn decode_rejects_wrong_size_fields() {
        let mut pkt = encode_to_bytes(&sample_blend(0), &sample_kin(0)).to_vec();
        // Corrupt the blend_sz field.
        let wrong: u16 = 100;
        pkt[4..6].copy_from_slice(&wrong.to_le_bytes());
        assert!(matches!(
            decode(&pkt),
            Err(DecodeError::SizeMismatch { field: "BlendshapeFrame", .. })
        ));
    }

    #[test]
    fn encode_with_scratch_buffer_matches_encode_to_bytes() {
        let b = sample_blend(7);
        let k = sample_kin(7);
        let owned = encode_to_bytes(&b, &k);
        let mut scratch = BytesMut::with_capacity(PACKET_SIZE);
        encode(&b, &k, &mut scratch);
        assert_eq!(owned.as_ref(), scratch.as_ref());
    }
}
