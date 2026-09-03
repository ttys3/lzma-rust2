//! Differential tests for the `asm` feature: 7-Zip's assembly LZMA decoder
//! against the portable decoder, on the same streams with the same `read()`
//! sizes. Without the feature (or on another target) both readers are the
//! portable one and the file degrades to a self-consistency check.
#![cfg(feature = "encoder")]

use std::{
    io::{Read, Write},
    num::NonZeroU64,
};

use lzma_rust2::{LZMA2_ASM_DECODER, Lzma2Options, Lzma2Reader, Lzma2ReaderMt, Lzma2Writer};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// A small deterministic generator.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn text(len: usize) -> Vec<u8> {
    let source = std::fs::read("tests/data/pg100.txt").unwrap();
    source.iter().copied().cycle().take(len).collect()
}

fn random_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut lcg = Lcg(seed);
    (0..len).map(|_| lcg.next() as u8).collect()
}

/// 8 KiB of text alternating with 8 KiB of noise: compressible and stored
/// chunks side by side.
fn mixed(len: usize, seed: u64) -> Vec<u8> {
    let t = text(len);
    let r = random_bytes(len, seed);
    (0..len)
        .map(|i| if (i / (8 * KIB)) % 2 == 0 { t[i] } else { r[i] })
        .collect()
}

fn options(dict: u32, lc: u32, lp: u32, pb: u32, preset: u32) -> Lzma2Options {
    let mut options = Lzma2Options::with_preset(preset);
    options.lzma_options.dict_size = dict;
    options.lzma_options.lc = lc;
    options.lzma_options.lp = lp;
    options.lzma_options.pb = pb;
    options
}

fn compress(data: &[u8], options: Lzma2Options) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut writer = Lzma2Writer::new(&mut out, options);
        writer.write_all(data).unwrap();
        writer.finish().unwrap();
    }
    out
}

/// Control bytes of every chunk in `stream`, in order (stops at the first
/// malformed header).
fn chunk_controls(stream: &[u8]) -> Vec<u8> {
    let mut controls = Vec::new();
    let mut pos = 0;
    while pos < stream.len() {
        let control = stream[pos];
        controls.push(control);
        pos += 1;
        if control == 0 {
            break;
        }
        if control >= 0x80 {
            if pos + 4 > stream.len() {
                break;
            }
            let packed = u16::from_be_bytes([stream[pos + 2], stream[pos + 3]]) as usize + 1;
            pos += 4;
            if control >= 0xC0 {
                pos += 1;
            }
            pos += packed;
        } else if control <= 2 {
            if pos + 2 > stream.len() {
                break;
            }
            let size = u16::from_be_bytes([stream[pos], stream[pos + 1]]) as usize + 1;
            pos += 2 + size;
        } else {
            break;
        }
    }
    controls
}

fn dic_buf_size(dict: u32) -> usize {
    ((dict as usize) + 15) & !15
}

/// Reads through `reader` with the given repeating read sizes; returns the
/// bytes obtained before the first error and whether it ended in an error.
fn drain<R: Read>(mut reader: R, sizes: &[usize]) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = vec![0u8; *sizes.iter().max().unwrap()];
    let mut i = 0;
    loop {
        let n = sizes[i % sizes.len()];
        i += 1;
        match reader.read(&mut buf[..n]) {
            Ok(0) => return (out, false),
            Ok(k) => out.extend_from_slice(&buf[..k]),
            Err(_) => return (out, true),
        }
    }
}

/// Decodes `stream` with both cores and checks that they agree.
///
/// Both must reach the same outcome, and on a valid stream the bytes must be
/// identical. On a corrupt stream the assembly decoder gives up as soon as a
/// symbol reads past the chunk, while the portable one may keep producing
/// bytes from its one-filled reads until the chunk end check, so only the
/// common prefix is compared there. Returns the assembly decoder's result.
fn diff(stream: &[u8], dict: u32, sizes: &[usize]) -> (Vec<u8>, bool) {
    let asm = Lzma2Reader::new(stream, dict, None);
    assert_eq!(
        asm.is_asm_core(),
        LZMA2_ASM_DECODER && dic_buf_size(dict) >= 4096,
        "core selection for dict {dict}"
    );
    let portable = Lzma2Reader::new_portable(stream, dict, None);
    assert!(!portable.is_asm_core());

    let (a, a_err) = drain(asm, sizes);
    let (p, p_err) = drain(portable, sizes);
    assert_eq!(
        a_err, p_err,
        "outcome differs (asm err {a_err}, portable err {p_err}) for dict {dict}, sizes {sizes:?}"
    );
    if a_err {
        let n = a.len().min(p.len());
        assert!(
            a[..n] == p[..n],
            "outputs diverge before the error (dict {dict}, sizes {sizes:?})"
        );
    } else {
        assert!(
            a == p,
            "outputs differ on a valid stream (dict {dict}, sizes {sizes:?}, {} vs {} bytes)",
            a.len(),
            p.len()
        );
    }
    (a, a_err)
}

/// Both cores decode `stream` back to `expected`.
fn round_trip(stream: &[u8], dict: u32, sizes: &[usize], expected: &[u8]) {
    let (out, err) = diff(stream, dict, sizes);
    assert!(!err, "valid stream failed (dict {dict}, sizes {sizes:?})");
    assert!(
        out == expected,
        "round trip mismatch (dict {dict}, sizes {sizes:?})"
    );
}

#[test]
fn lc_lp_pb_sweep() {
    let data = text(256 * KIB);
    let dict = 64 * KIB as u32;
    for lc in 0..=4 {
        for lp in 0..=(4 - lc) {
            for pb in [0, 2, 4] {
                let stream = compress(&data, options(dict, lc, lp, pb, 1));
                round_trip(&stream, dict, &[4096], &data);
            }
        }
    }
    // lc = 8 with lp = 0 is the largest literal context the format allows.
    // The encoder's dictionary must be at least as large as lc + lp allows;
    // lc + lp > 4 is rejected by LZMA2, so this stays within the limit.
}

#[test]
fn dictionary_wrap_and_check_dic_size_transition() {
    for dict in [4 * KIB, 64 * KIB, MIB] {
        let data = mixed(3 * dict + 1234, 7);
        let stream = compress(&data, options(dict as u32, 3, 0, 2, 1));
        for sizes in [&[4096][..], &[MIB][..], &[1, 7, 4096, 3, 65536][..]] {
            round_trip(&stream, dict as u32, sizes, &data);
        }
        // The window also wraps while stored chunks are copied in.
        let noise = random_bytes(2 * dict + 77, 11);
        let stream = compress(&noise, options(dict as u32, 3, 0, 2, 1));
        assert!(
            chunk_controls(&stream).iter().any(|&c| c == 1 || c == 2),
            "noise must produce stored chunks"
        );
        round_trip(&stream, dict as u32, &[4096], &noise);
        round_trip(&stream, dict as u32, &[dict - 1], &noise);
    }
}

#[test]
fn stored_chunks_and_state_resets() {
    // Long noise runs make whole chunks incompressible, so the writer stores
    // them and resets the state before the next LZMA chunk.
    let dict = 64 * KIB as u32;
    let mut data = text(100 * KIB);
    data.extend(random_bytes(200 * KIB, 3));
    data.extend(text(100 * KIB));
    data.extend(random_bytes(200 * KIB, 4));
    let stream = compress(&data, options(dict, 3, 0, 2, 6));
    let controls = chunk_controls(&stream);
    assert!(
        controls.iter().any(|&c| c == 1 || c == 2),
        "no stored chunk in {controls:x?}"
    );
    assert!(
        controls.iter().any(|&c| (0xA0..0xC0).contains(&c)),
        "no state reset chunk in {controls:x?}"
    );
    for sizes in [&[1][..], &[7][..], &[4096][..], &[65536][..], &[MIB][..]] {
        round_trip(&stream, dict, sizes, &data);
    }
}

#[test]
fn dictionary_resets_mid_stream() {
    // `chunk_size` makes every chunk of the single-threaded writer
    // independent (dictionary reset); mixed data keeps the chunks small
    // enough for several of them.
    let dict = 64 * KIB as u32;
    let data = mixed(1200 * KIB, 13);
    let mut opts = options(dict, 3, 0, 2, 3);
    opts.set_chunk_size(NonZeroU64::new(dict as u64));
    let stream = compress(&data, opts);
    let resets = chunk_controls(&stream)
        .iter()
        .filter(|&&c| c >= 0xE0)
        .count();
    assert!(
        resets >= 3,
        "expected several dictionary resets, got {resets}"
    );
    for sizes in [&[4096][..], &[1, 7, 4096, 3, 65536][..], &[MIB][..]] {
        round_trip(&stream, dict, sizes, &data);
    }
}

#[test]
fn read_sizes_from_one_byte_up() {
    let dict = 64 * KIB as u32;
    let data = mixed(300 * KIB, 5);
    let stream = compress(&data, options(dict, 3, 1, 2, 6));
    for sizes in [
        &[1][..],
        &[7][..],
        &[4096][..],
        &[65536][..],
        &[MIB][..],
        &[1, 7, 4096, 3, 65536][..],
    ] {
        round_trip(&stream, dict, sizes, &data);
    }
}

#[test]
fn props_change_without_dictionary_reset() {
    // Two streams glued together: the second one's leading 0xE0 chunk is
    // rewritten to 0xC0 (new properties, keep the dictionary). With lc = lp
    // = 0 and a first part whose length is a multiple of 16 the second part
    // decodes unchanged, so the result must equal the concatenation.
    let dict = 64 * KIB as u32;
    let a = text(96 * KIB);
    let b = random_bytes(20 * KIB, 9)
        .iter()
        .map(|&x| b"abcdefgh"[(x % 8) as usize])
        .collect::<Vec<u8>>();
    let mut stream = compress(&a, options(dict, 0, 0, 2, 3));
    assert_eq!(stream.pop(), Some(0), "end marker");
    let mut second = compress(&b, options(dict, 0, 0, 4, 3));
    assert!(second[0] >= 0xE0);
    second[0] = 0xC0 | (second[0] & 0x1F);
    stream.extend_from_slice(&second);
    let expected: Vec<u8> = a.iter().chain(b.iter()).copied().collect();
    for sizes in [&[4096][..], &[1, 7, 4096, 3, 65536][..]] {
        round_trip(&stream, dict, sizes, &expected);
    }
}

#[test]
fn bit_flips_are_handled_alike() {
    let dict = 64 * KIB as u32;
    let data = mixed(64 * KIB, 21);
    let stream = compress(&data, options(dict, 3, 0, 2, 6));
    let mut lcg = Lcg(0xC0FFEE);
    let mut errors = 0;
    for _ in 0..200 {
        let mut corrupt = stream.clone();
        let pos = lcg.below(corrupt.len());
        corrupt[pos] ^= 1 << lcg.below(8);
        let (_, err) = diff(&corrupt, dict, &[4096]);
        errors += usize::from(err);
    }
    assert!(errors > 0, "no flip was ever detected");
}

#[test]
fn truncations_are_handled_alike() {
    let dict = 4 * KIB as u32;
    let data = text(8 * KIB);
    let stream = compress(&data, options(dict, 3, 0, 2, 6));
    assert!(stream.len() > 1024, "stream too small to be interesting");
    for len in 0..stream.len() {
        let (_, err) = diff(&stream[..len], dict, &[4096]);
        assert!(err, "a stream cut at {len} of {} decoded", stream.len());
    }
    round_trip(&stream, dict, &[4096], &data);
}

#[test]
fn control_byte_rewrites_are_handled_alike() {
    // Three independently compressed segments glued together (end markers
    // dropped in between), so the second chunk header sits at a known offset.
    let dict = 4 * KIB as u32;
    let first = compress(&text(4 * KIB), options(dict, 3, 0, 2, 3));
    let second = compress(&text(4 * KIB), options(dict, 3, 0, 2, 3));
    let third = compress(&random_bytes(3000, 5), options(dict, 3, 0, 2, 3));
    let mut stream = first[..first.len() - 1].to_vec();
    let at = stream.len();
    stream.extend_from_slice(&second[..second.len() - 1]);
    stream.extend_from_slice(&third);
    assert!(stream[at] >= 0xE0);
    let mut expected = text(4 * KIB);
    expected.extend(text(4 * KIB));
    expected.extend(random_bytes(3000, 5));
    round_trip(&stream, dict, &[1, 7, 4096], &expected);

    for control in [0x80u8, 0xA0, 0xC0, 0x00, 0x03, 0x7F, 0x01, 0x02] {
        let mut corrupt = stream.clone();
        corrupt[at] = control | (stream[at] & 0x1F);
        diff(&corrupt, dict, &[1, 7, 4096]);
    }
    // Properties that LZMA2 forbids (lc + lp > 4) in the first chunk header.
    let mut corrupt = stream.clone();
    corrupt[5] = 4 + 9;
    let (_, err) = diff(&corrupt, dict, &[4096]);
    assert!(err);
}

#[test]
fn tiny_inputs() {
    let dict = 64 * KIB as u32;
    let (out, err) = diff(&[0x00], dict, &[16]);
    assert!(
        !err && out.is_empty(),
        "the end marker alone is an empty stream"
    );
    let (_, err) = diff(&[], dict, &[16]);
    assert!(err, "a missing end marker is an error");
    round_trip(&[0x01, 0x00, 0x00, b'x', 0x00], dict, &[1], b"x");
    let (_, err) = diff(&[0x80, 0, 0, 0, 0], dict, &[16]);
    assert!(err, "an LZMA chunk without its properties");
}

#[test]
fn preset_dictionary_and_tiny_dictionaries_stay_portable() {
    let data = text(64 * KIB);
    let preset = data[..8 * KIB].to_vec();
    let mut opts = options(64 * KIB as u32, 3, 0, 2, 6);
    opts.lzma_options.preset_dict = Some(preset.clone());
    let stream = compress(&data[8 * KIB..], opts);
    let reader = Lzma2Reader::new(stream.as_slice(), 64 * KIB as u32, Some(&preset));
    assert!(!reader.is_asm_core());
    let (out, err) = drain(reader, &[4096]);
    assert!(!err);
    assert!(out == data[8 * KIB..]);

    let small = text(2000);
    let stream = compress(&small, options(100, 3, 0, 2, 6));
    let reader = Lzma2Reader::new(stream.as_slice(), 100, None);
    assert!(
        !reader.is_asm_core(),
        "dictionaries below 4 KiB stay portable"
    );
    round_trip(&stream, 100, &[4096], &small);
}

#[test]
fn seven_zip_vector() {
    let input = std::fs::read("tests/data/issue_44_7z.lzma2").unwrap();
    let expected = std::fs::read("tests/data/issue_44_7z.bin").unwrap();
    let dict = 8 << 20;
    round_trip(&input, dict, &[MIB], &expected);
    round_trip(&input, dict, &[1, 7, 4096, 3, 65536], &expected);

    let mut mt = Lzma2ReaderMt::new(input.as_slice(), dict, None, 4);
    let mut out = Vec::new();
    mt.read_to_end(&mut out).unwrap();
    assert!(out == expected);
}

/// Random-mutation soak: bit flips, byte edits, truncations and splices of
/// generated streams, checked with the differential rule for as long as
/// `LZMA2_DIFF_SOAK_SECS` says (default 30 s). Ignored by default; a stand-in
/// for `cargo fuzz run lzma2_diff` where cargo-fuzz is not available:
///
/// `LZMA2_DIFF_SOAK_SECS=3600 cargo test --release --features asm --test lzma2_asm_diff -- --ignored soak`
#[test]
#[ignore]
fn random_mutation_soak() {
    use std::time::{Duration, Instant};

    let secs: u64 = std::env::var("LZMA2_DIFF_SOAK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let dict = 64 * KIB as u32;
    let mut pool = Vec::new();
    for (seed, len, preset) in [(1, 16 * KIB, 1), (2, 40 * KIB, 6), (3, 100 * KIB, 3)] {
        pool.push(compress(&mixed(len, seed), options(dict, 3, 0, 2, preset)));
        pool.push(compress(&text(len), options(dict, 0, 2, 0, preset)));
        pool.push(compress(
            &random_bytes(len, seed),
            options(dict, 3, 0, 2, preset),
        ));
    }
    let sizes: [&[usize]; 4] = [&[1], &[7, 3, 4096], &[4096], &[65536]];
    let mut lcg = Lcg(0x5EED);
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut iterations = 0u64;
    while Instant::now() < deadline {
        let mut stream = pool[lcg.below(pool.len())].clone();
        for _ in 0..1 + lcg.below(4) {
            match lcg.below(6) {
                0 | 1 => {
                    let pos = lcg.below(stream.len());
                    stream[pos] ^= 1 << lcg.below(8);
                }
                2 => {
                    let pos = lcg.below(stream.len());
                    stream[pos] = lcg.next() as u8;
                }
                3 => {
                    let pos = lcg.below(stream.len());
                    stream.truncate(pos);
                    if stream.is_empty() {
                        stream.push(lcg.next() as u8);
                    }
                }
                4 => {
                    let pos = lcg.below(stream.len());
                    stream.insert(pos, lcg.next() as u8);
                }
                _ => {
                    let other = &pool[lcg.below(pool.len())];
                    let cut = lcg.below(stream.len());
                    let from = lcg.below(other.len());
                    stream.truncate(cut);
                    stream.extend_from_slice(&other[from..]);
                }
            }
        }
        diff(&stream, dict, sizes[lcg.below(sizes.len())]);
        iterations += 1;
    }
    eprintln!("soak: {iterations} mutated streams in {secs} s");
}
