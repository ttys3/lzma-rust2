//! Peak heap use of `Lzma2ReaderMt::with_memory_budget`, measured by a counting global
//! allocator. Allocations from every thread count, so the tests in this file take turns.

#![cfg(all(feature = "std", feature = "encoder"))]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    io::{Read, Write},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2ReaderMt, Lzma2Writer};

struct Allocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn grow(size: usize) {
    let current = CURRENT.fetch_add(size, Ordering::SeqCst) + size;
    PEAK.fetch_max(current, Ordering::SeqCst);
}

fn shrink(size: usize) {
    CURRENT.fetch_sub(size, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrink(layout.size());
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            if size > layout.size() {
                grow(size - layout.size());
            } else {
                shrink(layout.size() - size);
            }
        }
        ptr
    }
}

/// The most the heap held, over what it held before, while `f` ran.
fn heap_peak<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let before = CURRENT.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let value = f();
    (PEAK.load(Ordering::SeqCst) - before, value)
}

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// What the budget leaves out: the threads' bookkeeping, the result channel, the work
/// queue and the map of results waiting for their turn.
const SLACK: usize = 64 * KIB;

/// Text and pseudo-random bytes in turn, 16 KiB each.
fn mixed_data(len: usize) -> Vec<u8> {
    let text = std::fs::read("tests/data/pg100.txt").unwrap();
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|i| {
            if (i / (16 * KIB)) % 2 == 0 {
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

/// A stream of independent work units of the given sizes, each encoded on its own.
fn units_stream(data: &[u8], sizes: &[usize], dict_size: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    let mut at = 0;
    for &size in sizes {
        let mut options = Lzma2Options::with_preset(1);
        options.lzma_options.dict_size = dict_size;
        let mut writer = Lzma2Writer::new(Vec::new(), options);
        writer.write_all(&data[at..at + size]).unwrap();
        let unit = writer.finish().unwrap();
        stream.extend_from_slice(&unit[..unit.len() - 1]);
        at += size;
    }
    stream.push(0x00);
    stream
}

/// Reads `reader` to its end, checking every byte against `expected` without holding
/// more than one buffer of output.
fn check_output(mut reader: impl Read, expected: &[u8], buf: &mut [u8]) {
    let mut at = 0;
    loop {
        let n = reader.read(buf).unwrap();
        if n == 0 {
            break;
        }
        assert!(buf[..n] == expected[at..at + n], "output differs at {at}");
        at += n;
    }
    assert_eq!(at, expected.len());
}

struct Measured {
    /// Peak with the budget.
    peak: usize,
    /// Units decoded on the workers.
    units_on_workers: u64,
    /// Peak of the single-threaded reader on the same stream.
    single_peak: usize,
    /// Peak of the reader without a budget.
    unbounded_peak: usize,
}

fn measure(stream: &[u8], dict_size: u32, expected: &[u8], budget: u64) -> Measured {
    let mut buf = vec![0u8; 64 * KIB];
    let (peak, units_on_workers) = heap_peak(|| {
        let mut reader = Lzma2ReaderMt::with_memory_budget(stream, dict_size, None, 4, budget);
        check_output(&mut reader, expected, &mut buf);
        reader.chunk_count()
    });
    let (single_peak, ()) = heap_peak(|| {
        let reader = Lzma2Reader::new(stream, dict_size, None);
        check_output(reader, expected, &mut buf);
    });
    let (unbounded_peak, ()) = heap_peak(|| {
        let reader = Lzma2ReaderMt::new(stream, dict_size, None, 4);
        check_output(reader, expected, &mut buf);
    });
    println!(
        "budget {budget}: peak {peak}, single-threaded {single_peak}, unbounded {unbounded_peak}, \
         {units_on_workers} units on the workers"
    );
    Measured {
        peak,
        units_on_workers,
        single_peak,
        unbounded_peak,
    }
}

/// Streams whose units all fit the budget: every unit is decoded on a worker, the reader
/// stays within the budget, and without one it would not have.
#[test]
fn units_that_fit_stay_within_the_budget() {
    let _turn = ONE_AT_A_TIME.lock().unwrap();

    struct Case {
        name: &'static str,
        sizes: Vec<usize>,
        dict_size: u32,
        budget: u64,
    }
    let cases = [
        Case {
            name: "small units, then large ones",
            sizes: [vec![64 * KIB; 8], vec![2 * MIB; 3]].concat(),
            dict_size: 4 * MIB as u32,
            budget: 12 * MIB as u64,
        },
        Case {
            name: "small and large in turn",
            sizes: [64 * KIB, MIB].repeat(6),
            dict_size: MIB as u32,
            budget: 4 * MIB as u64,
        },
        Case {
            name: "a budget below the dictionary",
            sizes: vec![256 * KIB; 16],
            dict_size: 64 * MIB as u32,
            budget: 4 * MIB as u64,
        },
    ];
    for case in cases {
        let data = mixed_data(case.sizes.iter().sum());
        let stream = units_stream(&data, &case.sizes, case.dict_size);
        let measured = measure(&stream, case.dict_size, &data, case.budget);
        assert_eq!(
            measured.units_on_workers,
            case.sizes.len() as u64,
            "{}",
            case.name
        );
        assert!(
            measured.peak <= case.budget as usize + SLACK,
            "{}: peak {} over the budget {}",
            case.name,
            measured.peak,
            case.budget
        );
        if case.dict_size < 64 * MIB as u32 {
            assert!(
                measured.unbounded_peak > case.budget as usize + SLACK,
                "{}: unbounded peak {} is within the budget anyway",
                case.name,
                measured.unbounded_peak
            );
        }
    }
}

/// Thousands of one-byte units against a large dictionary: each costs a minimal window
/// and a decoder, not a dictionary.
#[test]
fn many_tiny_units_stay_within_the_budget() {
    let _turn = ONE_AT_A_TIME.lock().unwrap();

    let data: Vec<u8> = (0..2000).map(|i| (i % 251) as u8).collect();
    let mut stream = Vec::new();
    for &byte in &data {
        stream.extend_from_slice(&[0x01, 0x00, 0x00, byte]);
    }
    stream.push(0x00);
    let budget = MIB as u64;
    let measured = measure(&stream, 64 * MIB as u32, &data, budget);
    assert_eq!(measured.units_on_workers, 2000);
    assert!(
        measured.peak <= budget as usize + SLACK,
        "peak {} over the budget {budget}",
        measured.peak
    );
}

/// A unit that does not fit the budget on its own goes to the calling thread, after the
/// units before it: the peak is the budget or what the single-threaded reader needs,
/// whichever is larger.
#[test]
fn a_unit_over_the_budget_costs_what_single_threaded_does() {
    let _turn = ONE_AT_A_TIME.lock().unwrap();

    let sizes = [vec![64 * KIB; 4], vec![8 * MIB], vec![64 * KIB; 4]].concat();
    let dict_size = 8 * MIB as u32;
    let data = mixed_data(sizes.iter().sum());
    let stream = units_stream(&data, &sizes, dict_size);
    let budget = 2 * MIB as u64;
    let measured = measure(&stream, dict_size, &data, budget);
    assert_eq!(measured.units_on_workers, 4);
    let bound = (budget as usize).max(measured.single_peak) + SLACK;
    assert!(
        measured.peak <= bound,
        "peak {} over {bound} (single-threaded {})",
        measured.peak,
        measured.single_peak
    );
}

/// The budget charges a decoder 128 KiB besides its window; check that covers both cores
/// with the largest probability model (`lc + lp = 4`).
#[test]
fn a_decoder_needs_its_window_and_128_kib() {
    let _turn = ONE_AT_A_TIME.lock().unwrap();

    let data = mixed_data(64 * KIB);
    let mut options = Lzma2Options::with_preset(6);
    options.lzma_options.dict_size = MIB as u32;
    options.lzma_options.lc = 4;
    options.lzma_options.lp = 0;
    let mut writer = Lzma2Writer::new(Vec::new(), options);
    writer.write_all(&data).unwrap();
    let stream = writer.finish().unwrap();

    let mut buf = vec![0u8; 64 * KIB];
    let (default_core, ()) = heap_peak(|| {
        check_output(
            Lzma2Reader::new(stream.as_slice(), MIB as u32, None),
            &data,
            &mut buf,
        );
    });
    let (portable_core, ()) = heap_peak(|| {
        check_output(
            Lzma2Reader::new_portable(stream.as_slice(), MIB as u32, None),
            &data,
            &mut buf,
        );
    });
    println!("default core {default_core}, portable core {portable_core}");
    let window = 64 * KIB;
    for peak in [default_core, portable_core] {
        assert!(peak <= window + 128 * KIB, "{peak}");
    }
}
