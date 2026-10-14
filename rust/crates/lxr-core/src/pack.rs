//! The lossless container that the files of an index are stored in.
//!
//! A packed file is a small header followed by the original bytes, deflated when that makes
//! them smaller. Packing never changes a byte: [`pack`] decodes what it has just encoded and
//! compares it with the input, and stores the input itself when the two differ or when
//! deflating does not shrink it. [`unpack`] checks the length and a hash of the original
//! bytes, so a damaged file is an error and never a different value.
//!
//! Layout, all numbers little endian:
//!
//! ```text
//! magic "LXZ1" (4)   codec (1)   length of the original (8)   hash of the original (8)   payload
//! ```
//!
//! Codec 0 stores the original bytes as they are. Codec 1 stores them as a zlib stream.

use crate::store::hash64;
use std::io;

const MAGIC: &[u8; 4] = b"LXZ1";
const HEADER: usize = 21;
const STORED: u8 = 0;
const DEFLATED: u8 = 1;
/// Deflate level. Levels 7 to 9 shrink the files of an index by under 1% and take several
/// times as long to write.
const LEVEL: u8 = 6;
/// Largest original a header may claim. An index file is far smaller; a claim above this is
/// a damaged header and is refused before any memory is set aside for it.
const MAX_ORIGINAL: u64 = 1 << 32;
/// Most that one byte of a deflate stream can stand for (the format's limit is 1032).
const MAX_RATIO: usize = 1100;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// True when `bytes` begin like a packed file.
pub fn is_packed(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && &bytes[..4] == MAGIC
}

/// Packs `original`. The result unpacks to exactly `original`.
pub fn pack(original: &[u8]) -> Vec<u8> {
    let hash = hash64(original);
    let deflated = miniz_oxide::deflate::compress_to_vec_zlib(original, LEVEL);
    let smaller = deflated.len() < original.len();
    // An encoder that does not give the bytes back is not trusted with them.
    let exact = smaller && inflate(&deflated, original.len()).is_ok_and(|back| back == original);
    let (codec, payload) = if exact { (DEFLATED, deflated.as_slice()) } else { (STORED, original) };
    wrap(codec, original.len(), hash, payload)
}

/// Wraps `original` in the container without compressing it: for bytes that deflate hardly
/// shrinks and that are read at every query. [`unpack`] checks the length and the hash as for
/// any other packed file.
pub fn store(original: &[u8]) -> Vec<u8> {
    wrap(STORED, original.len(), hash64(original), original)
}

/// The header for a payload of `codec` that stands for `length` original bytes with this
/// `hash`, followed by the payload.
fn wrap(codec: u8, length: usize, hash: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(codec);
    out.extend_from_slice(&(length as u64).to_le_bytes());
    out.extend_from_slice(&hash.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// The original bytes of a file made by [`pack`]. An error for anything else, for a file that
/// was cut or changed, and for a header that claims more than an index file can hold.
pub fn unpack(packed: &[u8]) -> io::Result<Vec<u8>> {
    if !is_packed(packed) {
        return Err(invalid("not a packed file (wrong magic or too short)"));
    }
    let word = |at: usize| u64::from_le_bytes(packed[at..at + 8].try_into().expect("eight bytes"));
    let (codec, length, hash) = (packed[4], word(5), word(13));
    if length > MAX_ORIGINAL {
        return Err(invalid("the packed file claims a length that is too large"));
    }
    let length = length as usize;
    let payload = &packed[HEADER..];
    let original = match codec {
        STORED if payload.len() == length => payload.to_vec(),
        STORED => return Err(invalid("the size of the packed file does not match its header")),
        // Deflate cannot shrink bytes by more than about 1000 to 1, so a longer claim is a damaged header.
        DEFLATED if length > payload.len().saturating_mul(MAX_RATIO).saturating_add(64) => {
            return Err(invalid("the packed file claims more than its stream could hold"))
        }
        DEFLATED => inflate(payload, length)?,
        _ => return Err(invalid("the packed file uses a codec that is not known")),
    };
    if hash64(&original) != hash {
        return Err(invalid("the packed file is damaged (hash mismatch)"));
    }
    Ok(original)
}

/// Inflates a zlib stream that must give exactly `length` bytes.
fn inflate(payload: &[u8], length: usize) -> io::Result<Vec<u8>> {
    if length == 0 {
        // `pack` stores an empty input as it is, so a stream for one is not from `pack`.
        return Err(invalid("the packed file holds a stream for an empty original"));
    }
    let mut out = vec![0u8; length];
    let mut decoder = fdeflate::Decompressor::new();
    let (mut read, mut written) = (0usize, 0usize);
    while !decoder.is_done() {
        let (used, made) = decoder
            .read(&payload[read..], &mut out, written, true)
            .map_err(|e| invalid(&format!("the packed file is damaged ({e:?})")))?;
        read += used;
        written += made;
        if used == 0 && made == 0 {
            break;
        }
    }
    if !decoder.is_done() || written != length {
        return Err(invalid("the packed file is damaged (the stream does not end at the stated length)"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stream that does not compress.
    fn noise(n: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    fn samples() -> Vec<Vec<u8>> {
        let text: Vec<u8> = "def load_config(path):\n    return parse(read(path))\n".repeat(400).into_bytes();
        vec![Vec::new(), vec![7], b"short".to_vec(), text, noise(10_000), vec![0u8; 100_000]]
    }

    #[test]
    fn every_kind_of_input_comes_back_byte_for_byte() {
        for original in samples() {
            let packed = pack(&original);
            assert!(is_packed(&packed));
            assert_eq!(unpack(&packed).unwrap(), original, "{} bytes", original.len());
        }
    }

    #[test]
    fn repetitive_input_is_deflated_and_noise_is_stored() {
        let samples = samples();
        assert!(pack(&samples[3]).len() < samples[3].len() / 4);
        assert_eq!(pack(&samples[4]).len(), samples[4].len() + HEADER, "noise is stored as it is, plus the header");
        assert_eq!(pack(&samples[4])[4], STORED);
        assert_eq!(pack(&samples[3])[4], DEFLATED);
    }

    #[test]
    fn a_stored_file_is_never_compressed_and_is_checked_all_the_same() {
        for original in samples() {
            let stored = store(&original);
            assert_eq!(stored.len(), original.len() + HEADER);
            assert_eq!(stored[4], STORED);
            assert_eq!(unpack(&stored).unwrap(), original, "{} bytes", original.len());
            for at in (0..stored.len()).step_by(61).chain([stored.len() - 1]) {
                let mut damaged = stored.clone();
                damaged[at] ^= 0x01;
                assert!(unpack(&damaged).is_err(), "byte {at} of {} changed and was accepted", stored.len());
            }
        }
    }

    #[test]
    fn a_changed_byte_anywhere_is_an_error_and_never_a_different_value() {
        for original in samples().into_iter().filter(|s| !s.is_empty()) {
            let packed = pack(&original);
            let step = (packed.len() / 97).max(1);
            for at in (0..packed.len()).step_by(step).chain([packed.len() - 1]) {
                for flip in [0x01u8, 0x80, 0xff] {
                    let mut damaged = packed.clone();
                    damaged[at] ^= flip;
                    match unpack(&damaged) {
                        Err(_) => {}
                        Ok(back) => assert_eq!(back, original, "byte {at} changed and a different value came back"),
                    }
                }
            }
        }
    }

    #[test]
    fn a_cut_file_is_an_error_at_every_length() {
        let original = samples().remove(3);
        let packed = pack(&original);
        for cut in (0..packed.len()).step_by(37).chain([packed.len() - 1]) {
            assert!(unpack(&packed[..cut]).is_err(), "cut at {cut}");
        }
        let mut longer = packed.clone();
        longer.extend_from_slice(b"trailing");
        // A stream with bytes after its end still gives the original, and the hash vouches for it.
        assert_eq!(unpack(&longer).unwrap(), original);
    }

    #[test]
    fn a_header_that_claims_a_huge_length_is_refused_before_any_allocation() {
        let mut packed = pack(&samples().remove(3));
        packed[5..13].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(unpack(&packed).is_err());
        packed[5..13].copy_from_slice(&MAX_ORIGINAL.to_le_bytes());
        assert!(unpack(&packed).is_err(), "within the limit the stream still has to end there");
    }

    #[test]
    fn an_unknown_codec_and_a_wrong_magic_are_errors() {
        let mut packed = pack(&samples().remove(3));
        packed[4] = 9;
        assert!(unpack(&packed).is_err());
        packed[4] = DEFLATED;
        packed[0] = b'X';
        assert!(unpack(&packed).is_err());
        assert!(unpack(b"").is_err());
        assert!(!is_packed(b"LXZ1"));
    }

    #[test]
    fn a_stream_that_runs_past_the_stated_length_is_an_error() {
        let original = samples().remove(3);
        let mut packed = pack(&original);
        packed[5..13].copy_from_slice(&((original.len() - 1) as u64).to_le_bytes());
        assert!(unpack(&packed).is_err());
    }
}
