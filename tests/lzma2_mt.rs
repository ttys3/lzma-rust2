use std::{
    io::{Cursor, Read, Write},
    num::{NonZero, NonZeroU64},
    sync::{Arc, Mutex},
};

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2ReaderMt, Lzma2WriterMt};

static EXECUTABLE: &str = "tests/data/executable.exe";
static PG100: &str = "tests/data/pg100.txt";
static PG6800: &str = "tests/data/pg6800.txt";

/// A sink that can be read while the writer still holds it.
#[derive(Clone, Default)]
struct SharedSink(Arc<Mutex<Vec<u8>>>);

impl Write for SharedSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn flush_writes_out_every_chunk() {
    let data = std::fs::read(PG100).unwrap();
    let chunk_size = 128 * 1024;

    for num_workers in [1, 4] {
        let mut options = Lzma2Options::with_preset(6);
        options.lzma_options.dict_size = chunk_size as u32;
        options.set_chunk_size(NonZeroU64::new(chunk_size));

        let sink = SharedSink::default();
        let mut writer = Lzma2WriterMt::new(sink.clone(), options, num_workers).unwrap();

        let mut written = 0;

        for chunk in data[..900 * 1024].chunks(300 * 1024) {
            writer.write_all(chunk).unwrap();
            writer.flush().unwrap();
            written += chunk.len();

            // Every chunk that was flushed is complete, so an end marker turns the
            // flushed data into a valid stream.
            let mut compressed = sink.0.lock().unwrap().clone();
            compressed.push(0x00);

            let mut uncompressed = Vec::new();
            Lzma2Reader::new(Cursor::new(compressed.as_slice()), chunk_size as u32, None)
                .read_to_end(&mut uncompressed)
                .unwrap();

            // We don't use assert_eq since the debug output would be too big.
            assert!(uncompressed == data[..written]);
        }

        // Writing has to continue to work after the flushes.
        writer.finish().unwrap();

        let compressed = sink.0.lock().unwrap().clone();
        let mut uncompressed = Vec::new();
        Lzma2Reader::new(Cursor::new(compressed.as_slice()), chunk_size as u32, None)
            .read_to_end(&mut uncompressed)
            .unwrap();

        assert!(uncompressed == data[..written]);
    }
}

fn test_round_trip(path: &str, level: u32) {
    let data = std::fs::read(path).unwrap();
    let data_len = data.len() as u32;

    let mut option = Lzma2Options::with_preset(level);
    let dict_size = option.lzma_options.dict_size;
    option.set_chunk_size(NonZeroU64::new(dict_size as u64));

    let available_parallelism = std::thread::available_parallelism()
        .unwrap_or(NonZero::new(1).unwrap())
        .get()
        .min(256) as u32;

    let mut compressed = Vec::new();

    {
        let mut writer =
            Lzma2WriterMt::new(&mut compressed, option, available_parallelism).unwrap();
        writer.write_all(&data).unwrap();
        writer.finish().unwrap();
    }

    let mut uncompressed = Vec::new();

    {
        let mut reader = Lzma2ReaderMt::new(
            Cursor::new(compressed),
            dict_size,
            None,
            available_parallelism,
        );
        reader.read_to_end(&mut uncompressed).unwrap();

        if dict_size < data_len {
            assert!(reader.chunk_count() > 1);
        }
    }

    // We don't use assert_eq since the debug output would be too big.
    assert!(uncompressed.as_slice() == data);
}

#[test]
fn round_trip_executable_0() {
    test_round_trip(EXECUTABLE, 0);
}

#[test]
fn round_trip_executable_1() {
    test_round_trip(EXECUTABLE, 1);
}

#[test]
fn round_trip_executable_2() {
    test_round_trip(EXECUTABLE, 2);
}

#[test]
fn round_trip_executable_3() {
    test_round_trip(EXECUTABLE, 3);
}

#[test]
fn round_trip_executable_4() {
    test_round_trip(EXECUTABLE, 4);
}

#[test]
fn round_trip_executable_5() {
    test_round_trip(EXECUTABLE, 5);
}

#[test]
fn round_trip_executable_6() {
    test_round_trip(EXECUTABLE, 6);
}

#[test]
fn round_trip_executable_7() {
    test_round_trip(EXECUTABLE, 7);
}

#[test]
fn round_trip_executable_8() {
    test_round_trip(EXECUTABLE, 8);
}

#[test]
fn round_trip_executable_9() {
    test_round_trip(EXECUTABLE, 9);
}

#[test]
fn round_trip_pg100_0() {
    test_round_trip(PG100, 0);
}

#[test]
fn round_trip_pg100_1() {
    test_round_trip(PG100, 1);
}

#[test]
fn round_trip_pg100_2() {
    test_round_trip(PG100, 2);
}

#[test]
fn round_trip_pg100_3() {
    test_round_trip(PG100, 3);
}

#[test]
fn round_trip_pg100_4() {
    test_round_trip(PG100, 4);
}

#[test]
fn round_trip_pg100_5() {
    test_round_trip(PG100, 5);
}

#[test]
fn round_trip_pg100_6() {
    test_round_trip(PG100, 6);
}

#[test]
fn round_trip_pg100_7() {
    test_round_trip(PG100, 7);
}

#[test]
fn round_trip_pg100_8() {
    test_round_trip(PG100, 8);
}

#[test]
fn round_trip_pg100_9() {
    test_round_trip(PG100, 9);
}

#[test]
fn round_trip_pg6800_0() {
    test_round_trip(PG6800, 0);
}

#[test]
fn round_trip_pg6800_1() {
    test_round_trip(PG6800, 1);
}

#[test]
fn round_trip_pg6800_2() {
    test_round_trip(PG6800, 2);
}

#[test]
fn round_trip_pg6800_3() {
    test_round_trip(PG6800, 3);
}

#[test]
fn round_trip_pg6800_4() {
    test_round_trip(PG6800, 4);
}

#[test]
fn round_trip_pg6800_5() {
    test_round_trip(PG6800, 5);
}

#[test]
fn round_trip_pg6800_6() {
    test_round_trip(PG6800, 6);
}

#[test]
fn round_trip_pg6800_7() {
    test_round_trip(PG6800, 7);
}

#[test]
fn round_trip_pg6800_8() {
    test_round_trip(PG6800, 8);
}

#[test]
fn round_trip_pg6800_9() {
    test_round_trip(PG6800, 9);
}

#[test]
fn empty_input_decodes_to_nothing() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut uncompressed = Vec::new();
        let result = Lzma2ReaderMt::new(&[][..], 1 << 16, None, 2)
            .read_to_end(&mut uncompressed)
            .map(|_| uncompressed.len());
        let _ = sender.send(result);
    });

    let result = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the reader did not finish");
    assert_eq!(result.unwrap(), 0);
}

/// Text and pseudo-random bytes in turn, 16 KiB each: the text compresses into LZMA
/// chunks, the noise is stored as uncompressed chunks.
fn mixed_data(len: usize) -> Vec<u8> {
    let text = std::fs::read(PG100).unwrap();
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|i| {
            if (i / (16 * 1024)) % 2 == 0 {
                text[i % text.len()]
            } else {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            }
        })
        .collect()
}

/// A stream of independent 256 KiB work units, several LZMA2 chunks each (half the
/// data is noise, and a chunk holds at most 64 KiB of compressed data).
fn multi_unit_stream(data: &[u8]) -> (Vec<u8>, u32) {
    let unit = 256 * 1024u64;
    let mut options = Lzma2Options::with_preset(6);
    options.lzma_options.dict_size = unit as u32;
    options.set_chunk_size(NonZeroU64::new(unit));
    let dict_size = options.lzma_options.dict_size;
    let mut compressed = Vec::new();
    let mut writer = Lzma2WriterMt::new(&mut compressed, options, 2).unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap();
    (compressed, dict_size)
}

/// `(end offset in the stream, decompressed bytes up to and including the chunk)` of
/// every chunk, from the chunk headers.
fn chunk_ends(stream: &[u8]) -> Vec<(usize, usize)> {
    let mut ends = Vec::new();
    let (mut pos, mut unpacked) = (0, 0);
    while stream[pos] != 0x00 {
        let control = stream[pos];
        let size = |at: usize| u16::from_be_bytes([stream[at], stream[at + 1]]) as usize + 1;
        if control >= 0x80 {
            unpacked += (((control & 0x1F) as usize) << 16) + size(pos + 1);
            let header = if control >= 0xC0 { 6 } else { 5 };
            pos += header + size(pos + 3);
        } else {
            unpacked += size(pos + 1);
            pos += 3 + size(pos + 1);
        }
        ends.push((pos, unpacked));
    }
    ends
}

/// Everything the single-threaded reader returns before it fails: one byte per read,
/// so no read that fails takes decoded bytes down with it.
fn read_single_threaded(stream: &[u8], dict_size: u32) -> (Vec<u8>, Option<std::io::ErrorKind>) {
    let mut reader = Lzma2Reader::new(stream, dict_size, None);
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return (out, None),
            Ok(_) => out.push(byte[0]),
            Err(error) => return (out, Some(error.kind())),
        }
    }
}

fn read_multi_threaded(
    stream: &[u8],
    dict_size: u32,
    workers: u32,
) -> (Vec<u8>, Option<std::io::ErrorKind>) {
    let mut reader = Lzma2ReaderMt::new(stream, dict_size, None, workers);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 5000];
    let result = loop {
        match reader.read(&mut buf) {
            Ok(0) => break (out, None),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(error) => break (out, Some(error.kind())),
        }
    };
    if let Some(kind) = result.1 {
        // A failed stream stays failed.
        assert_eq!(reader.read(&mut buf).unwrap_err().kind(), kind);
    }
    result
}

/// On a damaged stream the multi-threaded reader returns every chunk before the damage,
/// in order, then the error the single-threaded reader reports: never fewer whole chunks
/// (it used to drop the units read ahead, and the decoded part of the failing one), and
/// nothing the single-threaded reader would not return.
fn assert_matches_single_threaded(
    stream: &[u8],
    dict_size: u32,
    ends: &[(usize, usize)],
    damage_at: usize,
    what: &str,
) {
    let clean = ends
        .iter()
        .take_while(|&&(end, _)| end <= damage_at)
        .last()
        .map_or(0, |&(_, unpacked)| unpacked);
    let (single, single_error) = read_single_threaded(stream, dict_size);
    for workers in [1, 2, 4] {
        let (multi, multi_error) = read_multi_threaded(stream, dict_size, workers);
        assert_eq!(multi_error, single_error, "{what}, {workers} workers");
        assert!(
            single.starts_with(&multi),
            "{what}, {workers} workers: {} bytes, not a prefix of the single-threaded {}",
            multi.len(),
            single.len()
        );
        assert!(
            multi.len() >= clean,
            "{what}, {workers} workers: {} bytes, fewer than the {clean} before the damage",
            multi.len()
        );
    }
}

#[test]
fn truncated_streams_return_every_whole_chunk() {
    let data = mixed_data(1024 * 1024);
    let (stream, dict_size) = multi_unit_stream(&data);
    let ends = chunk_ends(&stream);
    assert!(ends.len() > 8, "several chunks per unit: {}", ends.len());

    let mut cuts: Vec<usize> = (1..stream.len()).step_by(stream.len() / 40).collect();
    for &(end, _) in &ends {
        cuts.extend([end - 1, end, end + 1]);
    }
    cuts.retain(|&cut| cut < stream.len());
    for cut in cuts {
        let what = format!("cut at {cut}");
        assert_matches_single_threaded(&stream[..cut], dict_size, &ends, cut, &what);
    }
}

#[test]
fn corrupt_chunks_return_every_chunk_before_them() {
    let data = mixed_data(1024 * 1024);
    let (stream, dict_size) = multi_unit_stream(&data);
    let ends = chunk_ends(&stream);

    for at in (7..stream.len() - 1).step_by(stream.len() / 40) {
        let mut damaged = stream.clone();
        damaged[at] ^= 0x55;
        let what = format!("flip at {at}");
        assert_matches_single_threaded(&damaged, dict_size, &ends, at, &what);
    }
    for &(end, _) in &ends {
        if stream[end] == 0x00 {
            continue;
        }
        let mut damaged = stream.clone();
        damaged[end] = 0x03; // not a control byte
        let what = format!("control at {end}");
        assert_matches_single_threaded(&damaged, dict_size, &ends, end, &what);
    }
}

#[test]
fn a_clean_stream_still_decodes_whole() {
    let data = mixed_data(1024 * 1024);
    let (stream, dict_size) = multi_unit_stream(&data);
    for workers in [1, 2, 4] {
        let (out, error) = read_multi_threaded(&stream, dict_size, workers);
        assert_eq!(error, None);
        assert!(out == data);
    }
}
