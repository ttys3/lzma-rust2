use std::{
    io::{Cursor, Read, Write},
    num::{NonZero, NonZeroU64},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use lzma_rust2::{
    DecoderCancelled, Lzma2Options, Lzma2Reader, Lzma2ReaderMt, Lzma2Writer, Lzma2WriterMt,
};

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

/// Setting the cancellation flag ends a multi-threaded read mid-stream with an error that
/// carries `DecoderCancelled`, for this read and every later one.
#[test]
fn cancellation_ends_a_multi_threaded_read() {
    let data = std::fs::read(PG100).unwrap();
    let chunk_size = 256 * 1024;
    let mut options = Lzma2Options::with_preset(1);
    options.lzma_options.dict_size = chunk_size as u32;
    options.set_chunk_size(NonZeroU64::new(chunk_size));
    let mut writer = Lzma2WriterMt::new(Vec::new(), options, 4).unwrap();
    writer.write_all(&data).unwrap();
    let compressed = writer.finish().unwrap();

    let flag = Arc::new(AtomicBool::new(false));
    let mut reader = Lzma2ReaderMt::new(compressed.as_slice(), chunk_size as u32, None, 4);
    reader.set_cancellation(Arc::clone(&flag));
    let mut out = vec![0u8; 64 * 1024];
    reader.read_exact(&mut out).unwrap();
    assert!(out == data[..out.len()]);

    flag.store(true, Ordering::Relaxed);
    for _ in 0..2 {
        let error = reader.read(&mut out).unwrap_err();
        assert!(
            error
                .get_ref()
                .is_some_and(|inner| inner.is::<DecoderCancelled>()),
            "{error}"
        );
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

/// A stream of independent work units of the given sizes, each encoded on its own, so
/// that unit sizes need not be uniform.
fn units_stream(data: &[u8], sizes: &[usize], dict_size: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    let mut at = 0;
    for &size in sizes {
        let mut options = Lzma2Options::with_preset(1);
        options.lzma_options.dict_size = dict_size;
        let mut writer = Lzma2Writer::new(Vec::new(), options);
        writer.write_all(&data[at..at + size]).unwrap();
        let unit = writer.finish().unwrap();
        // Every unit but the last loses its end marker.
        assert_eq!(unit.last(), Some(&0x00));
        stream.extend_from_slice(&unit[..unit.len() - 1]);
        at += size;
    }
    stream.push(0x00);
    stream
}

fn read_multi_threaded(
    stream: &[u8],
    dict_size: u32,
    workers: u32,
    budget: Option<u64>,
) -> (Vec<u8>, Option<std::io::ErrorKind>) {
    let mut reader = match budget {
        Some(budget) => Lzma2ReaderMt::with_memory_budget(stream, dict_size, None, workers, budget),
        None => Lzma2ReaderMt::new(stream, dict_size, None, workers),
    };
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
///
/// Each budget also takes the path where the units from some point on are decoded on the
/// calling thread: `Some(0)` from the start, the others where a unit does not fit.
fn assert_matches_single_threaded(
    stream: &[u8],
    dict_size: u32,
    budgets: &[Option<u64>],
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
    for &budget in budgets {
        for workers in [1, 2, 4] {
            let (multi, multi_error) = read_multi_threaded(stream, dict_size, workers, budget);
            let what = format!("{what}, {workers} workers, budget {budget:?}");
            assert_eq!(multi_error, single_error, "{what}");
            assert!(
                single.starts_with(&multi),
                "{what}: {} bytes, not a prefix of the single-threaded {}",
                multi.len(),
                single.len()
            );
            assert!(
                multi.len() >= clean,
                "{what}: {} bytes, fewer than the {clean} before the damage",
                multi.len()
            );
        }
    }
}

/// Unit sizes in KiB for a stream whose fifth unit does not fit a 1 MiB budget on its
/// own, so the reader hands over in the middle of it.
const UNEVEN_UNITS_KIB: [usize; 9] = [64, 64, 64, 64, 768, 64, 64, 64, 64];
const UNEVEN_BUDGETS: [Option<u64>; 3] = [None, Some(0), Some(1024 * 1024)];
const UNIFORM_BUDGETS: [Option<u64>; 3] = [None, Some(0), Some(2 * 1024 * 1024)];

fn uneven_stream() -> (Vec<u8>, u32, Vec<u8>) {
    let sizes: Vec<usize> = UNEVEN_UNITS_KIB.iter().map(|kib| kib * 1024).collect();
    let data = mixed_data(sizes.iter().sum());
    let dict_size = 256 * 1024;
    (units_stream(&data, &sizes, dict_size), dict_size, data)
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
        assert_matches_single_threaded(
            &stream[..cut],
            dict_size,
            &UNIFORM_BUDGETS,
            &ends,
            cut,
            &what,
        );
    }
}

#[test]
fn truncated_uneven_streams_return_every_whole_chunk() {
    let (stream, dict_size, _) = uneven_stream();
    let ends = chunk_ends(&stream);
    for cut in (1..stream.len()).step_by(stream.len() / 60) {
        let what = format!("cut at {cut}");
        assert_matches_single_threaded(
            &stream[..cut],
            dict_size,
            &UNEVEN_BUDGETS,
            &ends,
            cut,
            &what,
        );
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
        assert_matches_single_threaded(&damaged, dict_size, &UNIFORM_BUDGETS, &ends, at, &what);
    }
    for &(end, _) in &ends {
        if stream[end] == 0x00 {
            continue;
        }
        let mut damaged = stream.clone();
        damaged[end] = 0x03; // not a control byte
        let what = format!("control at {end}");
        assert_matches_single_threaded(&damaged, dict_size, &UNIFORM_BUDGETS, &ends, end, &what);
    }
}

#[test]
fn corrupt_uneven_streams_return_every_chunk_before_them() {
    let (stream, dict_size, _) = uneven_stream();
    let ends = chunk_ends(&stream);
    for at in (7..stream.len() - 1).step_by(stream.len() / 60) {
        let mut damaged = stream.clone();
        damaged[at] ^= 0x55;
        let what = format!("flip at {at}");
        assert_matches_single_threaded(&damaged, dict_size, &UNEVEN_BUDGETS, &ends, at, &what);
    }
}

#[test]
fn a_clean_stream_still_decodes_whole() {
    let data = mixed_data(1024 * 1024);
    let (stream, dict_size) = multi_unit_stream(&data);
    for budget in UNIFORM_BUDGETS {
        for workers in [1, 2, 4] {
            let (out, error) = read_multi_threaded(&stream, dict_size, workers, budget);
            assert_eq!(error, None);
            assert!(out == data, "{workers} workers, budget {budget:?}");
        }
    }
}

/// Whatever the unit layout, the number of workers and the budget, the output is the
/// single-threaded reader's.
#[test]
fn budgets_never_change_the_output() {
    const KIB: usize = 1024;
    let layouts: [&[usize]; 5] = [
        // Small units, then large ones.
        &[16 * KIB, 16 * KIB, 16 * KIB, 16 * KIB, 512 * KIB, 512 * KIB],
        // Large and small in turn.
        &[512 * KIB, 1, 512 * KIB, 1, 512 * KIB, 1],
        // One unit only.
        &[1536 * KIB],
        // Many tiny units.
        &[1; 300],
        // A unit past the cap for its dictionary (1 MiB for 256 KiB).
        &[64 * KIB, 1100 * KIB, 64 * KIB],
    ];
    let budgets = [
        None,
        Some(0),
        Some(256 * 1024),
        Some(1024 * 1024),
        Some(3 * 1024 * 1024),
        Some(u64::MAX),
    ];
    let dict_size = 256 * 1024;
    for (index, layout) in layouts.iter().enumerate() {
        let data = mixed_data(layout.iter().sum());
        let stream = units_stream(&data, layout, dict_size);
        let mut single = Vec::new();
        Lzma2Reader::new(stream.as_slice(), dict_size, None)
            .read_to_end(&mut single)
            .unwrap();
        assert!(single == data, "layout {index}");
        for budget in budgets {
            for workers in [1, 2, 4] {
                let (out, error) = read_multi_threaded(&stream, dict_size, workers, budget);
                assert_eq!(
                    error, None,
                    "layout {index}, {workers} workers, budget {budget:?}"
                );
                assert!(
                    out == data,
                    "layout {index}, {workers} workers, budget {budget:?}"
                );
            }
        }
    }
}

/// The uneven stream exercises what its tests are for: the reader hands over at the unit
/// that does not fit, not before it.
#[test]
fn the_uneven_stream_hands_over_at_its_large_unit() {
    let (stream, dict_size, data) = uneven_stream();
    let expected = [(None, 9), (Some(0), 0), (Some(1024 * 1024), 4)];
    for (budget, units_on_workers) in expected {
        for workers in [1, 2, 4] {
            let mut reader = match budget {
                Some(budget) => Lzma2ReaderMt::with_memory_budget(
                    stream.as_slice(),
                    dict_size,
                    None,
                    workers,
                    budget,
                ),
                None => Lzma2ReaderMt::new(stream.as_slice(), dict_size, None, workers),
            };
            let mut out = Vec::new();
            reader.read_to_end(&mut out).unwrap();
            assert!(out == data);
            assert_eq!(
                reader.chunk_count(),
                units_on_workers,
                "{workers} workers, budget {budget:?}"
            );
        }
    }
}
