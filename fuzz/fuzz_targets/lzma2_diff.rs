//! Differential fuzzing of the `asm` LZMA2 decoder against the portable one:
//! both must reach the same outcome, and their outputs must agree on the
//! common prefix (the assembly decoder gives up earlier on a corrupt chunk).
#![no_main]

use std::io::Read;

use libfuzzer_sys::fuzz_target;
use lzma_rust2::Lzma2Reader;

const DICT_SIZE: u32 = 64 << 10;

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

fuzz_target!(|data: &[u8]| {
    let Some((&pattern, stream)) = data.split_first() else {
        return;
    };
    let sizes: &[usize] = match pattern % 4 {
        0 => &[1],
        1 => &[7, 3, 4096],
        2 => &[4096],
        _ => &[65536],
    };
    let (a, a_err) = drain(Lzma2Reader::new(stream, DICT_SIZE, None), sizes);
    let (p, p_err) = drain(Lzma2Reader::new_portable(stream, DICT_SIZE, None), sizes);
    assert_eq!(a_err, p_err, "outcome differs");
    let n = a.len().min(p.len());
    assert!(a[..n] == p[..n], "outputs diverge");
    if !a_err {
        assert_eq!(a.len(), p.len(), "valid stream, different lengths");
    }
});
