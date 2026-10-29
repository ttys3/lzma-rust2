use std::{
    collections::{BTreeMap, VecDeque},
    io,
    io::{Cursor, Read},
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::Duration,
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

/// The largest decompressed size a chunk header can declare: `(0x1F << 16) + 0x10000`.
const CHUNK_UNPACKED_MAX: usize = 2 * 1024 * 1024;

/// The most output the head's worker hands back at once: whole chunks, as many as fit.
const PIECE_MAX: usize = CHUNK_UNPACKED_MAX;

/// The most output the head's worker decodes ahead of the caller. The caller also reads the
/// input, and while a read waits for a slow medium it takes no output: a 1 MiB read at
/// 20 MB/s takes 50 ms, in which a worker decodes 10 MiB of text. With room for less, the
/// worker stopped and waited for the caller after every piece, and a stream written as
/// one unit decoded slower than on one thread. A quarter of the budget, if less.
const HEAD_OUTPUT_AHEAD: usize = 16 * 1024 * 1024;

/// How far the input of the head is kept ahead of its worker once the head has grown past
/// `lookahead_from`. Reading further ahead is how the units after the head are found, and
/// no unit 7-Zip's encoder writes is that large; past it the head is a stream written as
/// one unit, and reading ahead would only hold its input in memory. It keeps the worker
/// busy while the caller waits for a read of up to this much: a caller that reads its
/// source in larger bursts overlaps reading and decoding only in part here.
const STREAM_LOOKAHEAD: u64 = 1024 * 1024;

/// How often a reader waiting for a worker checks its cancellation flag.
const CANCELLATION_CHECK_INTERVAL: Duration = Duration::from_millis(50);

/// Cooperative decoder cancellation, carried through the reader's I/O result.
#[derive(Debug)]
pub struct DecoderCancelled;

impl std::fmt::Display for DecoderCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("decoder cancelled")
    }
}

impl std::error::Error for DecoderCancelled {}

/// A work unit for a worker thread: its sequence number, the raw compressed bytes of a
/// series of chunks that starts with a dictionary reset, and their decompressed size
/// (from the chunk headers).
struct WorkUnit {
    seq: u64,
    data: Vec<u8>,
    unpacked: usize,
}

/// The head as its worker sees it: a work unit whose chunks come in as the reader reads
/// them. The worker decodes each one as it arrives and hands its output back, chunk by
/// chunk.
struct StreamedUnit {
    seq: u64,
    /// Whole chunks in order, then what ends the unit: an end marker, or the byte that
    /// ends the stream. Once the sender is gone nothing more comes; without an end before
    /// it, the input ended there.
    chunks: Receiver<Vec<u8>>,
    /// The bytes of output the worker may decode ahead of the reader: first its whole
    /// allowance, then what the reader takes, piece by piece.
    credits: Receiver<usize>,
    /// Compressed bytes the worker has read and freed.
    freed: Arc<AtomicU64>,
}

/// Work for a worker thread.
enum Work {
    /// A unit gathered whole, decoded in one go.
    Whole(WorkUnit),
    /// The head.
    Streamed(StreamedUnit),
}

/// What a worker hands back for a work unit: the decompressed bytes of every chunk that
/// decoded cleanly, in order, and the error that stopped it, if one did. A streamed unit
/// hands back its output in pieces of whole chunks as they are decoded, with `more` set,
/// then a last result with no bytes and the error that stopped it, if one did.
struct UnitResult {
    seq: u64,
    data: Vec<u8>,
    error: Option<io::Error>,
    more: bool,
}

/// A unit's output: the bytes of every chunk that decoded cleanly, and the error that
/// stopped it, if one did.
type UnitOutput = (Vec<u8>, Option<io::Error>);

/// A chunk whose header has been read and whose data has not.
#[derive(Clone, Copy)]
struct ChunkHeader {
    bytes: [u8; CHUNK_HEADER_MAX],
    len: usize,
    unpacked: usize,
    packed: usize,
    /// It resets the dictionary, so a work unit starts with it.
    starts_unit: bool,
}

impl ChunkHeader {
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// What the stream holds next.
enum Next {
    /// A chunk, whose header has been read.
    Chunk(ChunkHeader),
    /// The end marker, or a byte that is not a control byte. Either ends the stream; it
    /// goes to the decoder as it is, so that a bad one is rejected after every chunk
    /// before it, with the error the single-threaded reader reports.
    End(u8),
    /// The input ended without an end marker.
    Eof,
}

/// Where the next chunk goes.
#[derive(Debug)]
enum Placement {
    /// It joined the work unit being gathered, or went to the head's worker.
    Placed,
    /// The budget has no room for it until the caller has read earlier output, the unit
    /// it belongs to has outgrown `unit_cap` and waits to be the head, or the head's input
    /// is as far ahead of its worker as it may be.
    Wait,
}

/// The unit next in line for the output when no worker has it whole: the first unit of
/// the stream, and any the reader could not read ahead of in time. A worker decodes it as
/// its chunks are read (see [`StreamedUnit`]), so the reader can read on to the units
/// after it meanwhile, and its output is never held whole.
struct Head {
    seq: u64,
    /// Where its chunks go; `None` once it is complete.
    chunks: Option<Sender<Vec<u8>>>,
    /// The worker's side, until the first chunk dispatches it.
    work: Option<StreamedUnit>,
    credits: Sender<usize>,
    freed: Arc<AtomicU64>,
    /// Compressed bytes sent.
    sent: u64,
    /// Decompressed size of the chunks sent.
    unpacked: usize,
    /// Its output as it comes back, piece by piece.
    output: VecDeque<Vec<u8>>,
    /// Set once the worker is done with it, to the error that stopped it if one did.
    end: Option<Option<io::Error>>,
}

impl Head {
    fn complete(&self) -> bool {
        self.chunks.is_none()
    }

    /// Compressed bytes sent that the worker has not freed yet.
    fn ahead(&self) -> u64 {
        self.sent.saturating_sub(self.freed.load(Ordering::Acquire))
    }
}

enum State {
    /// Actively reading from the inner reader and sending work to threads.
    Reading,
    /// No more work will be dispatched: the inner reader has ended or failed, or a unit
    /// failed. We are now waiting for the results up to `last_sequence_id`.
    Draining,
    /// All data has been decompressed and returned. The stream is exhausted.
    Finished,
    /// The stream failed; every further read reports the same error again.
    Failed(io::ErrorKind, String),
    /// The caller cancelled the stream; every further read reports [`DecoderCancelled`].
    Cancelled,
}

/// A multi-threaded LZMA2 decompressor.
pub struct Lzma2ReaderMt<R: Read> {
    inner: R,
    result_rx: Receiver<UnitResult>,
    result_tx: Sender<UnitResult>,
    current_work_unit: Vec<u8>,
    /// Decompressed size of the chunks gathered in `current_work_unit`.
    current_unit_unpacked: usize,
    /// A chunk whose header has been read, waiting for room in the budget.
    pending_chunk: Option<ChunkHeader>,
    next_sequence_to_dispatch: u64,
    next_sequence_to_return: u64,
    last_sequence_id: Option<u64>,
    out_of_order_chunks: BTreeMap<u64, UnitOutput>,
    current_chunk: Cursor<Vec<u8>>,
    /// The error that ended the unit `current_chunk` holds, reported once it is read out.
    pending_error: Option<io::Error>,
    /// The error the inner reader failed with, reported after every unit read before it.
    input_error: Option<io::Error>,
    /// The bytes the reader may hold for decoding in parallel.
    budget: u64,
    /// The largest work unit, in decompressed bytes, decoded whole on a worker.
    unit_cap: Option<usize>,
    /// The decompressed size past which the head's input is kept at most
    /// `STREAM_LOOKAHEAD` ahead of its worker: `unit_cap`, or 7-Zip's limit without one.
    lookahead_from: usize,
    /// The largest window a decoder grows: the dictionary size, rounded like the decoder.
    window_max: u64,
    /// What each unit in flight may hold, in order from `next_sequence_to_return`. The
    /// head's entry is 0: what it holds changes as it is decoded, see `head_cost`.
    unit_charges: VecDeque<u64>,
    /// The sum of `unit_charges`.
    in_flight_bytes: u64,
    /// The unit decoded as it is read, while there is one.
    head: Option<Head>,
    /// The units decoded as they were read.
    units_streamed: u64,
    /// Their sequence numbers.
    #[cfg(test)]
    streamed_units: Vec<u64>,
    shutdown_flag: Arc<AtomicBool>,
    /// The caller's flag, set to request cancellation.
    cancellation: Option<Arc<AtomicBool>>,
    state: State,
    work_queue: WorkStealingQueue<Work>,
    active_workers: Arc<AtomicU32>,
    max_workers: u32,
    dict_size: u32,
    preset_dict: Option<Arc<Vec<u8>>>,
    worker_handles: Vec<thread::JoinHandle<()>>,
    #[cfg(test)]
    hooks: TestHooks,
}

impl<R: Read> Lzma2ReaderMt<R> {
    /// Creates a new multi-threaded LZMA2 reader.
    ///
    /// - `inner`: The reader to read compressed data from.
    /// - `dict_size`: The dictionary size in bytes, as specified in the stream properties.
    /// - `preset_dict`: An optional preset dictionary.
    /// - `num_workers`: The maximum number of worker threads for decompression. Currently capped at 256 Threads.
    ///
    /// The stream is split into work units at its dictionary resets, and the workers decode
    /// them in parallel. The unit next in line for the output is streamed when no worker
    /// has it whole: the first unit of the stream, and any the reader could not read ahead
    /// of in time. A worker decodes it chunk by chunk as the reader reads it, up to 16 MiB
    /// ahead of the caller, and hands its output back in pieces of up to 2 MiB as soon as
    /// they are decoded, while the reader reads on to the units after it. A stream that is
    /// one work unit (single-threaded encoders, 7-Zip with `-mmt1`) therefore never has its
    /// output held whole, and its input is read while it is decoded. Once the streamed unit
    /// has grown past the size 7-Zip's encoder gives a unit for the dictionary (see
    /// [`Lzma2ReaderMt::with_memory_budget`]), its input is read at most 1 MiB ahead of its
    /// worker.
    ///
    /// At most `num_workers + 1` work units are in flight at once, the streamed one
    /// included: queued, being decoded, or decoded and waiting for their turn. Any other
    /// unit holds its compressed input and, once decoded, its whole decompressed output,
    /// so the memory the reader needs grows with the number of workers and the size of the
    /// largest work unit, not with the size of the stream. Use
    /// [`Lzma2ReaderMt::with_memory_budget`] to bound it in bytes.
    ///
    /// On corrupt or truncated input the reader returns what [`Lzma2Reader`] would decode
    /// before the damage, at chunk granularity: every chunk before the first one that fails
    /// to decode or could not be read in full, in order, then the error.
    ///
    /// Dropping the reader stops its workers at the end of the chunk they are decoding and
    /// waits for them to exit. Use [`Lzma2ReaderMt::set_cancellation`] to stop a read that
    /// waits for a worker.
    pub fn new(inner: R, dict_size: u32, preset_dict: Option<&[u8]>, num_workers: u32) -> Self {
        Self::with_options(
            inner,
            dict_size,
            preset_dict,
            num_workers,
            u64::MAX,
            None,
            #[cfg(test)]
            TestHooks::default(),
        )
    }

    /// Creates a multi-threaded LZMA2 reader that holds at most `budget` bytes to decode
    /// in parallel.
    ///
    /// The budget covers what decoding ahead of the caller costs: the compressed bytes of
    /// the work units gathered and in flight, their decompressed output until the caller
    /// has read it, and the decoder of each unit (its window, which grows with the unit's
    /// output up to `dict_size`, and about 128 KiB besides). For the streamed unit that is
    /// its input read ahead of its worker, its decoder, the output its worker decodes ahead
    /// of the caller (a quarter of the budget, but between 2 MiB and 16 MiB) and the piece
    /// of up to 2 MiB the caller reads. When the next chunk does not fit, the reader reads
    /// no further ahead until the caller has read earlier output.
    ///
    /// A work unit is not decoded whole on a worker when it does not fit the budget on its
    /// own, or when it grows past the size 7-Zip's encoder gives a unit for this dictionary
    /// (`4 * dict_size`, clamped to 1 MiB - 256 MiB, at least `dict_size`, rounded up to
    /// 1 MiB; 7-Zip's decoder stops decoding in parallel at the same point). It is
    /// streamed once the units before it are out, and the units after it are decoded in
    /// parallel again. Past that size the streamed unit's input is read at most 1 MiB
    /// ahead of its worker: there is no unit boundary to be expected for a while. The
    /// streamed unit needs a window of up to `dict_size` whatever the budget, a chunk of
    /// input and 4 MiB of output, so a budget below that does not lower the footprint
    /// further. Thread stacks and a few KiB of bookkeeping are not counted.
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
            TestHooks::default(),
        )
    }

    fn with_options(
        inner: R,
        dict_size: u32,
        preset_dict: Option<&[u8]>,
        num_workers: u32,
        budget: u64,
        unit_cap: Option<usize>,
        #[cfg(test)] hooks: TestHooks,
    ) -> Self {
        let max_workers = num_workers.clamp(1, 256);

        let work_queue = WorkStealingQueue::new();
        // Unbounded: what it can hold is bounded already, one result for each unit in flight
        // and the head's output by its worker's allowance. With room for one result, the
        // head's worker stopped whenever the caller was reading rather than taking output.
        let (result_tx, result_rx) = mpsc::channel::<UnitResult>();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let active_workers = Arc::new(AtomicU32::new(0));
        let preset_dict = preset_dict.map(|s| s.to_vec()).map(Arc::new);

        let mut reader = Self {
            inner,
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
            lookahead_from: unit_cap.unwrap_or_else(|| max_unit_size(dict_size)),
            window_max: (u64::from(dict_size) + 15) & !15,
            unit_charges: VecDeque::new(),
            in_flight_bytes: 0,
            head: None,
            units_streamed: 0,
            #[cfg(test)]
            streamed_units: Vec::new(),
            shutdown_flag,
            cancellation: None,
            state: State::Reading,
            work_queue,
            active_workers,
            max_workers,
            dict_size,
            preset_dict,
            worker_handles: Vec::new(),
            #[cfg(test)]
            hooks,
        };

        reader.spawn_worker_thread();

        reader
    }

    /// Sets a shared cancellation flag.
    ///
    /// Setting the flag to `true` requests cancellation. The reader checks it on every
    /// read, and every 50 ms while it waits for a worker: the read returns an I/O error
    /// carrying [`DecoderCancelled`], and so does every read after it. The workers stop
    /// at the end of the chunk they are decoding.
    ///
    /// Without a flag, a read that waits for a work unit returns only once the unit has
    /// been decoded, which for the largest units 7-Zip writes (256 MiB) takes seconds.
    pub fn set_cancellation(&mut self, flag: Arc<AtomicBool>) {
        self.cancellation = Some(flag);
    }

    fn cancellation_requested(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
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
            hooks: self.hooks.clone(),
        };

        let handle = thread::spawn(move || worker_thread_logic(worker_handle, context));

        self.worker_handles.push(handle);
    }

    /// The count of independent chunks found inside the compressed file.
    /// This is effectively tha maximum parallelization possible.
    pub fn chunk_count(&self) -> u64 {
        self.next_sequence_to_return
    }

    /// The count of those decoded as they were read, their output handed back chunk by
    /// chunk, rather than decoded whole.
    #[doc(hidden)]
    pub fn chunks_streamed(&self) -> u64 {
        self.units_streamed
    }

    /// Reads what the stream holds next: a chunk's header, or what ends the stream.
    fn read_chunk_header(&mut self) -> io::Result<Next> {
        let mut header = ChunkHeader {
            bytes: [0; CHUNK_HEADER_MAX],
            len: 1,
            unpacked: 0,
            packed: 0,
            starts_unit: false,
        };
        match self.inner.read_exact(&mut header.bytes[..1]) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(Next::Eof),
            Err(error) => return Err(error),
        }

        let control = header.bytes[0];
        if control < 0x80 && control != 0x01 && control != 0x02 {
            return Ok(Next::End(control));
        }

        header.starts_unit = control >= 0xE0 || control == 0x01;
        header.len = match control {
            0xC0.. => 6,
            0x80.. => 5,
            _ => 3,
        };
        self.inner.read_exact(&mut header.bytes[1..header.len])?;
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
        Ok(Next::Chunk(header))
    }

    /// Reads the next chunk's header, or ends the stream there.
    fn read_ahead(&mut self) {
        match self.read_chunk_header() {
            Ok(Next::Chunk(header)) => self.pending_chunk = Some(header),
            Ok(Next::End(byte)) => self.end_stream(Some(byte)),
            Ok(Next::Eof) => self.end_stream(None),
            Err(error) => self.end_input(error),
        }
    }

    /// Whether the head is still being read.
    fn head_incomplete(&self) -> bool {
        self.head.as_ref().is_some_and(|head| !head.complete())
    }

    /// Whether more of the stream may be read for the workers: fewer units are queued than
    /// there are workers, and fewer than `max_workers + 1` are in flight.
    fn may_read_ahead(&self) -> bool {
        // Keep the workers fed: read ahead while fewer units are queued than there are
        // workers. Waiting for a result as soon as the queue is non-empty races with the
        // worker that is about to steal that unit, and losing the race blocks the reader
        // until the unit is fully decoded while every other worker idles.
        //
        // But never let more than `max_workers + 1` units be in flight: a unit that
        // finishes ahead of its turn waits, whole, in `out_of_order_chunks`, and while one
        // slow unit holds up the order the queue keeps draining, so a queue-length check
        // alone let every later unit's output pile up behind it. One header dispatches at
        // most one unit, so checking before it is enough.
        self.work_queue.len() < self.max_workers as usize
            && self.units_in_flight() <= u64::from(self.max_workers)
    }

    /// Places the pending chunk: with the head while it is being read, in the work unit
    /// being gathered otherwise. A chunk that starts a unit first closes the one before it.
    /// A chunk that fails to read is not kept at all.
    ///
    /// `needed`: the head's worker has nothing left to decode, so the chunk is sent
    /// whatever the budget and the lookahead.
    fn place_pending_chunk(&mut self, needed: bool) -> io::Result<Placement> {
        let header = self.pending_chunk.expect("a chunk is pending");
        if header.starts_unit {
            self.close_unit();
        }
        if self.head_incomplete() {
            return self.place_head_chunk(header, needed);
        }

        let unpacked = checked_unpacked_size(self.current_unit_unpacked, header.unpacked)?;
        // A unit too large for a worker waits to be the head.
        if self.unit_cap.is_some_and(|cap| unpacked > cap) {
            return Ok(Placement::Wait);
        }

        // One byte to spare: the end marker that closes the unit never reallocates.
        let needed = self.current_work_unit.len() + header.len + header.packed + 1;
        if !self.reserve_unit_buffer(needed, unpacked) {
            return Ok(Placement::Wait);
        }

        self.pending_chunk = None;
        let chunk_start = self.current_work_unit.len();
        self.current_work_unit.extend_from_slice(header.bytes());
        let data_start = self.current_work_unit.len();
        self.current_work_unit.resize(data_start + header.packed, 0);
        if let Err(error) = self
            .inner
            .read_exact(&mut self.current_work_unit[data_start..])
        {
            self.current_work_unit.truncate(chunk_start);
            return Err(error);
        }

        self.current_unit_unpacked = unpacked;
        Ok(Placement::Placed)
    }

    /// Sends the pending chunk to the head's worker, if the budget has room for it and,
    /// once the head has outgrown `lookahead_from`, its input is not too far ahead of the
    /// worker already; or if the worker needs it.
    fn place_head_chunk(&mut self, header: ChunkHeader, needed: bool) -> io::Result<Placement> {
        let head = self.head.as_ref().expect("the head is being read");
        let unpacked = checked_unpacked_size(head.unpacked, header.unpacked)?;
        let size = header.len + header.packed;
        if !needed {
            let room = self.budget.saturating_sub(self.in_flight_bytes);
            if (head.unpacked > self.lookahead_from && head.ahead() >= STREAM_LOOKAHEAD)
                || self.head_cost(size, unpacked) > room
            {
                return Ok(Placement::Wait);
            }
        }

        let mut chunk = Vec::new();
        chunk.try_reserve_exact(size).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "LZMA2 chunk does not fit in memory",
            )
        })?;
        chunk.extend_from_slice(header.bytes());
        chunk.resize(size, 0);
        self.inner.read_exact(&mut chunk[header.len..])?;

        self.pending_chunk = None;
        self.send_to_head(chunk);
        self.head.as_mut().expect("the head is being read").unpacked = unpacked;
        Ok(Placement::Placed)
    }

    /// Sends `bytes` (whole chunks, or what ends the unit) to the head's worker. The first
    /// bytes dispatch the head.
    fn send_to_head(&mut self, bytes: Vec<u8>) {
        let head = self.head.as_mut().expect("the head is being read");
        head.sent += bytes.len() as u64;
        if let Some(chunks) = head.chunks.as_ref() {
            // A worker that has stopped receives nothing more; its result says why.
            let _ = chunks.send(bytes);
        }
        if let Some(work) = head.work.take() {
            self.dispatch(Work::Streamed(work));
        }
    }

    /// Nothing more goes to the head after `last`, its end marker or the byte that ends the
    /// stream; or, without one, after what it has, the input having ended there.
    fn complete_head(&mut self, last: Option<u8>) {
        if let Some(byte) = last {
            self.send_to_head(vec![byte]);
        }
        self.head.as_mut().expect("the head is being read").chunks = None;
    }

    /// The chunk about to be placed starts a unit: the one gathered before it is complete.
    fn close_unit(&mut self) {
        match self.head.as_ref() {
            // A head that has nothing yet is the unit this chunk starts.
            Some(head) if !head.complete() => {
                if head.sent > 0 {
                    self.complete_head(Some(0x00));
                }
            }
            _ => {
                if !self.current_work_unit.is_empty() {
                    self.current_work_unit.push(0x00);
                    self.send_work_unit();
                }
            }
        }
    }

    /// Makes room for `needed` compressed bytes in the work unit being gathered, if the
    /// budget lets the unit grow to them with `unpacked` decompressed bytes.
    fn reserve_unit_buffer(&mut self, needed: usize, unpacked: usize) -> bool {
        let room = self
            .budget
            .saturating_sub(self.in_flight_bytes)
            .saturating_sub(self.head_cost(0, 0));
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

    /// The window of a decoder that has produced `unpacked` bytes, at most.
    fn window(&self, unpacked: usize) -> u64 {
        let preset = self.preset_dict.as_ref().map_or(0, |preset| preset.len());
        (unpacked as u64)
            .saturating_add(preset as u64)
            .saturating_mul(2)
            .max(WINDOW_MIN)
            .min(self.window_max)
    }

    /// The most a work unit holds from dispatch until its output is read: its compressed
    /// input, its output, and the decoder that turns one into the other.
    fn unit_cost(&self, packed_capacity: usize, unpacked: usize) -> u64 {
        (packed_capacity as u64)
            .saturating_add(unpacked as u64)
            .saturating_add(self.window(unpacked))
            .saturating_add(DECODER_OVERHEAD)
    }

    /// What the head holds with `extra` more compressed bytes sent, its chunks then adding
    /// up to `unpacked` (or what they add up to now, if more): its input not yet freed, its
    /// decoder, the output its worker may decode ahead of the caller and the piece the
    /// caller reads.
    fn head_cost(&self, extra: usize, unpacked: usize) -> u64 {
        let Some(head) = self.head.as_ref() else {
            return 0;
        };
        let unpacked = head.unpacked.max(unpacked);
        let output = unpacked.min(self.head_output_ahead() + PIECE_MAX) as u64;
        head.ahead()
            .saturating_add(extra as u64)
            .saturating_add(self.window(unpacked))
            .saturating_add(DECODER_OVERHEAD)
            .saturating_add(output)
    }

    /// The most output the head's worker decodes ahead of the caller: `HEAD_OUTPUT_AHEAD`,
    /// or a quarter of the budget, but at least the largest chunk.
    fn head_output_ahead(&self) -> usize {
        usize::try_from(self.budget / 4)
            .unwrap_or(usize::MAX)
            .clamp(PIECE_MAX, HEAD_OUTPUT_AHEAD)
    }

    /// Units dispatched and not yet returned to the caller: the head, and those queued,
    /// being decoded, in the result channel, or waiting in `out_of_order_chunks`.
    fn units_in_flight(&self) -> u64 {
        self.next_sequence_to_dispatch - self.next_sequence_to_return
    }

    /// Queues `work` for the workers, with a new worker if none is idle to take it.
    fn dispatch(&mut self, work: Work) -> bool {
        if !self.work_queue.push(work) {
            // The queue is closed only once the reader has failed or is being dropped.
            self.input_error = Some(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker threads have shut down",
            ));
            self.stop_dispatching();
            return false;
        }

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
        true
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
        if !self.dispatch(Work::Whole(WorkUnit {
            seq,
            data,
            unpacked,
        })) {
            return;
        }
        self.unit_charges.push_back(charge);
        self.in_flight_bytes = self.in_flight_bytes.saturating_add(charge);
        self.next_sequence_to_dispatch += 1;
    }

    /// The unit being gathered is next in line for the output and no worker has it: it is
    /// streamed, starting with the chunks gathered so far.
    fn start_head(&mut self) {
        let (chunk_tx, chunk_rx) = mpsc::channel();
        let (credit_tx, credit_rx) = mpsc::channel();
        let _ = credit_tx.send(self.head_output_ahead());
        let freed = Arc::new(AtomicU64::new(0));
        let seq = self.next_sequence_to_dispatch;
        self.head = Some(Head {
            seq,
            chunks: Some(chunk_tx),
            work: Some(StreamedUnit {
                seq,
                chunks: chunk_rx,
                credits: credit_rx,
                freed: Arc::clone(&freed),
            }),
            credits: credit_tx,
            freed,
            sent: 0,
            unpacked: core::mem::take(&mut self.current_unit_unpacked),
            output: VecDeque::new(),
            end: None,
        });
        self.unit_charges.push_back(0);
        self.next_sequence_to_dispatch += 1;
        let gathered = core::mem::take(&mut self.current_work_unit);
        if !gathered.is_empty() {
            self.send_to_head(gathered);
        }
    }

    /// The head's last result has been taken.
    fn finish_head(&mut self) {
        let head = self.head.take().expect("there is a head");
        #[cfg(test)]
        self.streamed_units.push(head.seq);
        drop(head);
        self.units_streamed += 1;
        self.next_sequence_to_return += 1;
        if let Some(charge) = self.unit_charges.pop_front() {
            self.in_flight_bytes -= charge;
        }
    }

    /// No more work is dispatched; the reader waits for the units already sent.
    fn stop_dispatching(&mut self) {
        // Input without a chunk dispatches nothing to wait for.
        self.last_sequence_id = self.next_sequence_to_dispatch.checked_sub(1);
        self.state = State::Draining;
    }

    /// The stream ends here, with `last` (its end marker or a byte that is not a control
    /// byte) or, when the input ended without either, with nothing.
    fn end_stream(&mut self, last: Option<u8>) {
        match self.head.as_ref() {
            // Input that ends before a unit starts holds no unit, and decodes to nothing.
            Some(head) if !head.complete() && last.is_none() && head.sent == 0 => {
                self.head = None;
                self.unit_charges.pop_back();
                self.next_sequence_to_dispatch -= 1;
            }
            Some(head) if !head.complete() => self.complete_head(last),
            _ => {
                if let Some(byte) = last {
                    self.current_work_unit.push(byte);
                }
                // Without the end marker the worker's decoder reports what the
                // single-threaded reader does.
                self.send_work_unit();
            }
        }
        self.stop_dispatching();
    }

    /// The input failed: every chunk read in full before the failure still decodes, as it
    /// does single-threaded, and the error comes after them.
    fn end_input(&mut self, error: io::Error) {
        self.pending_chunk = None;
        if self.head_incomplete() {
            self.complete_head(Some(0x00));
        } else {
            if !self.current_work_unit.is_empty() {
                self.current_work_unit.push(0x00);
            }
            self.send_work_unit();
        }
        self.input_error = Some(error);
        self.stop_dispatching();
    }

    /// The unit `seq` failed: the stream ends there. Nothing after it is dispatched any
    /// more, and results of later units are dropped.
    fn unit_failed(&mut self, seq: u64) {
        if matches!(self.state, State::Reading) {
            self.current_work_unit.clear();
            self.current_unit_unpacked = 0;
            self.pending_chunk = None;
            self.state = State::Draining;
        }
        self.last_sequence_id = Some(self.last_sequence_id.map_or(seq, |last| last.min(seq)));
        self.out_of_order_chunks.retain(|&later, _| later <= seq);
    }

    /// Files a worker's result.
    fn accept_result(&mut self, result: UnitResult) {
        if let Some(head) = self.head.as_mut().filter(|head| head.seq == result.seq) {
            if result.more {
                head.output.push_back(result.data);
            } else {
                let failed = result.error.is_some();
                head.end = Some(result.error);
                if failed {
                    self.unit_failed(result.seq);
                }
            }
            return;
        }

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
            self.unit_failed(result.seq);
        }
        self.out_of_order_chunks
            .insert(result.seq, (result.data, result.error));
    }

    /// Waits for a worker's result and files it. Every dispatched unit yields one, a worker
    /// panic included. With a cancellation flag the wait wakes up to check it.
    fn wait_for_result(&mut self) -> Result<(), DecoderCancelled> {
        if self.cancellation.is_none() {
            if let Ok(result) = self.result_rx.recv() {
                self.accept_result(result);
            }
            return Ok(());
        }
        loop {
            if self.cancellation_requested() {
                return Err(DecoderCancelled);
            }
            match self.result_rx.recv_timeout(CANCELLATION_CHECK_INTERVAL) {
                Ok(result) => {
                    self.accept_result(result);
                    return Ok(());
                }
                Err(RecvTimeoutError::Timeout) => {}
                // The reader holds a sender itself, so the channel never disconnects.
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }

    /// Reads ahead without waiting for a worker: files the results that are in, sends the
    /// head its chunks and gathers the units after it for the workers, until the head has
    /// output for the caller. The caller takes no output while the reader reads, and on a
    /// slow medium a read waits for it: a reader that read on, even for a millisecond more,
    /// handed out a piece per read, and the head's worker waited for the caller.
    fn pump(&mut self) {
        loop {
            if let Ok(result) = self.result_rx.try_recv() {
                self.accept_result(result);
                continue;
            }
            if !matches!(self.state, State::Reading) || self.cancellation_requested() {
                return;
            }
            #[cfg(test)]
            let read_on = self.hooks.read_on;
            #[cfg(not(test))]
            let read_on = false;
            let head_ready = self
                .head
                .as_ref()
                .is_some_and(|head| !head.output.is_empty() || head.end.is_some());
            if head_ready && !read_on {
                return;
            }
            if self.pending_chunk.is_some() {
                match self.place_pending_chunk(false) {
                    Ok(Placement::Placed) => {}
                    Ok(Placement::Wait) => return,
                    Err(error) => {
                        self.end_input(error);
                        return;
                    }
                }
            } else if self.head_incomplete() || self.may_read_ahead() {
                self.read_ahead();
            } else {
                return;
            }
        }
    }

    /// Sends the head's worker its next chunk when it has nothing left: a reader about to
    /// wait for the head must not leave its worker waiting for input.
    fn feed_head(&mut self) {
        while matches!(self.state, State::Reading)
            && self
                .head
                .as_ref()
                .is_some_and(|head| !head.complete() && head.ahead() == 0)
        {
            if self.pending_chunk.is_none() {
                self.read_ahead();
                continue;
            }
            // Sent, or the chunk starts the next unit and completed the head.
            if let Err(error) = self.place_pending_chunk(true) {
                self.end_input(error);
            }
        }
    }

    /// The next output in order and the error that ended its unit early, if one did: a unit
    /// decoded whole, or a chunk of the head. `None` once every unit there is has been
    /// returned.
    fn next_unit(&mut self) -> Result<Option<UnitOutput>, DecoderCancelled> {
        loop {
            // Reading ahead can take many rounds of this loop before anything is returned.
            if self.cancellation_requested() {
                return Err(DecoderCancelled);
            }

            if let Some(head) = self.head.as_mut() {
                if let Some(data) = head.output.pop_front() {
                    // Its worker may decode that much more while the caller reads this one.
                    let _ = head.credits.send(data.len());
                    return Ok(Some((data, None)));
                }
                if let Some(end) = head.end.take() {
                    self.finish_head();
                    match end {
                        Some(error) => return Ok(Some((Vec::new(), Some(error)))),
                        None => continue,
                    }
                }
                // Nothing to hand out: read on while the head's worker decodes, and leave it
                // something to decode.
                self.pump();
                self.feed_head();
                // Unless the input ended before the head held anything, or it has output now.
                if self
                    .head
                    .as_ref()
                    .is_some_and(|head| head.output.is_empty() && head.end.is_none())
                {
                    self.wait_for_result()?;
                }
                continue;
            }

            // Always check for already-received chunks first.
            if let Some(result) = self
                .out_of_order_chunks
                .remove(&self.next_sequence_to_return)
            {
                self.next_sequence_to_return += 1;
                if let Some(charge) = self.unit_charges.pop_front() {
                    self.in_flight_bytes -= charge;
                }
                return Ok(Some(result));
            }

            match self.state {
                State::Reading => {
                    // Nothing comes before the unit being gathered: rather than wait for it
                    // to be gathered whole and decoded, stream it.
                    if self.next_sequence_to_return == self.next_sequence_to_dispatch {
                        self.start_head();
                        continue;
                    }

                    // First, always try to receive a result without blocking.
                    // This keeps the pipeline moving and avoids unnecessary blocking on I/O.
                    // The reader holds a sender itself, so the channel never disconnects.
                    if let Ok(result) = self.result_rx.try_recv() {
                        self.accept_result(result);
                        continue;
                    }

                    if self.pending_chunk.is_some() {
                        match self.place_pending_chunk(false) {
                            Ok(Placement::Placed) => continue,
                            // Units are in flight: the unit being gathered would have been
                            // the head otherwise. Every one of them yields a result, and a
                            // result frees the unit's input and decoder at once, its output
                            // once the caller has read it.
                            Ok(Placement::Wait) => {}
                            Err(error) => {
                                self.end_input(error);
                                continue;
                            }
                        }
                    } else if self.may_read_ahead() {
                        self.read_ahead();
                        continue;
                    }

                    // Now we MUST wait for a result to make progress.
                    self.wait_for_result()?;
                }
                State::Draining => {
                    if self
                        .last_sequence_id
                        .is_none_or(|last| self.next_sequence_to_return > last)
                    {
                        return Ok(None);
                    }

                    // In Draining state, we only wait for results.
                    self.wait_for_result()?;
                }
                State::Finished | State::Failed(..) | State::Cancelled => return Ok(None),
            }
        }
    }

    /// Ends the stream with `error`. Nothing after the failure is needed any more.
    fn fail(&mut self, error: io::Error) -> io::Error {
        self.state = State::Failed(error.kind(), error.to_string());
        self.stop_workers();
        error
    }

    /// Ends the stream on the caller's request.
    fn cancel(&mut self) -> io::Error {
        self.state = State::Cancelled;
        self.stop_workers();
        io::Error::other(DecoderCancelled)
    }

    /// Nothing the workers decode is needed any more: queued units are dropped, each worker
    /// stops at the end of the chunk it is decoding instead of the end of its unit, which
    /// can be hundreds of MiB further on, and the head's worker, which may be waiting for
    /// input or for the caller, is let go.
    fn stop_workers(&mut self) {
        self.shutdown_flag.store(true, Ordering::Release);
        self.head = None;
        self.work_queue.discard_pending();
        self.work_queue.close();
    }
}

/// A unit's decompressed size of `unpacked` with one more chunk of `size` bytes.
fn checked_unpacked_size(unpacked: usize, size: usize) -> io::Result<usize> {
    unpacked.checked_add(size).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "LZMA2 work unit unpacked size exceeds the platform limit",
        )
    })
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
    result_tx: Sender<UnitResult>,
    dict_size: u32,
    preset_dict: Option<Arc<Vec<u8>>>,
    shutdown_flag: Arc<AtomicBool>,
    active_workers: Arc<AtomicU32>,
    #[cfg(test)]
    hooks: TestHooks,
}

/// Ways for tests to steer the workers.
#[cfg(test)]
#[derive(Clone, Default)]
struct TestHooks {
    /// A worker panics when it takes this unit, to exercise the panic path.
    panic_on_unit: Option<u64>,
    /// A worker that takes this unit holds it until the reader shuts down, as a long
    /// decode would.
    stall_on_unit: Option<u64>,
    /// Counts the worker threads that have returned.
    workers_exited: Arc<AtomicU32>,
    /// The reader reads on while the head has output for the caller, until it can read no
    /// further, so that how far it reads ahead does not depend on how fast the test runs.
    read_on: bool,
}

/// The logic for a single worker thread.
fn worker_thread_logic(worker_handle: WorkerHandle<Work>, context: WorkerContext) {
    while !context.shutdown_flag.load(Ordering::Acquire) {
        let Some(work) = worker_handle.steal() else {
            // No more work available and queue is closed
            break;
        };
        context.active_workers.fetch_add(1, Ordering::Release);

        // A panic must still produce a result: the reader waits for every unit it sent.
        let panicked = || Some(io::Error::other("LZMA2 worker thread panicked"));
        let sent = match work {
            Work::Whole(unit) => {
                let seq = unit.seq;
                let (data, error) =
                    panic::catch_unwind(AssertUnwindSafe(|| decode_unit(unit, &context)))
                        .unwrap_or_else(|_| (Vec::new(), panicked()));
                context
                    .result_tx
                    .send(UnitResult {
                        seq,
                        data,
                        error,
                        more: false,
                    })
                    .is_ok()
            }
            Work::Streamed(unit) => {
                let seq = unit.seq;
                panic::catch_unwind(AssertUnwindSafe(|| stream_unit(unit, &context)))
                    .unwrap_or_else(|_| {
                        context
                            .result_tx
                            .send(UnitResult {
                                seq,
                                data: Vec::new(),
                                error: panicked(),
                                more: false,
                            })
                            .is_ok()
                    })
            }
        };

        context.active_workers.fetch_sub(1, Ordering::Release);
        if !sent {
            break;
        }
    }
    #[cfg(test)]
    context.hooks.workers_exited.fetch_add(1, Ordering::Release);
}

/// The test hooks a worker applies to the unit it has taken.
#[cfg(test)]
fn apply_hooks(seq: u64, context: &WorkerContext) {
    if context.hooks.panic_on_unit == Some(seq) {
        panic!("test panic in work unit {seq}");
    }
    if context.hooks.stall_on_unit == Some(seq) {
        while !context.shutdown_flag.load(Ordering::Acquire) {
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

/// Decodes one work unit. Every read returns at the end of a chunk, so a failure keeps
/// exactly the chunks before the one that failed.
fn decode_unit(unit: WorkUnit, context: &WorkerContext) -> UnitOutput {
    #[cfg(test)]
    apply_hooks(unit.seq, context);

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

    let mut filled = 0;
    while filled < unit.unpacked {
        // Once the reader has failed, been cancelled or dropped, nothing this unit produces
        // is needed. Its output is freed before the result goes out.
        if context.shutdown_flag.load(Ordering::Acquire) {
            return (Vec::new(), Some(io::Error::other("LZMA2 reader shut down")));
        }
        // Initialise only the room the next chunk can fill. Zeroing the whole unit up
        // front (256 MiB for the largest units 7-Zip writes: 70 ms on one thread, several
        // times that on many at once) came before the first check of the flag above, and
        // the reader's drop waits for it. `filled` is at a chunk boundary and no chunk is
        // larger than this, so every read below still ends at the end of a chunk.
        let end = filled + (unit.unpacked - filled).min(CHUNK_UNPACKED_MAX);
        decompressed_data.resize(end, 0);
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

/// The input of a streamed unit's decoder: what has come in, whole chunks in order, each
/// part freed and counted in `freed` once the decoder has read it.
struct ChunkFeed {
    parts: VecDeque<Cursor<Vec<u8>>>,
    freed: Arc<AtomicU64>,
}

impl ChunkFeed {
    fn is_drained(&self) -> bool {
        self.parts.is_empty()
    }

    /// The decompressed size of the chunk the next read starts: 0 for what ends a unit, or
    /// when nothing is left.
    fn next_chunk_unpacked(&self) -> usize {
        let Some(part) = self.parts.front() else {
            return 0;
        };
        let header = &part.get_ref()[part.position() as usize..];
        let size = |at: usize| {
            header
                .get(at..at + 2)
                .map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])) + 1)
        };
        match header.first() {
            Some(&control) if control >= 0x80 => (usize::from(control & 0x1F) << 16) + size(1),
            Some(0x01 | 0x02) => size(1),
            _ => 0,
        }
    }
}

impl Read for ChunkFeed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while let Some(part) = self.parts.front_mut() {
            let bytes_read = part.read(buf)?;
            let len = part.get_ref().len();
            if part.position() as usize == len {
                self.parts.pop_front();
                self.freed.fetch_add(len as u64, Ordering::Release);
            }
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
        }
        Ok(0)
    }
}

/// Decodes a streamed unit as its chunks come in, and hands back the output of the chunks
/// that decoded cleanly in pieces of whole chunks, then the error that stopped the unit, if
/// one did. Returns whether the reader was there to take the results.
///
/// A piece takes the chunks after its first while they have come in already and fit in
/// `PIECE_MAX`: poorly compressible data fills the 64 KiB a chunk may pack with about
/// 50 KiB of output, and a round trip to the caller for each of those chunks held the head
/// up. A piece never waits for input that has not come in.
fn stream_unit(unit: StreamedUnit, context: &WorkerContext) -> bool {
    #[cfg(test)]
    apply_hooks(unit.seq, context);

    let StreamedUnit {
        seq,
        chunks,
        credits,
        freed,
    } = unit;
    let feed = ChunkFeed {
        parts: VecDeque::new(),
        freed,
    };
    let mut reader = Lzma2Reader::new(
        feed,
        context.dict_size,
        context.preset_dict.as_deref().map(|v| v.as_slice()),
    );
    let send = |piece: Vec<u8>| {
        let piece = UnitResult {
            seq,
            data: piece,
            error: None,
            more: true,
        };
        context.result_tx.send(piece).is_ok()
    };
    let mut piece = Vec::new();
    // The bytes of output it may still decode before the reader has taken more.
    let mut allowance = 0;
    let error = loop {
        // Once the reader has failed, been cancelled or dropped, nothing more is needed.
        if context.shutdown_flag.load(Ordering::Acquire) {
            break Some(io::Error::other("LZMA2 reader shut down"));
        }
        // Every read below decodes one whole chunk, so a chunk starts here. The reader
        // sends whole chunks; without one to come, the decoder reports the end of input.
        if reader.inner().is_drained() {
            let bytes = if piece.is_empty() {
                chunks.recv().ok()
            } else {
                chunks.try_recv().ok()
            };
            match bytes {
                Some(bytes) => reader.inner_mut().parts.push_back(Cursor::new(bytes)),
                None if !piece.is_empty() => {
                    if !send(core::mem::take(&mut piece)) {
                        return false;
                    }
                    continue;
                }
                None => {}
            }
        }
        let size = reader.inner().next_chunk_unpacked();
        allowance += credits.try_iter().sum::<usize>();
        // A piece goes out before the worker waits for the reader, who may be waiting for
        // that piece.
        if !piece.is_empty() && (size == 0 || piece.len() + size > PIECE_MAX || allowance < size) {
            if !send(core::mem::take(&mut piece)) {
                return false;
            }
            continue;
        }
        if size == 0 {
            // The end marker, a byte that is not a control byte, or no input at all.
            break match reader.read_to_chunk_end(&mut [0u8; 1]) {
                Ok(0) => None,
                Ok(_) => Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "LZMA2 chunk header declares no data",
                )),
                Err(error) => Some(error),
            };
        }

        // Wait until the reader has taken enough of the output before. The allowance is at
        // least the largest chunk, so once it has taken everything, any chunk fits.
        while allowance < size {
            match credits.recv() {
                Ok(bytes) => allowance += bytes,
                Err(_) => return false,
            }
        }
        allowance -= size;
        let start = piece.len();
        if piece.capacity() < start + size {
            // Grow geometrically within `PIECE_MAX`: a piece of many chunks is not copied
            // over and over.
            let target = (start + size).max(2 * piece.capacity()).min(PIECE_MAX);
            if piece.try_reserve_exact(target - start).is_err() {
                if piece.is_empty() {
                    break Some(io::Error::new(
                        io::ErrorKind::OutOfMemory,
                        "LZMA2 chunk output does not fit in memory",
                    ));
                }
                // What has been decoded goes out; the chunk then starts a piece of its own.
                if !send(core::mem::take(&mut piece)) {
                    return false;
                }
                continue;
            }
        }
        piece.resize(start + size, 0);
        let error = match reader.read_to_chunk_end(&mut piece[start..]) {
            Ok(n) if n == size => continue,
            Ok(_) => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "LZMA2 chunk ended before its declared size",
            ),
            Err(error) => error,
        };
        // The chunks before the one that failed still go out, in their piece.
        piece.truncate(start);
        if !piece.is_empty() && !send(piece) {
            return false;
        }
        break Some(error);
    };
    context
        .result_tx
        .send(UnitResult {
            seq,
            data: Vec::new(),
            error,
            more: false,
        })
        .is_ok()
}

impl<R: Read> Read for Lzma2ReaderMt<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        loop {
            match &self.state {
                State::Failed(kind, message) => return Err(io::Error::new(*kind, message.clone())),
                State::Cancelled => return Err(io::Error::other(DecoderCancelled)),
                _ => {}
            }
            if self.cancellation_requested() {
                return Err(self.cancel());
            }

            let bytes_read = self.current_chunk.read(buf)?;
            if bytes_read > 0 {
                return Ok(bytes_read);
            }
            // The caller has read this output: free it before the reader gathers more.
            self.current_chunk = Cursor::new(Vec::new());

            if let Some(error) = self.pending_error.take() {
                return Err(self.fail(error));
            }

            match self.next_unit() {
                Err(DecoderCancelled) => return Err(self.cancel()),
                Ok(Some((data, error))) => {
                    self.current_chunk = Cursor::new(data);
                    self.pending_error = error;
                }
                Ok(None) => {
                    if let Some(error) = self.input_error.take() {
                        return Err(self.fail(error));
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
        self.stop_workers();
        // Disconnect before joining, so that what the workers still send is dropped at once.
        let (_, disconnected) = mpsc::channel();
        drop(core::mem::replace(&mut self.result_rx, disconnected));
        // Each worker stops at the end of its chunk, so once the reader is gone, so are
        // its threads and the memory they held.
        for handle in self.worker_handles.drain(..) {
            let _ = handle.join();
        }
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

    /// An LZMA2 stream of stored chunks of up to 64 KiB, one work unit of each size.
    fn stored_units(sizes: &[usize]) -> (Vec<u8>, Vec<u8>) {
        let plain: Vec<u8> = (0..sizes.iter().sum::<usize>())
            .map(|i| (i % 251) as u8)
            .collect();
        let mut stream = Vec::new();
        let mut at = 0;
        for &size in sizes {
            for (index, chunk) in plain[at..at + size].chunks(64 * 1024).enumerate() {
                let control = if index == 0 { 0x01 } else { 0x02 };
                stream.push(control);
                stream.extend_from_slice(&u16::try_from(chunk.len() - 1).unwrap().to_be_bytes());
                stream.extend_from_slice(chunk);
            }
            at += size;
        }
        stream.push(0x00);
        (stream, plain)
    }

    /// The largest stored chunk with its header.
    const STORED_CHUNK: u64 = 64 * 1024 + 3;

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
                let head = reader.head_cost(0, 0);
                assert!(
                    reader.in_flight_bytes + gathering + reading + head <= budget,
                    "{workers} workers: {} in flight, {gathering} gathered, {reading} read, \
                     {head} for the head",
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
            assert_eq!(reader.chunk_count(), 3000);
            assert!(
                reader.chunks_streamed() < 3000,
                "{workers} workers: nothing on the workers"
            );
        }
    }

    /// A unit that does not fit the budget on its own is streamed, after the units before
    /// it, and the units after it go to the workers again.
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
            assert_eq!(reader.chunk_count(), 6001, "{workers} workers");
            assert!(
                reader.streamed_units.contains(&3000),
                "{workers} workers: {:?}",
                reader.streamed_units
            );
            let after = reader
                .streamed_units
                .iter()
                .filter(|&&seq| seq > 3000)
                .count();
            assert!(after < 3000, "{workers} workers: none after it on a worker");
        }
    }

    /// A unit past 7-Zip's limit for the dictionary is streamed whatever the budget; `new`
    /// gives it to a worker whole.
    #[test]
    fn a_unit_past_the_cap_is_streamed() {
        let (stream, plain) = stored_units(&[1, 2 * 1024 * 1024 + 1, 1, 1, 1]);
        assert_eq!(max_unit_size(4096), 1024 * 1024);

        let mut reader =
            Lzma2ReaderMt::with_memory_budget(stream.as_slice(), 4096, None, 2, u64::MAX);
        assert!(read_all(&mut reader, plain.len()) == plain);
        assert_eq!(reader.chunk_count(), 5);
        assert!(
            reader.streamed_units.starts_with(&[0, 1]),
            "{:?}",
            reader.streamed_units
        );

        let mut reader = Lzma2ReaderMt::with_options(
            stream.as_slice(),
            4096,
            None,
            2,
            u64::MAX,
            None,
            TestHooks {
                read_on: true,
                ..TestHooks::default()
            },
        );
        assert!(read_all(&mut reader, plain.len()) == plain);
        assert_eq!(reader.chunk_count(), 5);
        assert!(
            !reader.streamed_units.contains(&1),
            "{:?}",
            reader.streamed_units
        );
    }

    /// A stream that is one unit is streamed: no worker gets it whole, the caller holds one
    /// piece of output at a time, a piece takes the chunks that are in when it starts (with
    /// room in the budget, many), and once the unit has grown past 7-Zip's limit for the
    /// dictionary its input is kept at most `STREAM_LOOKAHEAD` ahead of the worker (with no
    /// room in the budget, a chunk at a time).
    #[test]
    fn one_unit_is_streamed() {
        const MIB: usize = 1024 * 1024;
        let (stream, plain) = stored_units(&[4 * MIB]);
        for budget in [None, Some(u64::MAX), Some(0)] {
            let mut reader = match budget {
                Some(budget) => {
                    Lzma2ReaderMt::with_memory_budget(stream.as_slice(), 4096, None, 4, budget)
                }
                None => Lzma2ReaderMt::new(stream.as_slice(), 4096, None, 4),
            };
            let mut out = Vec::with_capacity(plain.len());
            let mut buf = vec![0u8; 4096];
            let mut most_ahead = 0;
            let mut largest_piece = 0;
            loop {
                let n = reader.read(&mut buf).unwrap();
                let piece = reader.current_chunk.get_ref();
                assert!(
                    piece.capacity() <= PIECE_MAX,
                    "{budget:?}: more than a piece of output"
                );
                largest_piece = largest_piece.max(piece.len());
                if let Some(head) = reader.head.as_ref() {
                    // The cap for a 4 KiB dictionary is 1 MiB.
                    if head.unpacked > MIB {
                        most_ahead = most_ahead.max(head.ahead());
                    }
                }
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            assert!(out == plain, "{budget:?}: output differs");
            if budget != Some(0) {
                assert!(
                    largest_piece > STORED_CHUNK as usize,
                    "{budget:?}: one chunk a piece"
                );
            }
            assert_eq!(reader.streamed_units, [0], "{budget:?}");
            assert_eq!(reader.chunk_count(), 1, "{budget:?}");
            let bound = match budget {
                Some(0) => 2 * STORED_CHUNK,
                _ => STREAM_LOOKAHEAD + STORED_CHUNK,
            };
            assert!(most_ahead <= bound, "{budget:?}: {most_ahead} ahead");
        }
    }

    /// While the head is streamed the reader reads on to the units after it: reading on, the
    /// rest of the stream is with the workers by the time the first read returns. Without
    /// that, the read returns once the head has output.
    #[test]
    fn the_head_does_not_hold_up_the_units_after_it() {
        const MIB: usize = 1024 * 1024;
        let (stream, plain) = stored_units(&[MIB, MIB, MIB, MIB]);
        for read_on in [true, false] {
            let mut reader = Lzma2ReaderMt::with_options(
                stream.as_slice(),
                4096,
                None,
                4,
                u64::MAX,
                None,
                TestHooks {
                    read_on,
                    ..TestHooks::default()
                },
            );
            let mut buf = vec![0u8; 64 * 1024];
            let n = reader.read(&mut buf).unwrap();
            assert_eq!(buf[..n], plain[..n]);
            if read_on {
                assert_eq!(reader.next_sequence_to_dispatch, 4);
            }
            let mut out = buf[..n].to_vec();
            reader.read_to_end(&mut out).unwrap();
            assert!(out == plain, "read on: {read_on}");
            assert_eq!(reader.chunk_count(), 4);
            if read_on {
                assert_eq!(reader.streamed_units, [0]);
            }
        }
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
                TestHooks {
                    panic_on_unit: Some(2),
                    ..TestHooks::default()
                },
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

    /// A panic in the head's worker fails the head the same way.
    #[test]
    fn a_panic_in_the_head_fails_it() {
        let (stream, _) = stored_stream(64 * 1024, 3);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = Lzma2ReaderMt::with_options(
                stream.as_slice(),
                4096,
                None,
                2,
                u64::MAX,
                None,
                TestHooks {
                    panic_on_unit: Some(0),
                    ..TestHooks::default()
                },
            );
            let mut out = Vec::new();
            let error = reader.read_to_end(&mut out).unwrap_err();
            let _ = sender.send((out, error.to_string()));
        });

        let (out, message) = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the reader did not finish");
        assert!(out.is_empty());
        assert_eq!(message, "LZMA2 worker thread panicked");
    }

    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

    fn is_cancelled(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(|inner| inner.is::<DecoderCancelled>())
    }

    /// A read waiting for a unit that a worker has not finished returns once the flag is
    /// set, not once the unit is decoded, and so does every read after it. The reader's
    /// workers are gone once it is dropped.
    #[test]
    fn cancellation_ends_a_read_that_waits_for_a_worker() {
        let (stream, plain) = stored_stream(64 * 1024, 3);
        let hooks = TestHooks {
            stall_on_unit: Some(1),
            ..TestHooks::default()
        };
        let workers_exited = Arc::clone(&hooks.workers_exited);
        let flag = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&flag);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = Lzma2ReaderMt::with_options(
                stream.as_slice(),
                4096,
                None,
                2,
                u64::MAX,
                None,
                hooks,
            );
            reader.set_cancellation(cancellation);
            let mut out = Vec::new();
            let first = reader.read_to_end(&mut out).unwrap_err();
            let returned = std::time::Instant::now();
            let second = reader.read(&mut [0u8; 16]).unwrap_err();
            let spawned = reader.worker_handles.len();
            drop(reader);
            let _ = sender.send((out, first, second, returned, spawned));
        });

        thread::sleep(Duration::from_millis(200));
        let requested = std::time::Instant::now();
        flag.store(true, Ordering::Relaxed);
        let (out, first, second, returned, spawned) = receiver
            .recv_timeout(DEADLINE)
            .expect("the read ignored cancellation");
        assert!(
            out == plain[..64 * 1024],
            "unit 0 only: unit 1 never finished"
        );
        assert!(is_cancelled(&first), "{first}");
        assert!(is_cancelled(&second), "{second}");
        assert!(
            returned.duration_since(requested) < Duration::from_secs(1),
            "{:?}",
            returned.duration_since(requested)
        );
        assert_eq!(workers_exited.load(Ordering::Acquire) as usize, spawned);
    }

    /// A flag set before the first read cancels the stream before anything is read.
    #[test]
    fn cancellation_before_the_first_read() {
        let (stream, _) = stored_stream(64 * 1024, 3);
        let mut reader = Lzma2ReaderMt::new(stream.as_slice(), 4096, None, 2);
        reader.set_cancellation(Arc::new(AtomicBool::new(true)));
        let error = reader.read(&mut [0u8; 16]).unwrap_err();
        assert!(is_cancelled(&error), "{error}");
        assert_eq!(reader.next_sequence_to_dispatch, 0);
    }

    /// A read waiting for the head's worker returns once the flag is set, too.
    #[test]
    fn cancellation_ends_a_read_that_waits_for_the_head() {
        let (stream, _) = stored_stream(64 * 1024, 3);
        let hooks = TestHooks {
            stall_on_unit: Some(0),
            ..TestHooks::default()
        };
        let workers_exited = Arc::clone(&hooks.workers_exited);
        let flag = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&flag);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = Lzma2ReaderMt::with_options(
                stream.as_slice(),
                4096,
                None,
                2,
                u64::MAX,
                None,
                hooks,
            );
            reader.set_cancellation(cancellation);
            let error = reader.read(&mut [0u8; 16]).unwrap_err();
            let returned = std::time::Instant::now();
            let spawned = reader.worker_handles.len();
            drop(reader);
            let _ = sender.send((error, returned, spawned));
        });

        thread::sleep(Duration::from_millis(200));
        let requested = std::time::Instant::now();
        flag.store(true, Ordering::Relaxed);
        let (error, returned, spawned) = receiver
            .recv_timeout(DEADLINE)
            .expect("the read ignored cancellation");
        assert!(is_cancelled(&error), "{error}");
        assert!(
            returned.duration_since(requested) < Duration::from_secs(1),
            "{:?}",
            returned.duration_since(requested)
        );
        assert_eq!(workers_exited.load(Ordering::Acquire) as usize, spawned);
    }

    /// While a unit is streamed, the flag still ends the stream.
    #[test]
    fn cancellation_ends_a_streamed_unit() {
        let (stream, plain) = stored_stream(1024 * 1024, 0);
        let flag = Arc::new(AtomicBool::new(false));
        let mut reader =
            Lzma2ReaderMt::with_memory_budget(stream.as_slice(), 4096, None, 2, 512 * 1024);
        reader.set_cancellation(Arc::clone(&flag));
        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).unwrap();
        assert!(reader.head.is_some(), "not streamed");
        assert_eq!(buf[..n], plain[..n]);
        flag.store(true, Ordering::Relaxed);
        let error = reader.read(&mut buf).unwrap_err();
        assert!(is_cancelled(&error), "{error}");
    }

    /// Dropping the reader while a worker decodes a unit does not leave the worker
    /// running: the drop waits for every worker thread to exit.
    #[test]
    fn dropping_the_reader_stops_and_joins_its_workers() {
        let (stream, plain) = stored_stream(64 * 1024, 3);
        let hooks = TestHooks {
            stall_on_unit: Some(1),
            ..TestHooks::default()
        };
        let workers_exited = Arc::clone(&hooks.workers_exited);
        let mut reader =
            Lzma2ReaderMt::with_options(Cursor::new(stream), 4096, None, 2, u64::MAX, None, hooks);
        // Unit 0 streams on a worker while the calling thread reads ahead and dispatches
        // unit 1.
        let mut out = vec![0u8; 64 * 1024];
        for half in out.chunks_mut(32 * 1024) {
            reader.read_exact(half).unwrap();
        }
        assert!(out == plain[..64 * 1024]);
        assert!(reader.next_sequence_to_dispatch > 1);
        let start = std::time::Instant::now();
        while reader.active_workers.load(Ordering::Acquire) == 0 {
            assert!(start.elapsed() < DEADLINE, "no worker took unit 1");
            thread::yield_now();
        }

        let spawned = reader.worker_handles.len();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            drop(reader);
            let _ = sender.send(());
        });
        receiver.recv_timeout(DEADLINE).expect("the drop hung");
        assert_eq!(workers_exited.load(Ordering::Acquire) as usize, spawned);
    }

    /// A worker stops at the next chunk boundary once the reader has shut down, and frees
    /// what it decoded so far.
    #[test]
    fn a_worker_stops_between_chunks_once_the_reader_shuts_down() {
        let (stream, _) = stored_stream(256 * 1024, 0);
        let (result_tx, _result_rx) = mpsc::channel();
        let context = WorkerContext {
            result_tx,
            dict_size: 4096,
            preset_dict: None,
            shutdown_flag: Arc::new(AtomicBool::new(true)),
            active_workers: Arc::new(AtomicU32::new(0)),
            hooks: TestHooks::default(),
        };
        let unit = WorkUnit {
            seq: 0,
            data: stream,
            unpacked: 256 * 1024,
        };
        let (data, error) = decode_unit(unit, &context);
        assert!(data.is_empty());
        assert_eq!(error.unwrap().to_string(), "LZMA2 reader shut down");
    }

    #[test]
    fn rejects_compressed_work_unit_unpacked_size_overflow() {
        // A dependent compressed chunk that declares 2 MiB unpacked and one
        // byte packed. Preloading the accumulator reaches the same boundary as
        // 2,048 such headers do on a 32-bit target, without a target-specific
        // multi-gigabyte allocation.
        let input = [0x9F, 0xFF, 0xFF, 0x00, 0x00, 0x00];
        for on_caller in [false, true] {
            let mut reader = Lzma2ReaderMt::new(input.as_slice(), 4096, None, 1);
            let near_the_limit = usize::MAX - 2 * 1024 * 1024 + 1;
            if on_caller {
                reader.start_head();
                reader.head.as_mut().unwrap().unpacked = near_the_limit;
            } else {
                reader.current_unit_unpacked = near_the_limit;
            }

            let Next::Chunk(header) = reader.read_chunk_header().unwrap() else {
                panic!("not a chunk");
            };
            reader.pending_chunk = Some(header);
            let error = reader.place_pending_chunk(true).unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{on_caller}");
            assert_eq!(
                error.to_string(),
                "LZMA2 work unit unpacked size exceeds the platform limit"
            );
        }
    }
}
