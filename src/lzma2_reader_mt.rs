use std::{
    collections::{BTreeMap, VecDeque},
    io,
    io::{Cursor, Read},
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use crate::{
    Lzma2Reader,
    work_queue::{WorkStealingQueue, WorkerHandle},
};

/// The window a decoder starts with. It doubles from there as the output grows, up to the
/// dictionary size, so a decoder that has produced `n` bytes holds at most
/// `max(WINDOW_MIN, 2 * n)` of it.
const WINDOW_MIN: u64 = 64 * 1024;

/// What a decoder holds besides its window, for either core: the probability model (up to
/// 28 KiB) and the buffer for one compressed chunk (64 KiB), rounded up.
const DECODER_OVERHEAD: u64 = 128 * 1024;

/// The smallest buffer a work unit's compressed bytes are gathered in.
const UNIT_BUFFER_MIN: usize = 64 * 1024;

/// The longest chunk header: the control byte, two size fields and the properties byte.
const CHUNK_HEADER_MAX: usize = 6;

const INPUT_HANDED_OVER: &str = "the input is handed over only once nothing more is dispatched";

/// A work unit for a worker thread: its sequence number, the raw compressed bytes of a
/// series of chunks that starts with a dictionary reset, and their decompressed size
/// (from the chunk headers).
struct WorkUnit {
    seq: u64,
    data: Vec<u8>,
    unpacked: usize,
}

/// What a worker hands back for a work unit: the decompressed bytes of every chunk that
/// decoded cleanly, in order, and the error that stopped it, if one did.
struct UnitResult {
    seq: u64,
    data: Vec<u8>,
    error: Option<io::Error>,
}

/// A chunk whose header has been read and whose data has not.
#[derive(Clone, Copy)]
struct ChunkHeader {
    bytes: [u8; CHUNK_HEADER_MAX],
    len: usize,
    unpacked: usize,
    packed: usize,
}

impl ChunkHeader {
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Where the next chunk goes.
#[derive(Debug)]
enum Placement {
    /// It joined the work unit being gathered.
    Placed,
    /// The budget has no room for it until the caller has read earlier output.
    Wait,
    /// The unit it belongs to is not decoded on a worker: it does not fit the budget on
    /// its own, or it outgrew `unit_cap`.
    HandOver,
}

/// The compressed bytes taken from the stream for the unit that is not decoded on a
/// worker, to be decoded on the calling thread once the units before it are out.
struct HandOver {
    gathered: Vec<u8>,
    header: Vec<u8>,
}

/// The input of the decoder that takes over on the calling thread: the bytes already
/// taken from the stream, then the rest of it. Each part is freed once it has been read.
struct Resume<R> {
    gathered: Cursor<Vec<u8>>,
    header: Cursor<Vec<u8>>,
    inner: R,
}

impl<R: Read> Read for Resume<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        for part in [&mut self.gathered, &mut self.header] {
            let bytes_read = part.read(buf)?;
            if part.position() as usize == part.get_ref().len() {
                *part = Cursor::new(Vec::new());
            }
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
        }
        self.inner.read(buf)
    }
}

enum State {
    /// Actively reading from the inner reader and sending work to threads.
    Reading,
    /// No more work will be dispatched: the inner reader has ended or failed, a unit
    /// failed, or the rest of the stream is left to the calling thread. We are now
    /// waiting for the results up to `last_sequence_id`.
    Draining,
    /// All data has been decompressed and returned. The stream is exhausted.
    Finished,
    /// The stream failed; every further read reports the same error again.
    Failed(io::ErrorKind, String),
}

/// A multi-threaded LZMA2 decompressor.
pub struct Lzma2ReaderMt<R: Read> {
    /// The compressed stream, until `single` takes it over.
    inner: Option<R>,
    result_rx: Receiver<UnitResult>,
    result_tx: SyncSender<UnitResult>,
    current_work_unit: Vec<u8>,
    /// Decompressed size of the chunks gathered in `current_work_unit`.
    current_unit_unpacked: usize,
    /// A chunk whose header has been read, waiting for room in the budget.
    pending_chunk: Option<ChunkHeader>,
    next_sequence_to_dispatch: u64,
    next_sequence_to_return: u64,
    last_sequence_id: Option<u64>,
    out_of_order_chunks: BTreeMap<u64, (Vec<u8>, Option<io::Error>)>,
    current_chunk: Cursor<Vec<u8>>,
    /// The error that ended the unit `current_chunk` holds, reported once it is read out.
    pending_error: Option<io::Error>,
    /// The error the inner reader failed with, reported after every unit read before it.
    input_error: Option<io::Error>,
    /// The bytes the reader may hold for decoding in parallel.
    budget: u64,
    /// The largest work unit, in decompressed bytes, decoded on a worker.
    unit_cap: Option<usize>,
    /// The largest window a decoder grows: the dictionary size, rounded like the decoder.
    window_max: u64,
    /// What each unit in flight may hold, in order from `next_sequence_to_return`.
    unit_charges: VecDeque<u64>,
    /// The sum of `unit_charges`.
    in_flight_bytes: u64,
    /// Set once the rest of the stream is left to the calling thread.
    hand_over: Option<HandOver>,
    /// The decoder of the rest of the stream, once it has taken over.
    single: Option<Lzma2Reader<Resume<R>>>,
    shutdown_flag: Arc<AtomicBool>,
    state: State,
    work_queue: WorkStealingQueue<WorkUnit>,
    active_workers: Arc<AtomicU32>,
    max_workers: u32,
    dict_size: u32,
    preset_dict: Option<Arc<Vec<u8>>>,
    worker_handles: Vec<thread::JoinHandle<()>>,
    /// A worker panics when it takes this unit, to exercise the panic path.
    #[cfg(test)]
    panic_on_unit: Option<u64>,
}

impl<R: Read> Lzma2ReaderMt<R> {
    /// Creates a new multi-threaded LZMA2 reader.
    ///
    /// - `inner`: The reader to read compressed data from.
    /// - `dict_size`: The dictionary size in bytes, as specified in the stream properties.
    /// - `preset_dict`: An optional preset dictionary.
    /// - `num_workers`: The maximum number of worker threads for decompression. Currently capped at 256 Threads.
    ///
    /// At most `num_workers + 1` work units are in flight at once: queued, being decoded, or
    /// decoded and waiting for their turn. Each holds its compressed input and, once decoded, its
    /// whole decompressed output, so the memory the reader needs grows with the number of workers
    /// and the size of the largest work unit, not with the size of the stream. Use
    /// [`Lzma2ReaderMt::with_memory_budget`] to bound it in bytes.
    ///
    /// On corrupt or truncated input the reader returns what [`Lzma2Reader`] would decode
    /// before the damage, at chunk granularity: every chunk before the first one that fails
    /// to decode or could not be read in full, in order, then the error.
    pub fn new(inner: R, dict_size: u32, preset_dict: Option<&[u8]>, num_workers: u32) -> Self {
        Self::with_options(
            inner,
            dict_size,
            preset_dict,
            num_workers,
            u64::MAX,
            None,
            #[cfg(test)]
            None,
        )
    }

    /// Creates a multi-threaded LZMA2 reader that holds at most `budget` bytes to decode
    /// in parallel.
    ///
    /// The budget covers what decoding ahead of the caller costs: the compressed bytes of
    /// the work units gathered and in flight, their decompressed output until the caller
    /// has read it, and the decoder of each unit (its window, which grows with the unit's
    /// output up to `dict_size`, and about 128 KiB besides). When the next chunk does not
    /// fit, the reader reads no further ahead until the caller has read earlier output.
    ///
    /// A work unit is not decoded on a worker when it does not fit the budget on its own,
    /// or when it grows past the size 7-Zip's encoder gives a unit for this dictionary
    /// (`4 * dict_size`, clamped to 1 MiB - 256 MiB, at least `dict_size`, rounded up to
    /// 1 MiB; 7-Zip's decoder stops decoding in parallel at the same point). The reader
    /// then returns the units before it and decodes the rest of the stream on the calling
    /// thread, as [`Lzma2Reader`] does. That decoder needs a window of up to `dict_size`
    /// whatever the budget, so a budget below it does not lower the footprint further.
    /// Thread stacks and a few KiB of bookkeeping are not counted.
    ///
    /// Otherwise the reader behaves as [`Lzma2ReaderMt::new`] describes, damaged input
    /// included.
    pub fn with_memory_budget(
        inner: R,
        dict_size: u32,
        preset_dict: Option<&[u8]>,
        num_workers: u32,
        budget: u64,
    ) -> Self {
        Self::with_options(
            inner,
            dict_size,
            preset_dict,
            num_workers,
            budget,
            Some(max_unit_size(dict_size)),
            #[cfg(test)]
            None,
        )
    }

    fn with_options(
        inner: R,
        dict_size: u32,
        preset_dict: Option<&[u8]>,
        num_workers: u32,
        budget: u64,
        unit_cap: Option<usize>,
        #[cfg(test)] panic_on_unit: Option<u64>,
    ) -> Self {
        let max_workers = num_workers.clamp(1, 256);

        let work_queue = WorkStealingQueue::new();
        let (result_tx, result_rx) = mpsc::sync_channel::<UnitResult>(1);
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let active_workers = Arc::new(AtomicU32::new(0));
        let preset_dict = preset_dict.map(|s| s.to_vec()).map(Arc::new);

        let mut reader = Self {
            inner: Some(inner),
            result_rx,
            result_tx,
            current_work_unit: Vec::new(),
            current_unit_unpacked: 0,
            pending_chunk: None,
            next_sequence_to_dispatch: 0,
            next_sequence_to_return: 0,
            last_sequence_id: None,
            out_of_order_chunks: BTreeMap::new(),
            current_chunk: Cursor::new(Vec::new()),
            pending_error: None,
            input_error: None,
            budget,
            unit_cap,
            window_max: (u64::from(dict_size) + 15) & !15,
            unit_charges: VecDeque::new(),
            in_flight_bytes: 0,
            hand_over: None,
            single: None,
            shutdown_flag,
            state: State::Reading,
            work_queue,
            active_workers,
            max_workers,
            dict_size,
            preset_dict,
            worker_handles: Vec::new(),
            #[cfg(test)]
            panic_on_unit,
        };

        reader.spawn_worker_thread();

        reader
    }

    fn spawn_worker_thread(&mut self) {
        let worker_handle = self.work_queue.worker();
        let context = WorkerContext {
            result_tx: self.result_tx.clone(),
            dict_size: self.dict_size,
            preset_dict: self.preset_dict.clone(),
            shutdown_flag: Arc::clone(&self.shutdown_flag),
            active_workers: Arc::clone(&self.active_workers),
            #[cfg(test)]
            panic_on_unit: self.panic_on_unit,
        };

        let handle = thread::spawn(move || worker_thread_logic(worker_handle, context));

        self.worker_handles.push(handle);
    }

    /// The count of independent chunks found inside the compressed file.
    /// This is effectively tha maximum parallelization possible.
    pub fn chunk_count(&self) -> u64 {
        self.next_sequence_to_return
    }

    /// Reads the next chunk's header. A chunk that starts a new work unit dispatches the
    /// unit before it.
    ///
    /// Returns `None` once no chunk follows: the input ended, or its end marker or a byte
    /// that is not a control byte went to the workers with the last unit.
    fn read_chunk_header(&mut self) -> io::Result<Option<ChunkHeader>> {
        let inner = self.inner.as_mut().expect(INPUT_HANDED_OVER);
        let mut header = ChunkHeader {
            bytes: [0; CHUNK_HEADER_MAX],
            len: 1,
            unpacked: 0,
            packed: 0,
        };
        match inner.read_exact(&mut header.bytes[..1]) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                // Clean end of stream.
                return Ok(None);
            }
            Err(error) => return Err(error),
        }

        let control = header.bytes[0];

        if control < 0x80 && control != 0x01 && control != 0x02 {
            // The end marker, or not a control byte. The unit takes it to a worker as it
            // is, so the decoder rejects a bad one after every chunk before it, with the
            // error the single-threaded reader reports.
            self.current_work_unit.push(control);
            self.send_work_unit();
            return Ok(None);
        }

        let is_independent_chunk = control >= 0xE0 || control == 0x01;

        // Split work units before independent chunks (but not for the very first chunk).
        if is_independent_chunk && !self.current_work_unit.is_empty() {
            self.current_work_unit.push(0x00);
            self.send_work_unit();
        }

        header.len = match control {
            0xC0.. => 6,
            0x80.. => 5,
            _ => 3,
        };
        let inner = self.inner.as_mut().expect(INPUT_HANDED_OVER);
        inner.read_exact(&mut header.bytes[1..header.len])?;
        let size = |at: usize| {
            usize::from(u16::from_be_bytes([header.bytes[at], header.bytes[at + 1]])) + 1
        };
        if control >= 0x80 {
            header.unpacked = (usize::from(control & 0x1F) << 16) + size(1);
            header.packed = size(3);
        } else {
            header.unpacked = size(1);
            header.packed = header.unpacked;
        }
        Ok(Some(header))
    }

    /// Appends the pending chunk to the work unit being gathered, if the unit may grow by
    /// it. On error the work unit holds whole chunks only.
    fn place_pending_chunk(&mut self) -> io::Result<Placement> {
        let header = self.pending_chunk.expect("a chunk is pending");
        let unpacked = self.checked_unpacked_size(header.unpacked)?;
        if self.unit_cap.is_some_and(|cap| unpacked > cap) {
            return Ok(Placement::HandOver);
        }

        // One byte to spare: the end marker that closes the unit never reallocates.
        let needed = self.current_work_unit.len() + header.len + header.packed + 1;
        if !self.reserve_unit_buffer(needed, unpacked) {
            return Ok(if self.units_in_flight() > 0 {
                Placement::Wait
            } else {
                Placement::HandOver
            });
        }

        self.pending_chunk = None;
        let chunk_start = self.current_work_unit.len();
        self.current_work_unit.extend_from_slice(header.bytes());
        let data_start = self.current_work_unit.len();
        self.current_work_unit.resize(data_start + header.packed, 0);
        let inner = self.inner.as_mut().expect(INPUT_HANDED_OVER);
        if let Err(error) = inner.read_exact(&mut self.current_work_unit[data_start..]) {
            self.current_work_unit.truncate(chunk_start);
            return Err(error);
        }

        self.current_unit_unpacked = unpacked;
        Ok(Placement::Placed)
    }

    /// Makes room for `needed` compressed bytes in the work unit being gathered, if the
    /// budget lets the unit grow to them with `unpacked` decompressed bytes.
    fn reserve_unit_buffer(&mut self, needed: usize, unpacked: usize) -> bool {
        let room = self.budget.saturating_sub(self.in_flight_bytes);
        let capacity = self.current_work_unit.capacity();
        if needed <= capacity {
            return self.unit_cost(capacity, unpacked) <= room;
        }

        // Grow geometrically while the budget allows it, and to exactly what is needed
        // close to the limit.
        let grown = needed.max(capacity.saturating_mul(2)).max(UNIT_BUFFER_MIN);
        let target = if self.unit_cost(grown, unpacked) <= room {
            grown
        } else if self.unit_cost(needed, unpacked) <= room {
            needed
        } else {
            return false;
        };
        let len = self.current_work_unit.len();
        self.current_work_unit.reserve_exact(target - len);
        true
    }

    /// The most a work unit holds from dispatch until its output is read: its compressed
    /// input, its output, and the decoder that turns one into the other.
    fn unit_cost(&self, packed_capacity: usize, unpacked: usize) -> u64 {
        let preset = self.preset_dict.as_ref().map_or(0, |preset| preset.len());
        let window = (unpacked as u64)
            .saturating_add(preset as u64)
            .saturating_mul(2)
            .max(WINDOW_MIN)
            .min(self.window_max);
        (packed_capacity as u64)
            .saturating_add(unpacked as u64)
            .saturating_add(window)
            .saturating_add(DECODER_OVERHEAD)
    }

    /// Units dispatched to the workers and not yet returned to the caller: queued, being
    /// decoded, in the result channel, or waiting in `out_of_order_chunks`.
    fn units_in_flight(&self) -> u64 {
        self.next_sequence_to_dispatch - self.next_sequence_to_return
    }

    /// The current work unit's decompressed size with one more chunk of `size` bytes.
    fn checked_unpacked_size(&self, size: usize) -> io::Result<usize> {
        self.current_unit_unpacked.checked_add(size).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "LZMA2 work unit unpacked size exceeds the platform limit",
            )
        })
    }

    /// Sends the current work unit to the workers.
    fn send_work_unit(&mut self) {
        if self.current_work_unit.is_empty() {
            return;
        }

        let data = core::mem::take(&mut self.current_work_unit);
        let unpacked = core::mem::take(&mut self.current_unit_unpacked);
        let charge = self.unit_cost(data.capacity(), unpacked);

        let seq = self.next_sequence_to_dispatch;
        if !self.work_queue.push(WorkUnit {
            seq,
            data,
            unpacked,
        }) {
            // The queue is closed only once the reader has failed or is being dropped.
            self.input_error = Some(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker threads have shut down",
            ));
            self.stop_dispatching();
            return;
        }
        self.unit_charges.push_back(charge);
        self.in_flight_bytes = self.in_flight_bytes.saturating_add(charge);

        // We spawn a new thread if we have work queued, no available workers, and haven't reached
        // the maximal allowed parallelism yet.
        let spawned_workers = self.worker_handles.len() as u32;
        let active_workers = self.active_workers.load(Ordering::Acquire);
        let queue_len = self.work_queue.len();

        // Spawn another worker when more units are queued than there are idle
        // workers to pick them up. `active_workers` is only raised by a worker
        // *after* it stole a unit, so a parked worker that has not woken up yet
        // still counts as idle here and is not doubled up needlessly.
        let idle_workers = spawned_workers.saturating_sub(active_workers) as usize;
        if queue_len > idle_workers && spawned_workers < self.max_workers {
            self.spawn_worker_thread();
        }

        self.next_sequence_to_dispatch += 1;
    }

    /// No more work is dispatched; the reader waits for the units already sent.
    fn stop_dispatching(&mut self) {
        // Input without a chunk dispatches nothing to wait for.
        self.last_sequence_id = self.next_sequence_to_dispatch.checked_sub(1);
        self.state = State::Draining;
    }

    /// The input failed: every chunk read in full before the failure still decodes, as it
    /// does single-threaded, and the error comes after them.
    fn end_input(&mut self, error: io::Error) {
        self.pending_chunk = None;
        if !self.current_work_unit.is_empty() {
            self.current_work_unit.push(0x00);
        }
        self.send_work_unit();
        self.input_error = Some(error);
        self.stop_dispatching();
    }

    /// Leaves the rest of the stream, from the pending chunk on, to the calling thread,
    /// once the units in flight are out.
    fn hand_over(&mut self) {
        let header = self.pending_chunk.take().expect("a chunk is pending");
        self.hand_over = Some(HandOver {
            gathered: core::mem::take(&mut self.current_work_unit),
            header: header.bytes().to_vec(),
        });
        self.current_unit_unpacked = 0;
        self.stop_dispatching();
        // The workers finish what is queued, then exit.
        self.work_queue.close();
    }

    /// Files a worker's result. A failed unit ends the stream there: nothing after it is
    /// dispatched any more, and results of later units are dropped.
    fn accept_result(&mut self, result: UnitResult) {
        if self.last_sequence_id.is_some_and(|last| result.seq > last)
            && !matches!(self.state, State::Reading)
        {
            return;
        }
        // The worker has dropped the unit's input and its decoder; only the output is left.
        let index = result.seq - self.next_sequence_to_return;
        if let Some(charge) = self.unit_charges.get_mut(index as usize) {
            self.in_flight_bytes -= *charge;
            *charge = result.data.capacity() as u64;
            self.in_flight_bytes += *charge;
        }
        if result.error.is_some() {
            if matches!(self.state, State::Reading) {
                self.current_work_unit.clear();
                self.current_unit_unpacked = 0;
                self.pending_chunk = None;
                self.state = State::Draining;
            }
            self.last_sequence_id = Some(
                self.last_sequence_id
                    .map_or(result.seq, |last| last.min(result.seq)),
            );
            let last = result.seq;
            self.out_of_order_chunks.retain(|&seq, _| seq <= last);
        }
        self.out_of_order_chunks
            .insert(result.seq, (result.data, result.error));
    }

    /// The next unit's output, in order: its bytes and the error that ended it early.
    /// `None` once every unit there is has been returned.
    fn next_unit(&mut self) -> Option<(Vec<u8>, Option<io::Error>)> {
        loop {
            // Always check for already-received chunks first.
            if let Some(result) = self
                .out_of_order_chunks
                .remove(&self.next_sequence_to_return)
            {
                self.next_sequence_to_return += 1;
                if let Some(charge) = self.unit_charges.pop_front() {
                    self.in_flight_bytes -= charge;
                }
                return Some(result);
            }

            match self.state {
                State::Reading => {
                    // First, always try to receive a result without blocking.
                    // This keeps the pipeline moving and avoids unnecessary blocking on I/O.
                    // The reader holds a sender itself, so the channel never disconnects.
                    if let Ok(result) = self.result_rx.try_recv() {
                        self.accept_result(result);
                        continue;
                    }

                    if self.pending_chunk.is_some() {
                        match self.place_pending_chunk() {
                            Ok(Placement::Placed) => continue,
                            // Every unit in flight yields a result, and a result frees
                            // the unit's input and decoder at once, its output once the
                            // caller has read it.
                            Ok(Placement::Wait) => {}
                            Ok(Placement::HandOver) => {
                                self.hand_over();
                                continue;
                            }
                            Err(error) => {
                                self.end_input(error);
                                continue;
                            }
                        }
                    } else if self.work_queue.len() < self.max_workers as usize
                        && self.units_in_flight() <= u64::from(self.max_workers)
                    {
                        // Keep the workers fed: read ahead while fewer units are queued
                        // than there are workers. Waiting for a result as soon as the
                        // queue is non-empty races with the worker that is about to
                        // steal that unit, and losing the race blocks the reader until
                        // the unit is fully decoded while every other worker idles.
                        //
                        // But never let more than `max_workers + 1` units be in flight:
                        // a unit that finishes ahead of its turn waits, whole, in
                        // `out_of_order_chunks`, and while one slow unit holds up the
                        // order the queue keeps draining, so a queue-length check alone
                        // let every later unit's output pile up behind it. One header
                        // dispatches at most one unit, so checking before it is enough.
                        match self.read_chunk_header() {
                            Ok(Some(header)) => self.pending_chunk = Some(header),
                            Ok(None) => {
                                // Send any remaining data as the final work unit.
                                self.send_work_unit();
                                self.stop_dispatching();
                            }
                            Err(error) => self.end_input(error),
                        }
                        continue;
                    }

                    // Now we MUST wait for a result to make progress. Every
                    // dispatched unit yields one, a worker panic included.
                    if let Ok(result) = self.result_rx.recv() {
                        self.accept_result(result);
                    }
                }
                State::Draining => {
                    if self
                        .last_sequence_id
                        .is_none_or(|last| self.next_sequence_to_return > last)
                    {
                        return None;
                    }

                    // In Draining state, we only wait for results.
                    if let Ok(result) = self.result_rx.recv() {
                        self.accept_result(result);
                    }
                }
                State::Finished | State::Failed(..) => return None,
            }
        }
    }

    /// Ends the stream with `error`. Nothing after the failure is needed any more, so the
    /// workers stop once their current unit is done.
    fn fail(&mut self, error: io::Error) -> io::Error {
        self.state = State::Failed(error.kind(), error.to_string());
        self.shutdown_flag.store(true, Ordering::Release);
        self.work_queue.close();
        error
    }
}

/// 7-Zip's limit on a work unit's decompressed size (`outBlockMax` in
/// `Lzma2Decoder.cpp`): the block size its multi-threaded encoder uses for the
/// dictionary.
fn max_unit_size(dict_size: u32) -> usize {
    const MIB: u64 = 1 << 20;
    let dict_size = u64::from(dict_size);
    let size = (dict_size << 2).clamp(MIB, 256 * MIB).max(dict_size);
    usize::try_from(size.div_ceil(MIB) * MIB).unwrap_or(usize::MAX)
}

/// What every worker thread shares with the reader.
struct WorkerContext {
    result_tx: SyncSender<UnitResult>,
    dict_size: u32,
    preset_dict: Option<Arc<Vec<u8>>>,
    shutdown_flag: Arc<AtomicBool>,
    active_workers: Arc<AtomicU32>,
    #[cfg(test)]
    panic_on_unit: Option<u64>,
}

/// The logic for a single worker thread.
fn worker_thread_logic(worker_handle: WorkerHandle<WorkUnit>, context: WorkerContext) {
    while !context.shutdown_flag.load(Ordering::Acquire) {
        let Some(unit) = worker_handle.steal() else {
            // No more work available and queue is closed
            break;
        };
        context.active_workers.fetch_add(1, Ordering::Release);

        let seq = unit.seq;
        // A panic must still produce a result: the reader waits for every unit it sent.
        let (data, error) = panic::catch_unwind(AssertUnwindSafe(|| decode_unit(unit, &context)))
            .unwrap_or_else(|_| {
                (
                    Vec::new(),
                    Some(io::Error::other("LZMA2 worker thread panicked")),
                )
            });
        let sent = context
            .result_tx
            .send(UnitResult { seq, data, error })
            .is_ok();

        context.active_workers.fetch_sub(1, Ordering::Release);
        if !sent {
            break;
        }
    }
}

/// Decodes one work unit. Every read returns at the end of a chunk, so a failure keeps
/// exactly the chunks before the one that failed.
fn decode_unit(unit: WorkUnit, context: &WorkerContext) -> (Vec<u8>, Option<io::Error>) {
    #[cfg(test)]
    if context.panic_on_unit == Some(unit.seq) {
        panic!("test panic in work unit {}", unit.seq);
    }

    let mut reader = Lzma2Reader::new(
        unit.data.as_slice(),
        context.dict_size,
        context.preset_dict.as_deref().map(|v| v.as_slice()),
    );

    // The chunk headers say how much comes out, so the result buffer is sized once
    // instead of growing (and copying) its way there. The reader checked that the sum
    // fits a `usize`.
    let mut decompressed_data = Vec::new();
    if decompressed_data.try_reserve_exact(unit.unpacked).is_err() {
        return (
            decompressed_data,
            Some(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "LZMA2 work unit output does not fit in memory",
            )),
        );
    }
    decompressed_data.resize(unit.unpacked, 0);

    let mut filled = 0;
    while filled < unit.unpacked {
        let error = match reader.read_to_chunk_end(&mut decompressed_data[filled..]) {
            Ok(0) => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "LZMA2 chunk ended before its declared size",
            ),
            Ok(n) => {
                filled += n;
                continue;
            }
            Err(error) => error,
        };
        decompressed_data.truncate(filled);
        return (decompressed_data, Some(error));
    }

    // What follows the last chunk must be the end marker (or, for the last unit of a
    // stream that has none, the error the single-threaded reader reports there too).
    match reader.read(&mut [0u8; 1]) {
        Ok(0) => (decompressed_data, None),
        Ok(_) => (
            decompressed_data,
            Some(io::Error::new(
                io::ErrorKind::InvalidData,
                "LZMA2 work unit holds more data than its chunk headers declare",
            )),
        ),
        Err(error) => (decompressed_data, Some(error)),
    }
}

impl<R: Read> Read for Lzma2ReaderMt<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            if let State::Failed(kind, message) = &self.state {
                return Err(io::Error::new(*kind, message.clone()));
            }

            if let Some(single) = self.single.as_mut() {
                // Chunk by chunk, like the workers, so a failure keeps the chunks before it.
                return match single.read_to_chunk_end(buf) {
                    Ok(bytes_read) => Ok(bytes_read),
                    Err(error) => Err(self.fail(error)),
                };
            }

            let bytes_read = self.current_chunk.read(buf)?;
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
            // The caller has read this unit: free it before the reader gathers more.
            self.current_chunk = Cursor::new(Vec::new());

            if let Some(error) = self.pending_error.take() {
                return Err(self.fail(error));
            }

            match self.next_unit() {
                Some((data, error)) => {
                    self.current_chunk = Cursor::new(data);
                    self.pending_error = error;
                }
                None => {
                    if let Some(error) = self.input_error.take() {
                        return Err(self.fail(error));
                    }
                    if let Some(hand_over) = self.hand_over.take() {
                        let inner = self.inner.take().expect(INPUT_HANDED_OVER);
                        let input = Resume {
                            gathered: Cursor::new(hand_over.gathered),
                            header: Cursor::new(hand_over.header),
                            inner,
                        };
                        let preset_dict = self.preset_dict.as_deref().map(|v| v.as_slice());
                        self.single = Some(Lzma2Reader::new(input, self.dict_size, preset_dict));
                        continue;
                    }
                    // This is the clean end of the stream.
                    self.state = State::Finished;
                    return Ok(0);
                }
            }
        }
    }
}

impl<R: Read> Drop for Lzma2ReaderMt<R> {
    fn drop(&mut self) {
        self.shutdown_flag.store(true, Ordering::Release);
        self.work_queue.close();
        // Worker threads will exit when the work queue is closed.
        // JoinHandles will be dropped, which is fine since we set the shutdown flag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An LZMA2 stream of stored chunks: one work unit of `first_unit` bytes,
    /// then `small_units` units of one byte each. Every unit starts with a
    /// dictionary-resetting stored chunk (0x01), the rest of a unit continues
    /// with stored chunks that keep the dictionary (0x02).
    fn stored_stream(first_unit: usize, small_units: usize) -> (Vec<u8>, Vec<u8>) {
        fn stored_chunk(stream: &mut Vec<u8>, control: u8, data: &[u8]) {
            let size = u16::try_from(data.len() - 1).unwrap().to_be_bytes();
            stream.push(control);
            stream.extend_from_slice(&size);
            stream.extend_from_slice(data);
        }

        let plain: Vec<u8> = (0..first_unit + small_units)
            .map(|i| (i % 251) as u8)
            .collect();
        let mut stream = Vec::new();
        for (index, chunk) in plain[..first_unit].chunks(64 * 1024).enumerate() {
            stored_chunk(&mut stream, if index == 0 { 0x01 } else { 0x02 }, chunk);
        }
        for byte in &plain[first_unit..] {
            stored_chunk(&mut stream, 0x01, core::slice::from_ref(byte));
        }
        stream.push(0x00);
        (stream, plain)
    }

    fn read_all(mut reader: impl Read, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        reader.read_to_end(&mut out).unwrap();
        out
    }

    /// A slow first unit holds up the order while the small ones behind it
    /// finish at once. Before the bound, a single read kept dispatching them,
    /// and every one of them waited, decoded, in `out_of_order_chunks`.
    #[test]
    fn a_slow_unit_does_not_let_later_results_pile_up() {
        let (stream, plain) = stored_stream(32 * 1024 * 1024, 2000);
        for workers in [1u32, 2, 4] {
            let mut reader = Lzma2ReaderMt::new(stream.as_slice(), 4096, None, workers);
            let mut out = Vec::with_capacity(plain.len());
            let mut buf = vec![0u8; 4096];
            loop {
                let n = reader.read(&mut buf).unwrap();
                assert!(
                    reader.units_in_flight() <= u64::from(workers) + 1,
                    "{workers} workers: {} units in flight",
                    reader.units_in_flight()
                );
                assert!(reader.out_of_order_chunks.len() <= workers as usize);
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            assert!(out == plain, "{workers} workers: output differs");
        }
    }

    /// What the units in flight, the unit being gathered and the output the caller is
    /// reading may hold never exceeds the budget, and every unit that fits it is decoded
    /// on a worker.
    #[test]
    fn units_in_flight_stay_within_the_budget() {
        // Two one-byte units fit (each is charged a 64 KiB buffer, a 4 KiB window and a
        // decoder), five could be in flight.
        let (stream, plain) = stored_stream(0, 3000);
        let budget = 512 * 1024;
        for workers in [1u32, 2, 4] {
            let mut reader =
                Lzma2ReaderMt::with_memory_budget(stream.as_slice(), 4096, None, workers, budget);
            let mut out = Vec::with_capacity(plain.len());
            let mut buf = vec![0u8; 7];
            loop {
                let n = reader.read(&mut buf).unwrap();
                let gathering = match reader.current_work_unit.capacity() {
                    0 => 0,
                    capacity => reader.unit_cost(capacity, reader.current_unit_unpacked),
                };
                let reading = reader.current_chunk.get_ref().capacity() as u64;
                assert!(
                    reader.in_flight_bytes + gathering + reading <= budget,
                    "{workers} workers: {} in flight, {gathering} gathered, {reading} read",
                    reader.in_flight_bytes
                );
                assert_eq!(
                    reader.in_flight_bytes,
                    reader.unit_charges.iter().sum::<u64>()
                );
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            assert!(out == plain, "{workers} workers: output differs");
            assert!(reader.single.is_none(), "{workers} workers: handed over");
            assert_eq!(reader.chunk_count(), 3000);
        }
    }

    /// A unit that does not fit the budget on its own is decoded on the calling thread,
    /// after the units before it, and so is everything after it.
    #[test]
    fn a_unit_over_the_budget_is_decoded_on_the_calling_thread() {
        // 3000 one-byte units, a 1 MiB unit, then 3000 more.
        let (head, head_plain) = stored_stream(0, 3000);
        let (tail, tail_plain) = stored_stream(1024 * 1024, 3000);
        let mut stream = head[..head.len() - 1].to_vec();
        stream.extend_from_slice(&tail);
        let plain = [head_plain, tail_plain].concat();

        for workers in [1u32, 2, 4] {
            let mut reader = Lzma2ReaderMt::with_memory_budget(
                stream.as_slice(),
                4096,
                None,
                workers,
                1024 * 1024,
            );
            let out = read_all(&mut reader, plain.len());
            assert!(out == plain, "{workers} workers: output differs");
            assert!(
                reader.single.is_some(),
                "{workers} workers: not handed over"
            );
            assert_eq!(reader.chunk_count(), 3000, "{workers} workers");
        }
    }

    /// A unit past 7-Zip's limit for the dictionary is decoded on the calling thread
    /// whatever the budget; `new` keeps every unit on the workers.
    #[test]
    fn a_unit_past_the_cap_is_decoded_on_the_calling_thread() {
        let (stream, plain) = stored_stream(2 * 1024 * 1024 + 1, 3);
        assert_eq!(max_unit_size(4096), 1024 * 1024);

        let mut reader =
            Lzma2ReaderMt::with_memory_budget(stream.as_slice(), 4096, None, 2, u64::MAX);
        assert!(read_all(&mut reader, plain.len()) == plain);
        assert!(reader.single.is_some());

        let mut reader = Lzma2ReaderMt::new(stream.as_slice(), 4096, None, 2);
        assert!(read_all(&mut reader, plain.len()) == plain);
        assert!(reader.single.is_none());
    }

    #[test]
    fn max_unit_size_follows_7zip() {
        const MIB: usize = 1024 * 1024;
        assert_eq!(max_unit_size(64 * 1024), MIB);
        assert_eq!(max_unit_size(16 * MIB as u32), 64 * MIB);
        assert_eq!(max_unit_size(64 * MIB as u32), 256 * MIB);
        assert_eq!(max_unit_size(100 * MIB as u32), 256 * MIB);
        assert_eq!(max_unit_size(300 * MIB as u32 + 1), 301 * MIB);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(max_unit_size(u32::MAX), 4096 * MIB);
    }

    /// A worker that panics fails its unit the way a decoding error does: the units
    /// before it come out whole, then the error, and the reader does not wait forever
    /// for a result that never comes.
    #[test]
    fn a_worker_panic_fails_its_unit() {
        let (stream, plain) = stored_stream(64 * 1024, 3);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = Lzma2ReaderMt::with_options(
                stream.as_slice(),
                4096,
                None,
                2,
                u64::MAX,
                None,
                Some(2),
            );
            let mut out = Vec::new();
            let error = reader.read_to_end(&mut out).unwrap_err();
            let _ = sender.send((out, error.to_string()));
        });

        let (out, message) = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the reader did not finish");
        assert!(out == plain[..64 * 1024 + 1], "units 0 and 1");
        assert_eq!(message, "LZMA2 worker thread panicked");
    }

    #[test]
    fn rejects_compressed_work_unit_unpacked_size_overflow() {
        // A dependent compressed chunk that declares 2 MiB unpacked and one
        // byte packed. Preloading the accumulator reaches the same boundary as
        // 2,048 such headers do on a 32-bit target, without a target-specific
        // multi-gigabyte allocation.
        let input = [0x9F, 0xFF, 0xFF, 0x00, 0x00, 0x00];
        let mut reader = Lzma2ReaderMt::new(input.as_slice(), 4096, None, 1);
        reader.current_unit_unpacked = usize::MAX - 2 * 1024 * 1024 + 1;

        reader.pending_chunk = reader.read_chunk_header().unwrap();
        let error = reader.place_pending_chunk().unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "LZMA2 work unit unpacked size exceeds the platform limit"
        );
    }
}
