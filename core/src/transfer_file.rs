//! Single-file transfer over QUIC.
//!
//! The sender pushes consecutive chunks onto a single continuous
//! unidirectional QUIC stream, framing each one with:
//!
//! ```text
//! [chunk_index : u64 LE | flags : u8 | payload_len : u32 LE | payload]
//! ```
//!
//! A stream carries up to `BATCH_MAX_BYTES` of frames before the sender
//! `finish()`es it and opens the next — no per-chunk `stopped()` wait
//! (引擎B). QUIC's per-stream flow control + packet retransmission
//! provides the back-pressure and reliability the old per-chunk ACK /
//! CRC32 layer used to provide; TLS 1.3 AEAD authenticates every byte so
//! a chunk-level CRC stays redundant.
//!
//! File-level integrity is checked with SHA-256: the sender hashes
//! incrementally as it reads chunks in order; the receiver hashes them in
//! place as it writes (引擎A) and only re-reads the file if the write-back
//! went out of order. The two sides compare via `FileChecksum` over the
//! control stream.

use std::collections::HashSet;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, trace, warn};

use crate::bandwidth::BandwidthLimiter;
use crate::compression::{AdaptiveCompressor, Decompressor};
use crate::error::{Error, Result};
use crate::network::quic::QuicConnection;
use crate::progress::ProgressState;
use crate::protocol::ConfigMessage;

/// 引擎B：单条 uni stream 每次最多承载的帧净负载字节（含帧头）。
/// 低于单流 RECEIVE_WINDOW (64 MiB)，发送端可不被对端读取阻塞地写满
/// 整批再 `finish()`，无需逐块等待 `stopped()`。
const BATCH_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 接收端单条 uni stream 的读取上限；须 ≥ `BATCH_MAX_BYTES + 帧头余量`。
const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;

/// Maximum per-file size we'll honour from a peer-supplied manifest.
/// Without this cap a hostile peer can advertise a multi-petabyte
/// `file_size`, leading the receiver to compute an enormous
/// `total_chunks` and block in `accept_uni()` forever (finding 4.1).
/// 1 TiB is large enough for any plausible single-file transfer.
pub const MAX_TRANSFER_FILE_SIZE: u64 = 1024 * 1024 * 1024 * 1024;

/// Reject a peer-supplied per-file size that exceeds the sanity bound.
/// Called from the folder-receive path before any stream is accepted.
pub fn validate_file_size(size: u64) -> Result<()> {
    if size > MAX_TRANSFER_FILE_SIZE {
        return Err(Error::Protocol(format!(
            "peer-supplied file size {size} exceeds maximum {MAX_TRANSFER_FILE_SIZE}"
        )));
    }
    Ok(())
}

/// Ceiling-divide a file size by chunk size — number of full + final
/// chunks needed to cover `file_size` bytes when laid out under
/// `chunk_size`. Single source of truth (the sender, receiver, and
/// resume state all must agree).
pub const fn chunk_count(file_size: u64, chunk_size: u32) -> u64 {
    let cs = chunk_size as u64;
    (file_size + cs - 1) / cs
}

/// Per-frame header: `[index: u64 LE | flags: u8 | payload_len: u32 LE]`.
const CHUNK_HEADER_BYTES: usize = 13;

/// Flag bit 0: payload is zstd-compressed.
const FLAG_COMPRESSED: u8 = 0b0000_0001;

/// File transfer session. Borrows the QUIC connection; never owns it.
pub struct FileTransferSession<'a> {
    connection: &'a mut QuicConnection,
    config: ConfigMessage,
    bandwidth_limiter: Option<BandwidthLimiter>,
    pub compressed_bytes_sent: u64,
    pub uncompressed_bytes_sent: u64,
}

impl<'a> FileTransferSession<'a> {
    pub fn new(connection: &'a mut QuicConnection, config: ConfigMessage) -> Self {
        let bandwidth_limiter = if config.bandwidth_limit > 0 {
            Some(BandwidthLimiter::new(config.bandwidth_limit))
        } else {
            None
        };
        Self {
            connection,
            config,
            bandwidth_limiter,
            compressed_bytes_sent: 0,
            uncompressed_bytes_sent: 0,
        }
    }

    /// Send a file to the peer as a single continuous stream of frames,
    /// skipping any chunk indices already present in `completed_chunks`
    /// (resume). Frame layout on the wire:
    ///
    /// ```text
    /// [chunk_index : u64 LE | flags : u8 | payload_len : u32 LE | payload]
    /// ```
    ///
    /// Frames accumulate in `batch`; each uni-stream carries up to
    /// `BATCH_MAX_BYTES` of frames before `finish()` and the next stream
    /// opens (引擎B). This removes the old per-chunk `stopped()` wait —
    /// QUIC per-stream flow control provides back-pressure, TLS 1.3 AEAD
    /// authenticates every byte, and one `stopped()` per batch is enough to
    /// keep the final batch from being torn down with the connection.
    ///
    /// Returns the SHA-256 of the complete file (computed incrementally as
    /// chunks are read in order).
    pub async fn send_file<F>(
        &mut self,
        path: &Path,
        completed_chunks: &[u64],
        mut chunk_complete_callback: Option<F>,
        mut progress: Option<&mut ProgressState>,
    ) -> Result<[u8; 32]>
    where
        F: FnMut(u64),
    {
        debug!("Starting file send: {:?}", path);

        let mut reader = ChunkReader::new(path, self.config.chunk_size as usize).await?;
        let total_chunks = reader.total_chunks();

        if !completed_chunks.is_empty() {
            info!(
                "Resuming: {} of {} chunks already completed",
                completed_chunks.len(),
                total_chunks
            );
        }

        // AdaptiveCompressor carries decision state; we route calls through an
        // Arc<Mutex> so a large chunk's CPU burst can run off the async
        // executor (spawn_blocking) without moving the compressor itself.
        let compressor: Option<Arc<Mutex<AdaptiveCompressor>>> = if self.config.compression_enabled
        {
            let sample_size = if self.config.adaptive_compression {
                3
            } else {
                0
            };
            Some(Arc::new(Mutex::new(AdaptiveCompressor::new(
                self.config.compression_level,
                sample_size,
            ))))
        } else {
            None
        };

        // O(1) lookup vs O(n) on a slice — matters once a resume bitmap
        // covers tens of thousands of chunks.
        let completed: HashSet<u64> = completed_chunks.iter().copied().collect();

        // Frames accumulate here and are flushed as one batch per uni-stream.
        let mut batch: Vec<u8> = Vec::with_capacity(64 * 1024);

        for chunk_index in 0..total_chunks {
            if completed.contains(&chunk_index) {
                trace!("Skipping already-completed chunk {}", chunk_index);
                // ChunkReader.read_chunk seeks per call, so skipping is safe;
                // but we still need to fold the chunk into the SHA-256.
                reader.fold_chunk(chunk_index).await?;
                continue;
            }

            let chunk_data = reader.read_chunk(chunk_index).await?;
            let uncompressed_size = chunk_data.len() as u64;

            let (final_data, is_compressed) = if let Some(comp) = &compressor {
                compress_chunk(comp, &chunk_data).await?
            } else {
                (chunk_data, false)
            };

            if let Some(limiter) = &self.bandwidth_limiter {
                limiter.wait_for_tokens(final_data.len()).await;
            }

            // Flush the current batch before appending a frame that would
            // push it past one stream's capacity.
            let frame_len = CHUNK_HEADER_BYTES + final_data.len();
            if batch.len() as u64 + frame_len as u64 > BATCH_MAX_BYTES {
                self.flush_batch(&batch).await?;
                batch.clear();
            }

            let header_start = batch.len();
            batch.resize(header_start + CHUNK_HEADER_BYTES, 0u8);
            batch[header_start..header_start + 8].copy_from_slice(&chunk_index.to_le_bytes());
            batch[header_start + 8] = if is_compressed { FLAG_COMPRESSED } else { 0 };
            batch[header_start + 9..header_start + 13]
                .copy_from_slice(&(final_data.len() as u32).to_le_bytes());
            batch.extend_from_slice(&final_data);

            self.compressed_bytes_sent += final_data.len() as u64;
            self.uncompressed_bytes_sent += uncompressed_size;

            if let Some(ref mut p) = progress {
                p.add_bytes(uncompressed_size);
            }
            if let Some(ref mut cb) = chunk_complete_callback {
                cb(chunk_index);
            }

            trace!("Sent chunk {}/{}", chunk_index + 1, total_chunks);
        }

        if !batch.is_empty() {
            self.flush_batch(&batch).await?;
            batch.clear();
        }

        let checksum = reader.finalize_checksum();
        debug!("File send complete, SHA256: {:02x?}", &checksum[..8]);
        Ok(checksum)
    }

    /// Receive a file from the peer. `file_size` is the file's REAL byte
    /// count (from the sender manifest); `total_chunks` is its total chunk
    /// count (used as the bound for incoming `chunk_index` values);
    /// `streams_to_receive` is the number of DISTINCT chunk_indices the
    /// sender will deliver — `total_chunks - already_sent` on a resume.
    ///
    /// `ChunkWriter` sizes the `.partial` to `file_size` and the SHA-256 fast
    /// path triggers when the in-order prefix reaches exactly `file_size`, so
    /// a non-chunk-multiple tail file is hashed incrementally too (no
    /// post-transfer re-read) — unless the write-back went out of order.
    ///
    /// Each uni-stream carries one or more frames; the loop parses every
    /// frame's `payload_len` to advance through the stream (引擎B).
    ///
    /// Duplicate frame indices are dropped with a warn so a buggy or hostile
    /// peer cannot make the loop terminate while leaving a real chunk
    /// missing (finding 1.6).
    pub async fn receive_file(
        &mut self,
        output_path: &Path,
        file_size: u64,
        total_chunks: u64,
        streams_to_receive: u64,
        mut chunk_complete_callback: Option<impl FnMut(u64)>,
        mut progress: Option<&mut ProgressState>,
    ) -> Result<[u8; 32]> {
        debug!(
            "Starting file receive: {:?} ({} bytes, {} chunks, {} distinct indices expected)",
            output_path, file_size, total_chunks, streams_to_receive
        );
        if streams_to_receive > total_chunks {
            return Err(Error::Protocol(format!(
                "streams_to_receive {streams_to_receive} > total_chunks {total_chunks}"
            )));
        }

        let writer =
            ChunkWriter::new(output_path, self.config.chunk_size as usize, file_size).await?;
        let mut decompressor: Option<Decompressor> = if self.config.compression_enabled {
            Some(Decompressor::new())
        } else {
            None
        };

        // 引擎B：磁盘写挪进独立后台任务，网络读与写盘重叠，吞掉逐次写盘的
        // 延迟。有界 channel 提供背压：写盘落后时收端读自然停顿，内存有上界。
        // 哈希仍在有序写回路径就地累计（见 ChunkWriter::record_hash），语义
        // 与原先同步写完全一致。
        let (batch_tx, batch_rx) = mpsc::channel::<WriteItem>(2);
        let (done_tx, done_rx) = oneshot::channel::<Result<[u8; 32]>>();
        tokio::spawn(write_loop(writer, batch_rx, done_tx));

        let mut seen: HashSet<u64> = HashSet::with_capacity(streams_to_receive as usize);
        // 解压后的在途缓冲上限：一条流解压可远超其线上字节（高压缩率），
        // 按解压字节数限流，防止在手机上吃爆内存。channel 容量 2 × 该值
        // 即为写盘任务允许追赶网络读的最大内存差。
        let mut current_batch: Vec<(u64, Vec<u8>)> = Vec::with_capacity(256);
        let mut current_bytes: usize = 0;
        while (seen.len() as u64) < streams_to_receive {
            let mut stream = self.connection.accept_uni().await?;
            let raw = stream
                .read_to_end(MAX_STREAM_BYTES)
                .await
                .map_err(|e| Error::Quic(format!("chunk stream read: {e}")))?;

            // Parse every frame in this stream: a stream may hold many
            // chunks (引擎B). `off` advances by header + actual payload_len.
            let mut off = 0usize;
            while off < raw.len() {
                if raw.len() - off < CHUNK_HEADER_BYTES {
                    return Err(Error::Protocol(format!(
                        "truncated frame header at byte {off} of {}",
                        raw.len()
                    )));
                }
                let chunk_index =
                    u64::from_le_bytes(raw[off..off + 8].try_into().expect("8 bytes"));
                let flags = raw[off + 8];
                let payload_len =
                    u32::from_le_bytes(raw[off + 9..off + 13].try_into().expect("4 bytes"))
                        as usize;

                if chunk_index >= total_chunks {
                    return Err(Error::Protocol(format!(
                        "chunk_index {chunk_index} >= total_chunks {total_chunks}"
                    )));
                }
                let header_end = off + CHUNK_HEADER_BYTES;
                if payload_len > raw.len() - header_end {
                    return Err(Error::Protocol(format!(
                        "frame payload_len {payload_len} exceeds stream remainder {}",
                        raw.len() - header_end
                    )));
                }
                let payload = &raw[header_end..header_end + payload_len];
                off = header_end + payload_len;

                if !seen.insert(chunk_index) {
                    // Duplicate frame (e.g. a re-sent chunk during resume).
                    // The hash was already counted on first write; ignore.
                    warn!(
                        "duplicate chunk_index {chunk_index} in stream; ignoring (already received)"
                    );
                    continue;
                }

                // 解出/复制为自有数据交由后台写；progress/回调仍按接收字节
                // 即时上报，前端进度不因磁盘而卡顿。
                let data = if flags & FLAG_COMPRESSED != 0 {
                    let decomp = decompressor.as_mut().ok_or_else(|| {
                        Error::Protocol(
                            "compressed chunk but compression disabled in config".to_string(),
                        )
                    })?;
                    decomp.decompress(payload)?
                } else {
                    payload.to_vec()
                };
                let written = data.len() as u64;
                current_batch.push((chunk_index, data));
                current_bytes += written as usize;
                if current_bytes >= FLUSH_BATCH_BYTES {
                    batch_tx
                        .send(WriteItem::Batch(std::mem::take(&mut current_batch)))
                        .await
                        .map_err(|_| {
                            Error::Other("file writer task ended unexpectedly".to_string())
                        })?;
                    current_bytes = 0;
                }

                if let Some(ref mut p) = progress {
                    p.add_bytes(written);
                }
                if let Some(ref mut cb) = chunk_complete_callback {
                    cb(chunk_index);
                }

                trace!(
                    "Received chunk {} ({}/{})",
                    chunk_index,
                    seen.len(),
                    streams_to_receive
                );
            }

            if !current_batch.is_empty() {
                batch_tx
                    .send(WriteItem::Batch(std::mem::take(&mut current_batch)))
                    .await
                    .map_err(|_| Error::Other("file writer task ended unexpectedly".to_string()))?;
            }
        }

        // 正常收齐：发终止令牌让写盘任务落盘并 finalize（乱序/续传时整读）。
        batch_tx
            .send(WriteItem::Finalize)
            .await
            .map_err(|_| Error::Other("file writer task ended unexpectedly".to_string()))?;
        let checksum = done_rx
            .await
            .map_err(|_| Error::Other("file writer task terminated".to_string()))??;
        debug!("File receive complete, SHA256: {:02x?}", &checksum[..8]);
        Ok(checksum)
    }

    /// Open one uni-stream and write a whole batch of frames in a single
    /// `write_all`, then `finish()`. One `stopped()` wait per batch (i.e. per
    /// `BATCH_MAX_BYTES`) replaces the old per-chunk wait; it still guarantees
    /// the peer drained the batch before the connection can be torn down.
    async fn flush_batch(&self, batch: &[u8]) -> Result<()> {
        let mut stream = self.connection.open_uni().await?;
        stream
            .write_all(batch)
            .await
            .map_err(|e| Error::Quic(format!("write batch: {e}")))?;
        stream
            .finish()
            .map_err(|e| Error::Quic(format!("finish stream: {e}")))?;
        stream
            .stopped()
            .await
            .map_err(|e| Error::Quic(format!("stream stopped: {e}")))?;
        Ok(())
    }
}

/// Message to the background disk writer task (引擎B). `Finalize` tells it
/// to `sync_all` + rename + finalize once all batches are flushed; if the
/// channel closes without `Finalize`, the write-back aborted early and the
/// task drops the writer without renaming any `.partial` into place.
enum WriteItem {
    Batch(Vec<(u64, Vec<u8>)>),
    Finalize,
}

/// Background disk writer: drains batches, coalesces contiguous ordered
/// frames into single seeks/writes (引擎D), keeps the incremental hash in
/// sync, then finalizes on `Finalize`. Runs detached from the network
/// reader so disk latency overlaps the next stream's reads.
async fn write_loop(
    mut writer: ChunkWriter,
    mut rx: mpsc::Receiver<WriteItem>,
    done: oneshot::Sender<Result<[u8; 32]>>,
) {
    let result = async {
        let mut group: Vec<(u64, Vec<u8>)> = Vec::new();
        while let Some(item) = rx.recv().await {
            match item {
                WriteItem::Batch(batch) => {
                    for (idx, data) in batch {
                        if group.last().map(|(li, _)| *li + 1 == idx).unwrap_or(false) {
                            group.push((idx, data));
                        } else {
                            write_contiguous(&mut writer, &mut group).await?;
                            group.push((idx, data));
                        }
                    }
                }
                WriteItem::Finalize => break,
            }
        }
        write_contiguous(&mut writer, &mut group).await?;
        writer.finalize().await
    }
    .await;
    let _ = done.send(result);
}

/// Flush a run of (index, data) frames to disk in one contiguous write
/// when possible, preserving the chunk writer's hash bookkeeping.
async fn write_contiguous(writer: &mut ChunkWriter, group: &mut Vec<(u64, Vec<u8>)>) -> Result<()> {
    if group.is_empty() {
        return Ok(());
    }
    writer.write_chunks(group).await?;
    group.clear();
    Ok(())
}

/// Chunks at/above this size have their zstd compression pushed off the
/// async executor; smaller ones pay the task-spawn overhead for nothing.
const COMPRESS_OFFLOAD_BYTES: usize = 64 * 1024;

/// Max decompressed bytes buffered per batch handed to the background disk
/// writer, bounding memory regardless of the on-wire (compressed) batch size.
const FLUSH_BATCH_BYTES: usize = 64 * 1024 * 1024;

/// Compress one chunk, offloading CPU-bound zstd work to `spawn_blocking`
/// for large chunks so the sender task stays responsive to the network and
/// cancellation. Compressor state (adaptive decision) is guarded by a Mutex
/// and consumed in strict per-chunk order.
async fn compress_chunk(
    comp: &Arc<Mutex<AdaptiveCompressor>>,
    data: &[u8],
) -> Result<(Vec<u8>, bool)> {
    if data.len() >= COMPRESS_OFFLOAD_BYTES {
        let comp = comp.clone();
        let owned = data.to_vec();
        Ok(tokio::task::spawn_blocking(move || {
            let mut cm = comp
                .lock()
                .map_err(|p| Error::Compression(format!("compressor lock poisoned: {p}")))?;
            let (compressed, was_compressed, _decision) = cm.compress(&owned)?;
            Ok((compressed, was_compressed))
        })
        .await
        .map_err(|e| Error::Compression(format!("compress task panicked: {e}")))?)
    } else {
        let mut cm = comp
            .lock()
            .map_err(|p| Error::Compression(format!("compressor lock poisoned: {p}")))?;
        let (compressed, was_compressed, _decision) = cm.compress(data)?;
        Ok((compressed, was_compressed))
    }
}

// ----------------------------------------------------------------------
// Chunk reader (sender side) — streams the file in order, hashes inline.
// ----------------------------------------------------------------------

pub struct ChunkReader {
    file: File,
    chunk_size: usize,
    total_chunks: u64,
    file_size: u64,
    hasher: Sha256,
}

impl ChunkReader {
    pub async fn new(path: &Path, chunk_size: usize) -> Result<Self> {
        let file = File::open(path).await.map_err(|e| {
            Error::Network(std::io::Error::new(
                e.kind(),
                format!("Failed to open file {:?}: {}", path, e),
            ))
        })?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len();
        let total_chunks = chunk_count(file_size, chunk_size as u32);
        Ok(Self {
            file,
            chunk_size,
            total_chunks,
            file_size,
            hasher: Sha256::new(),
        })
    }

    pub fn total_chunks(&self) -> u64 {
        self.total_chunks
    }

    /// Read `index`-th chunk from disk, updating the running SHA-256.
    pub async fn read_chunk(&mut self, index: u64) -> Result<Vec<u8>> {
        let offset = index * self.chunk_size as u64;
        self.file.seek(SeekFrom::Start(offset)).await?;
        let remaining = self.file_size - offset;
        let to_read = remaining.min(self.chunk_size as u64) as usize;
        let mut buffer = vec![0u8; to_read];
        self.file.read_exact(&mut buffer).await?;
        self.hasher.update(&buffer);
        Ok(buffer)
    }

    /// Read `index`-th chunk and fold it into the running SHA-256 but
    /// discard the bytes. Used during resume to keep the running hash
    /// over the full file even when we don't re-send the chunk.
    pub async fn fold_chunk(&mut self, index: u64) -> Result<()> {
        let _ = self.read_chunk(index).await?;
        Ok(())
    }

    pub fn finalize_checksum(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

// ----------------------------------------------------------------------
// Chunk writer (receiver side) — writes chunks at arbitrary offsets,
// then re-reads the file from disk to compute the SHA-256.
// ----------------------------------------------------------------------

pub struct ChunkWriter {
    file: File,
    path: PathBuf,
    chunk_size: usize,
    /// 引擎A：写回时就地累计 SHA-256，有序快路径下 finalize 免整文件重读。
    hasher: Sha256,
    /// 已连续写入的前缀长度（偏移）；用于判定写回是否有序。
    next_expected: u64,
    /// 出现缺块/乱序后，finalize 需回退整文件重读。
    out_of_order: bool,
    /// 目标文件总字节数，用于判断前缀是否写满。
    file_size: u64,
}

impl ChunkWriter {
    /// Open (or create) the `<path>.partial` file, sizing it to the file's
    /// REAL byte count `file_size`. Any leftover partial from an earlier
    /// session longer than that (e.g. the user changed `--chunk-size`
    /// between sessions, or the file was externally truncated to a larger
    /// size) is truncated back to `file_size` so stale trailing bytes never
    /// survive into `finalize` (finding 1.7). Legitimate ordered writes of
    /// the last, short tail chunk never exceed `file_size`.
    pub async fn new(path: &Path, chunk_size: usize, file_size: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut partial = path.as_os_str().to_os_string();
        partial.push(".partial");
        let partial = PathBuf::from(partial);

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&partial)
            .await
            .map_err(|e| {
                Error::Network(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create file {:?}: {}", partial, e),
                ))
            })?;

        let current_len = file.metadata().await?.len();
        if current_len > file_size {
            warn!(
                ".partial file {:?} is {} bytes; truncating to expected {} bytes",
                partial, current_len, file_size
            );
            file.set_len(file_size).await?;
        }

        Ok(Self {
            file,
            path: path.to_path_buf(),
            chunk_size,
            hasher: Sha256::new(),
            next_expected: 0,
            out_of_order: false,
            file_size,
        })
    }

    /// Write a chunk at its absolute offset. 引擎A：不再逐块 `sync_data`
    /// （与 FlyingCarpet 一致，靠 `finalize().sync_all()` 一次性落盘），
    /// 并在写回有序时就地累计 SHA-256，令 finalize 无需整文件重读。
    pub async fn write_chunk(&mut self, index: u64, data: &[u8]) -> Result<()> {
        let offset = index * self.chunk_size as u64;
        self.file.seek(SeekFrom::Start(offset)).await?;
        self.file.write_all(data).await?;
        self.record_hash(index, data);
        Ok(())
    }

    /// 写回一段（尽量）连续有序的帧。引擎D：连续帧先拼接为单一写缓冲区，
    /// 一次 seek + 一次 write_all，砍掉逐帧 seek 的 syscall 开销；哈希按帧
    /// 就地累计，语义与逐帧 [`write_chunk`](Self::write_chunk) 完全一致。
    pub async fn write_chunks(&mut self, chunks: &[(u64, Vec<u8>)]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let start = chunks[0].0;
        let contiguous = chunks
            .iter()
            .enumerate()
            .all(|(i, (idx, _))| *idx == start + i as u64);
        if contiguous {
            let total: usize = chunks.iter().map(|(_, d)| d.len()).sum();
            let mut buf: Vec<u8> = Vec::with_capacity(total);
            for (_, d) in chunks {
                buf.extend_from_slice(d);
            }
            let offset = start * self.chunk_size as u64;
            self.file.seek(SeekFrom::Start(offset)).await?;
            self.file.write_all(&buf).await?;
            for (i, (_, d)) in chunks.iter().enumerate() {
                self.record_hash(start + i as u64, d);
            }
        } else {
            for (idx, d) in chunks {
                let offset = idx * self.chunk_size as u64;
                self.file.seek(SeekFrom::Start(offset)).await?;
                self.file.write_all(d).await?;
                self.record_hash(*idx, d);
            }
        }
        Ok(())
    }

    /// 有序快路径的哈希记账：前缀连续则就地累计，重复块忽略，否则标记乱序。
    fn record_hash(&mut self, index: u64, data: &[u8]) {
        if self.out_of_order {
            return;
        }
        let offset = index * self.chunk_size as u64;
        if offset == self.next_expected {
            self.hasher.update(data);
            self.next_expected = self.next_expected.saturating_add(data.len() as u64);
        } else if offset < self.next_expected {
            // 重复块（resume 重发已收块）：哈希已计，忽略。
        } else {
            // 缺块/乱序：后续回退整读。
            self.out_of_order = true;
        }
    }

    fn partial_path(&self) -> PathBuf {
        let mut p = self.path.as_os_str().to_os_string();
        p.push(".partial");
        PathBuf::from(p)
    }

    /// Sync once, rename `.partial` → final path, compute SHA-256. 引擎A：
    /// 快路径（有序写满）直接取增量哈希省去整文件重读；仅当发生乱序/
    /// 缺块（如续传）时回退整读，保证结果一致。
    pub async fn finalize(self) -> Result<[u8; 32]> {
        self.file.sync_all().await?;
        let partial_path = self.partial_path();
        let final_path = self.path.clone();
        drop(self.file);
        tokio::fs::rename(&partial_path, &final_path).await?;

        if !self.out_of_order && self.next_expected == self.file_size {
            info!("File finalized (incremental hash): {:?}", final_path);
            return Ok(self.hasher.finalize().into());
        }

        let mut hasher = Sha256::new();
        let mut f = File::open(&final_path).await?;
        // 1 MiB buffer amortises syscall overhead on the post-transfer
        // re-read; only reachable when the write-back was out of order.
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        info!("File finalized (re-read hash): {:?}", final_path);
        Ok(hasher.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    use crate::network::quic::QuicEndpoint;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;

    /// Single-file resume must not deadlock when the sender skips
    /// already-completed chunks: the receiver has to know to expect fewer
    /// streams than `total_chunks`.
    #[tokio::test]
    async fn resume_with_skipped_chunks_does_not_deadlock() {
        let chunk_size = 64usize;
        let total_chunks = 4u64;
        let completed = vec![0u64, 1];
        let file_bytes: Vec<u8> = (0..(chunk_size as u64 * total_chunks) as usize)
            .map(|i| (i % 251) as u8)
            .collect();

        let dir = tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");
        tokio::fs::write(&src, &file_bytes).await.unwrap();
        // Pre-populate the receiver's .partial with the chunks the sender will skip,
        // so the final SHA-256 verification matches.
        let mut partial_bytes = vec![0u8; file_bytes.len()];
        for &idx in &completed {
            let off = idx as usize * chunk_size;
            partial_bytes[off..off + chunk_size]
                .copy_from_slice(&file_bytes[off..off + chunk_size]);
        }
        let partial_path = {
            let mut p = dst.clone().into_os_string();
            p.push(".partial");
            std::path::PathBuf::from(p)
        };
        tokio::fs::write(&partial_path, &partial_bytes)
            .await
            .unwrap();

        let server_id = Arc::new(Identity::generate().unwrap());
        let server_fp = server_id.fingerprint();
        let server_ep = QuicEndpoint::bind(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            server_id.clone(),
        )
        .unwrap();
        let server_addr = server_ep.local_addr().unwrap();

        let cfg = ConfigMessage {
            compression_enabled: false,
            compression_level: 3,
            adaptive_compression: false,
            chunk_size: chunk_size as u32,
            bandwidth_limit: 0,
        };

        let dst_recv = dst.clone();
        let cfg_recv = cfg.clone();
        let src_size = file_bytes.len() as u64;
        let streams_to_receive = total_chunks - completed.len() as u64;
        let recv_task = tokio::spawn(async move {
            let mut conn = server_ep.accept().await.unwrap();
            let _ = conn.recv_message().await.unwrap(); // drive accept_bi
            let mut session = FileTransferSession::new(&mut conn, cfg_recv);
            session
                .receive_file(
                    &dst_recv,
                    src_size,
                    total_chunks,
                    streams_to_receive,
                    None::<fn(u64)>,
                    None,
                )
                .await
        });

        let client_id = Arc::new(Identity::generate().unwrap());
        let client_ep = QuicEndpoint::bind(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            client_id,
        )
        .unwrap();
        let mut conn = client_ep.connect(server_addr, server_fp).await.unwrap();
        conn.send_message(&crate::protocol::Message::Ping)
            .await
            .unwrap();
        let mut session = FileTransferSession::new(&mut conn, cfg);
        let send_fut = session.send_file(&src, &completed, None::<fn(u64)>, None);

        let recv_result = tokio::time::timeout(Duration::from_secs(5), async {
            let send_checksum = send_fut.await.unwrap();
            let recv_checksum = recv_task.await.unwrap().unwrap();
            assert_eq!(send_checksum, recv_checksum, "checksums must match");
            recv_checksum
        })
        .await;

        recv_result.expect("resume must finish within 5 s — receiver expected too many streams");
    }

    /// Finding 1.7: an over-long `.partial` (e.g. chunk_size shrank
    /// between sessions, or external truncation lengthened the file) must
    /// be brought back into a consistent state on resume. Otherwise stale
    /// trailing bytes survive into `finalize` and the SHA-256 mismatches.
    #[tokio::test]
    async fn chunk_writer_truncates_oversized_partial() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("out.bin");

        let mut partial_path = p.as_os_str().to_os_string();
        partial_path.push(".partial");
        let partial_path = std::path::PathBuf::from(partial_path);
        // Pre-seed an over-long partial: 4 chunks of junk where we only
        // expect 2 chunks of payload.
        tokio::fs::write(&partial_path, vec![0xAAu8; 64 * 4])
            .await
            .unwrap();

        let expected_size = 64u64 * 2;
        let _writer = ChunkWriter::new(&p, 64, expected_size).await.unwrap();

        let meta = tokio::fs::metadata(&partial_path).await.unwrap();
        assert_eq!(
            meta.len(),
            expected_size,
            "ChunkWriter::new must truncate over-long .partial down to expected_file_size"
        );
    }

    /// Finding 1.6: the receive loop counts streams, not distinct
    /// chunk_indices. A buggy or hostile sender that re-opens the same
    /// chunk_index satisfies the count and the loop terminates one short,
    /// silently leaving a hole. With proper dedup, duplicates are ignored
    /// and the loop continues until `streams_to_receive` DISTINCT chunks
    /// have arrived.
    #[tokio::test]
    async fn receive_file_dedups_duplicate_chunk_streams() {
        use std::time::Duration;

        let chunk_size = 64usize;
        let total_chunks = 3u64;
        let payload: Vec<u8> = (0..(chunk_size as u64 * total_chunks) as usize)
            .map(|i| (i % 251) as u8)
            .collect();

        let dir = tempdir().unwrap();
        let dst = dir.path().join("out.bin");

        let server_id = Arc::new(Identity::generate().unwrap());
        let server_fp = server_id.fingerprint();
        let server_ep = QuicEndpoint::bind(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            server_id.clone(),
        )
        .unwrap();
        let server_addr = server_ep.local_addr().unwrap();

        let cfg = ConfigMessage {
            compression_enabled: false,
            compression_level: 3,
            adaptive_compression: false,
            chunk_size: chunk_size as u32,
            bandwidth_limit: 0,
        };

        let dst_recv = dst.clone();
        let cfg_recv = cfg.clone();
        let src_size = payload.len() as u64;
        let recv_task = tokio::spawn(async move {
            let mut conn = server_ep.accept().await.unwrap();
            let _ = conn.recv_message().await.unwrap();
            let mut session = FileTransferSession::new(&mut conn, cfg_recv);
            // Pass total_chunks for both — sender opens 3 distinct streams
            // plus 1 duplicate of chunk 0.
            session
                .receive_file(
                    &dst_recv,
                    src_size,
                    total_chunks,
                    total_chunks,
                    None::<fn(u64)>,
                    None,
                )
                .await
        });

        let client_id = Arc::new(Identity::generate().unwrap());
        let client_ep = QuicEndpoint::bind(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            client_id,
        )
        .unwrap();
        let mut conn = client_ep.connect(server_addr, server_fp).await.unwrap();
        conn.send_message(&crate::protocol::Message::Ping)
            .await
            .unwrap();

        // Manually open 4 single-frame streams: [0, 0 (dup), 1, 2], each
        // carrying a 13-byte frame header (index | flags | payload_len).
        // A dedup-correct receiver must complete after seeing 3 distinct
        // indices.
        let order = [0u64, 0, 1, 2];
        for &idx in &order {
            let mut stream = conn.open_uni().await.unwrap();
            let mut header = [0u8; CHUNK_HEADER_BYTES];
            header[..8].copy_from_slice(&idx.to_le_bytes());
            header[8] = 0; // flags = 0 (uncompressed)
            header[9..13].copy_from_slice(&(chunk_size as u32).to_le_bytes());
            stream.write_all(&header).await.unwrap();
            let off = (idx as usize) * chunk_size;
            stream
                .write_all(&payload[off..off + chunk_size])
                .await
                .unwrap();
            stream.finish().unwrap();
            stream.stopped().await.unwrap();
        }

        let recv_result = tokio::time::timeout(Duration::from_secs(5), recv_task)
            .await
            .expect("receiver must complete within 5 s — duplicates should not stall the loop")
            .unwrap()
            .unwrap();

        // SHA-256 of the assembled payload should match the original.
        let expected = {
            let mut h = Sha256::new();
            h.update(&payload);
            let r: [u8; 32] = h.finalize().into();
            r
        };
        assert_eq!(recv_result, expected);
    }

    /// Finding 4.1: peer-supplied file sizes must be sanity-bounded.
    /// Without a cap, a hostile peer can advertise a multi-petabyte
    /// file_size, leading the receiver to compute an enormous
    /// total_chunks and block in accept_uni() forever.
    #[test]
    fn validate_file_size_rejects_absurd_values() {
        assert!(super::validate_file_size(0).is_ok());
        assert!(super::validate_file_size(MAX_TRANSFER_FILE_SIZE).is_ok());
        assert!(super::validate_file_size(MAX_TRANSFER_FILE_SIZE + 1).is_err());
        assert!(super::validate_file_size(u64::MAX).is_err());
    }

    #[tokio::test]
    async fn chunk_reader_reads_and_hashes() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("test.bin");
        let data = vec![0x42u8; 200];
        tokio::fs::write(&p, &data).await.unwrap();

        let mut reader = ChunkReader::new(&p, 64).await.unwrap();
        assert_eq!(reader.total_chunks(), 4u64);

        for i in 0..reader.total_chunks() {
            let _ = reader.read_chunk(i).await.unwrap();
        }
        let sha = reader.finalize_checksum();

        let expected = {
            let mut h = Sha256::new();
            h.update(&data);
            let r: [u8; 32] = h.finalize().into();
            r
        };
        assert_eq!(sha, expected);
    }

    #[tokio::test]
    async fn chunk_writer_assembles_out_of_order() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("out.bin");
        let mut writer = ChunkWriter::new(&p, 64, 200).await.unwrap();

        writer.write_chunk(2u64, &[0x02u8; 64]).await.unwrap();
        writer.write_chunk(0u64, &[0x00u8; 64]).await.unwrap();
        writer.write_chunk(1u64, &[0x01u8; 64]).await.unwrap();
        writer.write_chunk(3u64, &[0x03u8; 8]).await.unwrap();

        let sha = writer.finalize().await.unwrap();
        let bytes = tokio::fs::read(&p).await.unwrap();
        assert_eq!(bytes.len(), 200);

        let expected = {
            let mut h = Sha256::new();
            h.update(&bytes);
            let r: [u8; 32] = h.finalize().into();
            r
        };
        assert_eq!(sha, expected);
    }
}
