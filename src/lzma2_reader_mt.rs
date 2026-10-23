use std::{
    collections::BTreeMap,
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

/// A work unit for a worker thread: its sequence number, the raw compressed bytes of a
/// series of chunks that starts with a dictionary reset, and the decompressed size of each
/// of those chunks (from the chunk headers).
struct WorkUnit {
    seq: u64,
    data: Vec<u8>,
    chunk_sizes: Vec<usize>,
}

/// What a worker hands back for a work unit: the decompressed bytes of every chunk that
/// decoded cleanly, in order, and the error that stopped it, if one did.
struct UnitResult {
    seq: u64,
    data: Vec<u8>,
    error: Option<io::Error>,
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
}

/// A multi-threaded LZMA2 decompressor.
pub struct Lzma2ReaderMt<R: Read> {
    inner: R,
    result_rx: Receiver<UnitResult>,
    result_tx: SyncSender<UnitResult>,
    current_work_unit: Vec<u8>,
    /// Decompressed size of the chunks gathered in `current_work_unit`.
    current_unit_unpacked: usize,
    /// Decompressed size of each chunk gathered in `current_work_unit`.
    current_chunk_sizes: Vec<usize>,
    next_sequence_to_dispatch: u64,
    next_sequence_to_return: u64,
    last_sequence_id: Option<u64>,
    out_of_order_chunks: BTreeMap<u64, (Vec<u8>, Option<io::Error>)>,
    current_chunk: Cursor<Vec<u8>>,
    /// The error that ended the unit `current_chunk` holds, reported once it is read out.
    pending_error: Option<io::Error>,
    /// The error the inner reader failed with, reported after every unit read before it.
    input_error: Option<io::Error>,
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
    /// and the size of the largest work unit, not with the size of the stream.
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
            #[cfg(test)]
            None,
        )
    }

    fn with_options(
        inner: R,
        dict_size: u32,
        preset_dict: Option<&[u8]>,
        num_workers: u32,
        #[cfg(test)] panic_on_unit: Option<u64>,
    ) -> Self {
        let max_workers = num_workers.clamp(1, 256);

        let work_queue = WorkStealingQueue::new();
        let (result_tx, result_rx) = mpsc::sync_channel::<UnitResult>(1);
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let active_workers = Arc::new(AtomicU32::new(0));
        let preset_dict = preset_dict.map(|s| s.to_vec()).map(Arc::new);

        let mut reader = Self {
            inner,
            result_rx,
            result_tx,
            current_work_unit: Vec::with_capacity(1024 * 1024),
            current_unit_unpacked: 0,
            current_chunk_sizes: Vec::new(),
            next_sequence_to_dispatch: 0,
            next_sequence_to_return: 0,
            last_sequence_id: None,
            out_of_order_chunks: BTreeMap::new(),
            current_chunk: Cursor::new(Vec::new()),
            pending_error: None,
            input_error: None,
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

    /// Reads one LZMA2 chunk from the inner reader and appends it to the current work unit.
    /// If the chunk is an independent block, it dispatches the current work unit.
    ///
    /// Returns `Ok(false)` on clean EOF, `Ok(true)` on success, and `Err` on I/O error. On
    /// error the current work unit holds whole chunks only: a chunk is kept once it has
    /// been read in full.
    fn read_and_dispatch_chunk(&mut self) -> io::Result<bool> {
        let mut control_buf = [0u8; 1];
        match self.inner.read_exact(&mut control_buf) {
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                // Clean end of stream.
                return Ok(false);
            }
            Err(error) => return Err(error),
        }

        let control = control_buf[0];

        if control == 0x00 {
            // End of stream marker.
            self.current_work_unit.push(0x00);
            self.send_work_unit();
            return Ok(false);
        }

        if control < 0x80 && control != 0x01 && control != 0x02 {
            // Not a control byte. The unit goes to a worker as it is, so the decoder
            // rejects it after every chunk before it, with the error the single-threaded
            // reader reports.
            self.current_work_unit.push(control);
            self.send_work_unit();
            return Ok(false);
        }

        let is_independent_chunk = control >= 0xE0 || control == 0x01;

        // Split work units before independent chunks (but not for the very first chunk).
        if is_independent_chunk && !self.current_work_unit.is_empty() {
            self.current_work_unit.push(0x00);
            self.send_work_unit();
        }

        let chunk_start = self.current_work_unit.len();
        let result = self.read_chunk(control);
        if result.is_err() {
            self.current_work_unit.truncate(chunk_start);
        }
        result.map(|()| true)
    }

    /// Appends the chunk that starts with `control` (a valid control byte, not the end
    /// marker) to the current work unit.
    fn read_chunk(&mut self, control: u8) -> io::Result<()> {
        self.current_work_unit.push(control);

        let (unpacked_size, chunk_data_size) = if control >= 0x80 {
            // Compressed chunk. Read header to find size.
            let header_len = if control >= 0xC0 { 5 } else { 4 };
            let mut header_buf = [0; 5];
            self.inner.read_exact(&mut header_buf[..header_len])?;
            self.current_work_unit
                .extend_from_slice(&header_buf[..header_len]);
            let unpacked_size = (((control & 0x1F) as usize) << 16)
                + u16::from_be_bytes([header_buf[0], header_buf[1]]) as usize
                + 1;
            let packed_size = u16::from_be_bytes([header_buf[2], header_buf[3]]) as usize + 1;
            (unpacked_size, packed_size)
        } else {
            // Uncompressed chunk.
            let mut size_buf = [0u8; 2];
            self.inner.read_exact(&mut size_buf)?;
            self.current_work_unit.extend_from_slice(&size_buf);
            let size = u16::from_be_bytes(size_buf) as usize + 1;
            (size, size)
        };
        let unit_unpacked = self.checked_unpacked_size(unpacked_size)?;

        // Read the chunk data itself.
        let start_len = self.current_work_unit.len();
        self.current_work_unit
            .resize(start_len + chunk_data_size, 0);
        self.inner
            .read_exact(&mut self.current_work_unit[start_len..])?;

        self.current_unit_unpacked = unit_unpacked;
        self.current_chunk_sizes.push(unpacked_size);
        Ok(())
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

        let data = core::mem::replace(&mut self.current_work_unit, Vec::with_capacity(1024 * 1024));
        let chunk_sizes = core::mem::take(&mut self.current_chunk_sizes);
        self.current_unit_unpacked = 0;

        let seq = self.next_sequence_to_dispatch;
        if !self.work_queue.push(WorkUnit {
            seq,
            data,
            chunk_sizes,
        }) {
            // The queue is closed only once the reader has failed or is being dropped.
            self.input_error = Some(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "worker threads have shut down",
            ));
            self.stop_dispatching();
            return;
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

        self.next_sequence_to_dispatch += 1;
    }

    /// No more work is dispatched; the reader waits for the units already sent.
    fn stop_dispatching(&mut self) {
        // Input without a chunk dispatches nothing to wait for.
        self.last_sequence_id = self.next_sequence_to_dispatch.checked_sub(1);
        self.state = State::Draining;
    }

    /// Files a worker's result. A failed unit ends the stream there: nothing after it is
    /// dispatched any more, and results of later units are dropped.
    fn accept_result(&mut self, result: UnitResult) {
        if self.last_sequence_id.is_some_and(|last| result.seq > last)
            && !matches!(self.state, State::Reading)
        {
            return;
        }
        if result.error.is_some() {
            if matches!(self.state, State::Reading) {
                self.current_work_unit.clear();
                self.current_chunk_sizes.clear();
                self.current_unit_unpacked = 0;
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
                    // let every later unit's output pile up behind it. One read
                    // dispatches at most one unit, so checking before it is enough.
                    if self.work_queue.len() < self.max_workers as usize
                        && self.units_in_flight() <= u64::from(self.max_workers)
                    {
                        match self.read_and_dispatch_chunk() {
                            Ok(true) => {
                                // Successfully read and dispatched a chunk, loop to continue.
                                continue;
                            }
                            Ok(false) => {
                                // Clean EOF from inner reader.
                                // Send any remaining data as the final work unit.
                                self.send_work_unit();
                                self.stop_dispatching();
                                continue;
                            }
                            Err(error) => {
                                // Every chunk read in full before the failure still
                                // decodes, as it does single-threaded; the error comes
                                // after them.
                                if !self.current_work_unit.is_empty() {
                                    self.current_work_unit.push(0x00);
                                }
                                self.send_work_unit();
                                self.input_error = Some(error);
                                self.stop_dispatching();
                                continue;
                            }
                        }
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

/// Decodes one work unit chunk by chunk, so that a failure keeps exactly the chunks
/// before the one that failed, whatever read sizes the decoding happened to use.
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

    // The chunk headers say how much comes out, so the result buffer can be
    // sized once instead of growing (and copying) its way there. A header
    // claiming more than can be reserved falls back to growth. The reader
    // checked that the sum fits a `usize`.
    let unpacked: usize = unit.chunk_sizes.iter().sum();
    let mut decompressed_data = Vec::new();
    if decompressed_data.try_reserve_exact(unpacked).is_err() {
        decompressed_data = Vec::with_capacity(unit.data.len());
    }

    for &size in &unit.chunk_sizes {
        let start = decompressed_data.len();
        let result = (&mut reader)
            .take(size as u64)
            .read_to_end(&mut decompressed_data);
        let error = match result {
            Ok(n) if n == size => continue,
            Ok(_) => io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "LZMA2 chunk ended before its declared size",
            ),
            Err(error) => error,
        };
        decompressed_data.truncate(start);
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
            let bytes_read = self.current_chunk.read(buf)?;
            if bytes_read > 0 {
                return Ok(bytes_read);
            }

            if let Some(error) = self.pending_error.take() {
                return Err(self.fail(error));
            }
            if let State::Failed(kind, message) = &self.state {
                return Err(io::Error::new(*kind, message.clone()));
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

    /// A worker that panics fails its unit the way a decoding error does: the units
    /// before it come out whole, then the error, and the reader does not wait forever
    /// for a result that never comes.
    #[test]
    fn a_worker_panic_fails_its_unit() {
        let (stream, plain) = stored_stream(64 * 1024, 3);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = Lzma2ReaderMt::with_options(stream.as_slice(), 4096, None, 2, Some(2));
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

        let error = reader.read_and_dispatch_chunk().unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "LZMA2 work unit unpacked size exceeds the platform limit"
        );
    }
}
