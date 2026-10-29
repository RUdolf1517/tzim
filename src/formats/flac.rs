//! FLAC re-encoding behind the optional `flac` feature.
//!
//! Audio is decoded and compared with Claxon (a different implementation from
//! the flacenc writer). Metadata blocks are copied verbatim except STREAMINFO's
//! frame statistics and the SEEKTABLE's sample/byte locations, which must be
//! updated for the new frame layout. A failed, refused, or non-smaller rewrite
//! never reaches the original file; the pipeline installs only a verified,
//! smaller candidate.

use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use flacenc::bitsink::ByteSink;
use flacenc::component::{BitRepr, StreamInfo};
use flacenc::error::Verify as FlacVerify;
use flacenc::source::{Fill, FrameBuf, Source};
use md5::{Digest, Md5};

use crate::error::{Error, Result};

use super::{Cand, Options};

const MARKER: &[u8; 4] = b"fLaC";
const STREAMINFO_BYTES: usize = 34;
const METADATA_HEADER_BYTES: usize = 4;
const MAX_METADATA_BLOCKS: usize = 4096;
const MAX_FRAME_COUNT: usize = 1 << 31;
static SPOOL_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
struct MetadataBlock {
    kind: u8,
    data_start: usize,
    len: usize,
}

impl MetadataBlock {
    fn data_range(self) -> std::ops::Range<usize> {
        self.data_start..self.data_start + self.len
    }
}

#[derive(Debug)]
struct FlacFile {
    blocks: Vec<MetadataBlock>,
    frame_start: usize,
    seektable_index: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct FrameStart {
    sample: u64,
    offset: u64,
    samples: u16,
}

struct Spool {
    path: PathBuf,
    file: File,
}

impl Spool {
    fn create() -> Result<Self> {
        let dir = std::env::temp_dir();
        for _ in 0..128 {
            let id = SPOOL_ID.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("tzim-flac-{}-{id}.frames", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(Error::Io(error)),
            }
        }
        Err(Error::Io(std::io::Error::other(
            "could not allocate a unique FLAC frame spool",
        )))
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn refusal(reason: impl std::fmt::Display) -> Error {
    Error::refused(format!("flac: {reason}; the file is untouched"))
}

fn parse(data: &[u8]) -> Result<FlacFile> {
    if !data.starts_with(MARKER) {
        if data.starts_with(b"ID3") {
            return Err(refusal(
                "a leading ID3v2 tag is outside the supported FLAC subset",
            ));
        }
        return Err(Error::parse("flac: missing fLaC stream marker"));
    }

    let mut pos = MARKER.len();
    let mut blocks = Vec::new();
    let mut seektable_index = None;
    loop {
        if blocks.len() >= MAX_METADATA_BLOCKS {
            return Err(refusal("more than 4096 metadata blocks"));
        }
        let header_end = pos
            .checked_add(METADATA_HEADER_BYTES)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| Error::parse("flac: truncated metadata block header"))?;
        let first = data[pos];
        let last = first & 0x80 != 0;
        let kind = first & 0x7f;
        if kind > 6 {
            return Err(refusal(format!("reserved metadata block type {kind}")));
        }
        let len = ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | data[pos + 3] as usize;
        let data_start = header_end;
        let end = data_start
            .checked_add(len)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| Error::parse("flac: metadata block extends beyond end of file"))?;
        let block = MetadataBlock {
            kind,
            data_start,
            len,
        };
        if blocks.is_empty() && (kind != 0 || len != STREAMINFO_BYTES) {
            return Err(refusal(
                "STREAMINFO is not the first 34-byte metadata block",
            ));
        }
        if kind == 0 && (!blocks.is_empty() || len != STREAMINFO_BYTES) {
            return Err(refusal("duplicate or malformed STREAMINFO block"));
        }
        if kind == 3 {
            if seektable_index.replace(blocks.len()).is_some() {
                return Err(refusal("more than one SEEKTABLE block"));
            }
            if len % 18 != 0 {
                return Err(refusal("SEEKTABLE length is not a multiple of 18 bytes"));
            }
        }
        blocks.push(block);
        pos = end;
        if last {
            break;
        }
    }
    if pos >= data.len() {
        return Err(Error::parse("flac: stream has no audio frames"));
    }
    Ok(FlacFile {
        blocks,
        frame_start: pos,
        seektable_index,
    })
}

fn streaminfo_md5(data: &[u8], file: &FlacFile) -> Result<[u8; 16]> {
    let block = file
        .blocks
        .first()
        .ok_or_else(|| Error::parse("flac: missing STREAMINFO"))?;
    let info = &data[block.data_range()];
    let mut digest = [0u8; 16];
    digest.copy_from_slice(&info[18..34]);
    Ok(digest)
}

fn write_metadata_header<W: Write>(out: &mut W, kind: u8, last: bool, len: usize) -> Result<()> {
    if kind > 126 || len > 0x00ff_ffff {
        return Err(refusal("metadata block cannot be represented"));
    }
    out.write_all(&[(if last { 0x80 } else { 0 }) | kind])?;
    out.write_all(&[(len >> 16) as u8, (len >> 8) as u8, len as u8])?;
    Ok(())
}

fn decode_info(data: &[u8]) -> Result<claxon::metadata::StreamInfo> {
    let reader = claxon::FlacReader::new(Cursor::new(data))
        .map_err(|error| refusal(format!("independent decoder rejected input: {error}")))?;
    let info = reader.streaminfo();
    if info.samples.unwrap_or(0) == 0 {
        return Err(refusal(
            "STREAMINFO does not give a non-zero total sample count",
        ));
    }
    if !(1..=8).contains(&info.channels)
        || !matches!(info.bits_per_sample, 8 | 16 | 24)
        || info.sample_rate == 0
    {
        return Err(refusal(
            "sample format is outside the flacenc-supported subset",
        ));
    }
    Ok(info)
}

/// A bounded-memory source adapter. Claxon yields one decoded block at a time;
/// the encoder receives at most one 4096-sample frame buffer, rather than a
/// second full decoded copy of a long recording.
struct DecoderSource<'a> {
    reader: claxon::FlacReader<Cursor<&'a [u8]>>,
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
    total_samples: usize,
    samples_read: usize,
    next_block_sample: u64,
    current: Option<claxon::Block>,
    current_pos: u32,
    reuse: Vec<i32>,
    md5: Md5,
    end_checked: bool,
}

impl<'a> DecoderSource<'a> {
    fn new(data: &'a [u8], info: claxon::metadata::StreamInfo) -> Result<Self> {
        let total_samples = usize::try_from(info.samples.unwrap_or(0))
            .map_err(|_| refusal("sample count does not fit this platform"))?;
        let reader = claxon::FlacReader::new(Cursor::new(data))
            .map_err(|error| refusal(format!("independent decoder rejected input: {error}")))?;
        Ok(Self {
            reader,
            channels: info.channels as usize,
            bits_per_sample: info.bits_per_sample as usize,
            sample_rate: info.sample_rate as usize,
            total_samples,
            samples_read: 0,
            next_block_sample: 0,
            current: None,
            current_pos: 0,
            reuse: Vec::new(),
            md5: Md5::new(),
            end_checked: false,
        })
    }

    fn next_block(
        &mut self,
    ) -> std::result::Result<Option<claxon::Block>, flacenc::error::SourceError> {
        let buffer = std::mem::take(&mut self.reuse);
        let mut frames = self.reader.blocks();
        let block = frames.read_next_or_eof(buffer).map_err(|_| {
            flacenc::error::SourceError::by_reason(flacenc::error::SourceErrorReason::InvalidFormat)
        })?;
        if let Some(block) = &block {
            if block.channels() as usize != self.channels
                || u64::from(block.time()) != self.next_block_sample
                || block.duration() == 0
            {
                return Err(flacenc::error::SourceError::by_reason(
                    flacenc::error::SourceErrorReason::InvalidFormat,
                ));
            }
            self.next_block_sample = self
                .next_block_sample
                .saturating_add(u64::from(block.duration()));
        }
        Ok(block)
    }

    fn check_eof(&mut self) -> std::result::Result<(), flacenc::error::SourceError> {
        if self.end_checked {
            return Ok(());
        }
        if let Some(block) = self.current.take() {
            if self.current_pos < block.duration() {
                return Err(flacenc::error::SourceError::by_reason(
                    flacenc::error::SourceErrorReason::InvalidFormat,
                ));
            }
            self.reuse = block.into_buffer();
        }
        if self.next_block()?.is_some() {
            return Err(flacenc::error::SourceError::by_reason(
                flacenc::error::SourceErrorReason::InvalidFormat,
            ));
        }
        self.end_checked = true;
        Ok(())
    }
}

impl Source for DecoderSource<'_> {
    fn channels(&self) -> usize {
        self.channels
    }

    fn bits_per_sample(&self) -> usize {
        self.bits_per_sample
    }

    fn sample_rate(&self) -> usize {
        self.sample_rate
    }

    fn len_hint(&self) -> Option<usize> {
        Some(self.total_samples)
    }

    fn read_samples<F: Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> std::result::Result<usize, flacenc::error::SourceError> {
        if self.samples_read == self.total_samples {
            self.check_eof()?;
            return Ok(0);
        }
        if self.samples_read > self.total_samples {
            return Err(flacenc::error::SourceError::by_reason(
                flacenc::error::SourceErrorReason::InvalidFormat,
            ));
        }
        let wanted = block_size.min(self.total_samples - self.samples_read);
        let mut interleaved = Vec::with_capacity(wanted * self.channels);
        while interleaved.len() / self.channels < wanted {
            let exhausted = self
                .current
                .as_ref()
                .is_none_or(|block| self.current_pos >= block.duration());
            if exhausted {
                if let Some(block) = self.current.take() {
                    self.reuse = block.into_buffer();
                }
                self.current_pos = 0;
                self.current = self.next_block()?;
                if self.current.is_none() {
                    return Err(flacenc::error::SourceError::by_reason(
                        flacenc::error::SourceErrorReason::InvalidFormat,
                    ));
                }
            }
            let block = self.current.as_ref().expect("loaded above");
            let available = (block.duration() - self.current_pos) as usize;
            let take = available.min(wanted - interleaved.len() / self.channels);
            for sample in self.current_pos..self.current_pos + take as u32 {
                for channel in 0..self.channels {
                    let value = block.sample(channel as u32, sample);
                    self.md5
                        .update(&value.to_le_bytes()[..self.bits_per_sample.div_ceil(8)]);
                    interleaved.push(value);
                }
            }
            self.current_pos += take as u32;
        }
        dest.fill_interleaved(&interleaved)?;
        let count = interleaved.len() / self.channels;
        self.samples_read += count;
        Ok(count)
    }
}

fn encode_frames(
    input: &[u8],
    info: claxon::metadata::StreamInfo,
    original_md5: [u8; 16],
    frame_limit: u64,
) -> Result<Option<(Spool, Vec<FrameStart>, Vec<u8>)>> {
    let mut source = DecoderSource::new(input, info)?;
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|error| refusal(format!("flacenc configuration failed: {error}")))?;
    let block_size = config.block_size;
    if block_size < 16 || block_size > 65_535 {
        return Err(refusal(
            "flacenc block size is outside the FLAC frame range",
        ));
    }
    let channels = source.channels();
    let mut framebuf = FrameBuf::with_size(channels, block_size)
        .map_err(|error| refusal(format!("flacenc frame buffer failed: {error}")))?;
    let mut stream_info = StreamInfo::new(source.sample_rate(), channels, source.bits_per_sample())
        .map_err(|error| refusal(format!("flacenc STREAMINFO failed: {error}")))?;
    let mut spool = Spool::create()?;
    let mut starts = Vec::new();
    let mut sample_start = 0u64;
    let mut byte_offset = 0u64;
    let mut candidate_possible = true;

    loop {
        let count = source
            .read_samples(block_size, &mut framebuf)
            .map_err(|error| {
                refusal(format!(
                    "independent decoder failed during streaming: {error}"
                ))
            })?;
        if count == 0 {
            break;
        }
        if candidate_possible {
            if starts.len() >= MAX_FRAME_COUNT {
                return Err(refusal("more than 2^31 FLAC frames"));
            }
            let frame =
                flacenc::encode_fixed_size_frame(&config, &framebuf, starts.len(), &stream_info)
                    .map_err(|error| {
                        refusal(format!(
                            "flacenc could not encode the decoded samples: {error}"
                        ))
                    })?;
            let mut encoded = ByteSink::new();
            frame.write(&mut encoded).map_err(|error| {
                Error::Internal(format!("flacenc frame serialization failed: {error}"))
            })?;
            let frame_bytes = encoded.as_slice();
            if frame_bytes.is_empty() || frame_bytes.len() > 0x00ff_ffff {
                return Err(refusal("encoded frame has an invalid byte length"));
            }
            let next_byte_offset = byte_offset
                .checked_add(frame_bytes.len() as u64)
                .ok_or_else(|| refusal("encoded frame offset overflow"))?;
            if next_byte_offset > frame_limit {
                // Once the current output cannot beat the original even after
                // the pre-authorized metadata removal, stop encoding but keep
                // decoding the rest so CRC and the input MD5 are still checked.
                candidate_possible = false;
                continue;
            }
            let frame_samples = u16::try_from(frame.block_size())
                .map_err(|_| refusal("encoded frame block size exceeds FLAC limits"))?;
            starts.push(FrameStart {
                sample: sample_start,
                offset: byte_offset,
                samples: frame_samples,
            });
            sample_start = sample_start
                .checked_add(u64::from(frame_samples))
                .ok_or_else(|| refusal("sample count overflow"))?;
            byte_offset = next_byte_offset;
            stream_info.update_frame_info(&frame);
            spool.file.write_all(frame_bytes)?;
        }
    }
    source.check_eof().map_err(|error| {
        refusal(format!(
            "independent decoder failed at end of stream: {error}"
        ))
    })?;
    if (candidate_possible && sample_start != info.samples.unwrap_or(0))
        || source.samples_read != source.total_samples
    {
        return Err(refusal(
            "decoded frame count does not match STREAMINFO total samples",
        ));
    }
    let computed_md5: [u8; 16] = source.md5.clone().finalize().into();
    if original_md5 != [0u8; 16] && computed_md5 != original_md5 {
        return Err(refusal(
            "input STREAMINFO MD5 does not match the decoded PCM",
        ));
    }
    if !candidate_possible {
        return Ok(None);
    }
    stream_info.set_total_samples(source.total_samples);
    // Keep the input's checksum field byte-for-byte. If it was zero (legal),
    // do not silently turn verification into metadata editing.
    stream_info.set_md5_digest(&original_md5);

    let mut info_sink = ByteSink::new();
    stream_info.write(&mut info_sink).map_err(|error| {
        Error::Internal(format!("flacenc STREAMINFO serialization failed: {error}"))
    })?;
    if info_sink.as_slice().len() != STREAMINFO_BYTES {
        return Err(Error::Internal(
            "flacenc wrote a non-34-byte STREAMINFO".into(),
        ));
    }
    Ok(Some((spool, starts, info_sink.as_slice().to_vec())))
}

fn map_seek_table(
    data: &[u8],
    file: &FlacFile,
    block: MetadataBlock,
    starts: &[FrameStart],
    total: u64,
) -> Result<Vec<u8>> {
    let input = &data[block.data_range()];
    let original_frame_bytes = data.len().saturating_sub(file.frame_start) as u64;
    let mut output = Vec::with_capacity(input.len());
    let mut last_sample = None;
    let mut last_offset = None;
    let mut last_mapped_sample = None;
    let mut last_mapped_offset = None;
    let mut placeholders = false;
    for point in input.chunks_exact(18) {
        let sample = u64::from_be_bytes(point[0..8].try_into().unwrap());
        let offset = u64::from_be_bytes(point[8..16].try_into().unwrap());
        let frame_samples = u16::from_be_bytes(point[16..18].try_into().unwrap());
        if sample == u64::MAX {
            if offset != 0 || frame_samples != 0 {
                return Err(refusal("malformed SEEKTABLE placeholder"));
            }
            placeholders = true;
            output.extend_from_slice(point);
            continue;
        }
        if placeholders || sample >= total || offset >= original_frame_bytes || starts.is_empty() {
            return Err(refusal(
                "SEEKTABLE points are out of order or outside the audio",
            ));
        }
        if last_sample.is_some_and(|previous| sample <= previous)
            || last_offset.is_some_and(|previous| offset <= previous)
        {
            return Err(refusal("SEEKTABLE points are not strictly increasing"));
        }
        last_sample = Some(sample);
        last_offset = Some(offset);
        // FLAC seek points name a frame start. If the original target is not a
        // boundary in the new fixed-block stream, point to the containing frame
        // and update its sample number and byte offset together.
        let target = starts
            .iter()
            .take_while(|frame| frame.sample <= sample)
            .last()
            .ok_or_else(|| refusal("SEEKTABLE point precedes the first encoded frame"))?;
        if last_mapped_sample.is_some_and(|previous| target.sample <= previous)
            || last_mapped_offset.is_some_and(|previous| target.offset <= previous)
        {
            return Err(refusal(
                "distinct SEEKTABLE points collapse to the same re-encoded frame",
            ));
        }
        last_mapped_sample = Some(target.sample);
        last_mapped_offset = Some(target.offset);
        output.extend_from_slice(&target.sample.to_be_bytes());
        output.extend_from_slice(&target.offset.to_be_bytes());
        output.extend_from_slice(&target.samples.to_be_bytes());
    }
    if output.len() != input.len() {
        return Err(Error::Internal(
            "SEEKTABLE rewrite changed its length".into(),
        ));
    }
    Ok(output)
}

fn flac_crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn frame_header(data: &[u8], offset: usize) -> Option<(bool, u64, u16)> {
    let header = data.get(offset..)?;
    if header.len() < 6 || header[0] != 0xff || header[1] & 0xfe != 0xf8 || header[3] & 1 != 0 {
        return None;
    }
    let block_code = header[2] >> 4;
    let rate_code = header[2] & 0x0f;
    let sample_size_code = (header[3] >> 1) & 7;
    if block_code == 0
        || rate_code == 15
        || header[3] >> 4 > 10
        || matches!(sample_size_code, 3 | 7)
    {
        return None;
    }
    let strategy = header[1] & 1 != 0;
    let first = header[4];
    let number_bytes = if first & 0x80 == 0 {
        1
    } else if first & 0xe0 == 0xc0 {
        2
    } else if first & 0xf0 == 0xe0 {
        3
    } else if first & 0xf8 == 0xf0 {
        4
    } else if first & 0xfc == 0xf8 {
        5
    } else if first & 0xfe == 0xfc {
        6
    } else {
        return None;
    };
    let number = if number_bytes == 1 {
        u64::from(first & 0x7f)
    } else {
        let leading = 7usize - number_bytes;
        let mut value = u64::from(first & ((1u8 << leading) - 1));
        for index in 1..number_bytes {
            let byte = *header.get(4 + index)?;
            if byte & 0xc0 != 0x80 {
                return None;
            }
            value = (value << 6) | u64::from(byte & 0x3f);
        }
        value
    };
    let mut pos = 4 + number_bytes;
    let block_size = match block_code {
        1 => 192,
        2..=5 => 576u32 << (block_code - 2),
        6 => {
            let value = u32::from(*header.get(pos)?).checked_add(1)?;
            pos += 1;
            value
        }
        7 => {
            let value = u16::from_be_bytes([*header.get(pos)?, *header.get(pos + 1)?]) as u32 + 1;
            pos += 2;
            value
        }
        8..=15 => 256u32 << (block_code - 8),
        _ => return None,
    };
    match rate_code {
        12 => pos += 1,
        13 | 14 => pos += 2,
        _ => {}
    }
    let crc_index = pos;
    let actual_crc = *header.get(crc_index)?;
    if flac_crc8(&header[..crc_index]) != actual_crc
        || block_size == 0
        || block_size > u16::MAX as u32
        || number > (1u64 << 36) - 1
    {
        return None;
    }
    Some((strategy, number, block_size as u16))
}

fn validate_frame_sequence(data: &[u8], info: claxon::metadata::StreamInfo) -> Result<()> {
    let expected = info
        .samples
        .ok_or_else(|| refusal("STREAMINFO total samples are unknown"))?;
    let mut reader = claxon::FlacReader::new(Cursor::new(data))
        .map_err(|error| refusal(format!("Claxon rejected stream: {error}")))?;
    let mut frames = reader.blocks();
    let mut next_sample = 0u64;
    let mut minimum = u16::MAX;
    let mut maximum = 0u16;
    let mut reuse = Vec::new();
    loop {
        let block = frames
            .read_next_or_eof(reuse)
            .map_err(|error| refusal(format!("Claxon frame validation failed: {error}")))?;
        let Some(block) = block else { break };
        let count = u16::try_from(block.duration())
            .map_err(|_| refusal("frame block size exceeds STREAMINFO range"))?;
        if block.time() != next_sample {
            return Err(refusal("FLAC frame sample numbers have a gap or overlap"));
        }
        next_sample = next_sample.saturating_add(u64::from(count));
        minimum = minimum.min(count);
        maximum = maximum.max(count);
        reuse = block.into_buffer();
    }
    if next_sample != expected || minimum != info.min_block_size || maximum != info.max_block_size {
        return Err(refusal(
            "STREAMINFO block statistics disagree with decoded frames",
        ));
    }
    Ok(())
}

fn seektable_valid(data: &[u8], file: &FlacFile, total: u64, max_block_size: u16) -> Result<()> {
    let Some(index) = file.seektable_index else {
        return Ok(());
    };
    let block = file.blocks[index];
    let bytes = &data[block.data_range()];
    let frame_bytes = &data[file.frame_start..];
    let mut last_sample = None;
    let mut last_offset = None;
    let mut placeholders = false;
    for point in bytes.chunks_exact(18) {
        let sample = u64::from_be_bytes(point[0..8].try_into().unwrap());
        let offset = u64::from_be_bytes(point[8..16].try_into().unwrap());
        let count = u16::from_be_bytes(point[16..18].try_into().unwrap());
        if sample == u64::MAX {
            if offset != 0 || count != 0 {
                return Err(Error::verify("FLAC SEEKTABLE placeholder is malformed"));
            }
            placeholders = true;
            continue;
        }
        if placeholders
            || sample >= total
            || count == 0
            || offset
                .checked_add(2)
                .is_none_or(|end| end > frame_bytes.len() as u64)
        {
            return Err(Error::verify(
                "FLAC SEEKTABLE point is outside the encoded audio",
            ));
        }
        if last_sample.is_some_and(|previous| sample <= previous)
            || last_offset.is_some_and(|previous| offset <= previous)
        {
            return Err(Error::verify(
                "FLAC SEEKTABLE points are not strictly ordered",
            ));
        }
        let at = offset as usize;
        let (variable, coded_number, actual_count) =
            frame_header(frame_bytes, at).ok_or_else(|| {
                Error::verify("FLAC SEEKTABLE offset does not point to a valid frame header")
            })?;
        let frame_sample = if variable {
            coded_number
        } else {
            coded_number
                .checked_mul(u64::from(max_block_size))
                .ok_or_else(|| Error::verify("FLAC fixed-block frame number overflows"))?
        };
        if frame_sample != sample || actual_count != count {
            return Err(Error::verify(
                "FLAC SEEKTABLE sample or block length disagrees with its frame",
            ));
        }
        last_sample = Some(sample);
        last_offset = Some(offset);
    }
    Ok(())
}

fn compare_pcm(original: &[u8], candidate: &[u8]) -> Result<()> {
    let mut left = claxon::FlacReader::new(Cursor::new(original)).map_err(|error| {
        Error::verify(format!("original FLAC is unreadable by Claxon: {error}"))
    })?;
    let mut right = claxon::FlacReader::new(Cursor::new(candidate)).map_err(|error| {
        Error::verify(format!("candidate FLAC is unreadable by Claxon: {error}"))
    })?;
    let a = left.streaminfo();
    let b = right.streaminfo();
    if a.channels != b.channels
        || a.sample_rate != b.sample_rate
        || a.bits_per_sample != b.bits_per_sample
        || a.samples != b.samples
    {
        return Err(Error::verify(
            "FLAC sample format or total sample count changed",
        ));
    }
    let expected = a
        .samples
        .ok_or_else(|| Error::verify("FLAC total samples are unknown"))?;
    let mut digest = Md5::new();
    let bytes_per_sample = (a.bits_per_sample as usize).div_ceil(8);
    let mut samples_seen = 0u64;
    let mut left_samples = left.samples();
    let mut right_samples = right.samples();
    loop {
        match (left_samples.next(), right_samples.next()) {
            (None, None) => break,
            (Some(Ok(x)), Some(Ok(y))) if x == y => {
                digest.update(&x.to_le_bytes()[..bytes_per_sample]);
                samples_seen = samples_seen.saturating_add(1);
            }
            (Some(Ok(_)), Some(Ok(_))) => {
                return Err(Error::verify(
                    "independent Claxon PCM comparison found a changed sample",
                ));
            }
            (Some(Err(error)), _) => {
                return Err(Error::verify(format!(
                    "Claxon could not decode original FLAC: {error}"
                )));
            }
            (_, Some(Err(error))) => {
                return Err(Error::verify(format!(
                    "Claxon could not decode candidate FLAC: {error}"
                )));
            }
            _ => return Err(Error::verify("FLAC decoded sample counts differ")),
        }
    }
    if samples_seen != expected.saturating_mul(u64::from(a.channels)) {
        return Err(Error::verify(
            "FLAC decoded sample count disagrees with STREAMINFO",
        ));
    }
    let actual: [u8; 16] = digest.finalize().into();
    let stored = streaminfo_md5(
        original,
        &parse(original).map_err(|error| Error::verify(error.to_string()))?,
    )?;
    if stored != [0u8; 16] && stored != actual {
        return Err(Error::verify(
            "original FLAC STREAMINFO MD5 disagrees with decoded PCM",
        ));
    }
    let candidate_file = parse(candidate).map_err(|error| Error::verify(error.to_string()))?;
    if streaminfo_md5(candidate, &candidate_file)? != stored {
        return Err(Error::verify(
            "candidate FLAC STREAMINFO MD5 was not preserved",
        ));
    }
    Ok(())
}

fn compare_metadata(
    original: &[u8],
    a: &FlacFile,
    candidate: &[u8],
    b: &FlacFile,
    strip_metadata: bool,
) -> Result<()> {
    let expected = a
        .blocks
        .iter()
        .filter(|block| !(strip_metadata && removable_metadata(block.kind)))
        .collect::<Vec<_>>();
    if expected.len() != b.blocks.len() {
        return Err(Error::verify("FLAC metadata block count changed"));
    }
    for (left, right) in expected.into_iter().zip(&b.blocks) {
        if left.kind != right.kind || left.len != right.len {
            return Err(Error::verify(
                "FLAC metadata block type, order, or length changed",
            ));
        }
        let x = &original[left.data_range()];
        let y = &candidate[right.data_range()];
        match left.kind {
            0 => {
                if x.len() != STREAMINFO_BYTES || y.len() != STREAMINFO_BYTES || x[10..] != y[10..]
                {
                    return Err(Error::verify(
                        "FLAC STREAMINFO sample identity or MD5 changed",
                    ));
                }
            }
            3 => {
                // The seek table may move to the containing frame in the new
                // fixed-block stream. Its cardinality stays intact; offsets are
                // checked against decoded frames by the independent verifier.
                if x.len() != y.len() {
                    return Err(Error::verify("FLAC SEEKTABLE cardinality changed"));
                }
            }
            _ if x != y => return Err(Error::verify("FLAC non-target metadata changed")),
            _ => {}
        }
    }
    Ok(())
}

fn removable_metadata(kind: u8) -> bool {
    matches!(kind, 1 | 4 | 6) // PADDING, VORBIS_COMMENT, PICTURE
}

fn rewrite(
    input: &[u8],
    strip_metadata: bool,
) -> Result<Option<(Spool, FlacFile, Vec<FrameStart>, Vec<u8>)>> {
    let file = parse(input)?;
    let info = decode_info(input)?;
    let total = info.samples.unwrap_or(0);
    validate_frame_sequence(input, info)
        .map_err(|error| refusal(format!("invalid input frame sequence: {error}")))?;
    seektable_valid(input, &file, total, info.max_block_size)
        .map_err(|error| refusal(format!("invalid input SEEKTABLE: {error}")))?;
    let original_md5 = streaminfo_md5(input, &file)?;
    let removable_bytes = if strip_metadata {
        file.blocks
            .iter()
            .filter(|block| removable_metadata(block.kind))
            .map(|block| METADATA_HEADER_BYTES as u64 + block.len as u64)
            .sum::<u64>()
    } else {
        0
    };
    let original_metadata_len = file.frame_start.saturating_sub(MARKER.len()) as u64;
    let target_metadata_len = original_metadata_len.saturating_sub(removable_bytes);
    let may_keep_equal_size = removable_bytes > 0;
    let candidate_ceiling =
        (input.len() as u64).saturating_sub(if may_keep_equal_size { 0 } else { 1 });
    let frame_limit = candidate_ceiling
        .saturating_sub(MARKER.len() as u64)
        .saturating_sub(target_metadata_len);
    let Some((mut spool, starts, info_bytes)) =
        encode_frames(input, info, original_md5, frame_limit)?
    else {
        return Ok(None);
    };
    spool.file.flush()?;
    spool.file.seek(SeekFrom::Start(0))?;

    let retained = file
        .blocks
        .iter()
        .copied()
        .filter(|block| !(strip_metadata && removable_metadata(block.kind)))
        .collect::<Vec<_>>();
    let mut candidate_metadata = Vec::new();
    for (index, block) in retained.iter().copied().enumerate() {
        let last = index + 1 == retained.len();
        match block.kind {
            0 => {
                write_metadata_header(&mut candidate_metadata, 0, last, info_bytes.len())?;
                candidate_metadata.extend_from_slice(&info_bytes);
            }
            3 => {
                let seek = map_seek_table(input, &file, block, &starts, total)?;
                write_metadata_header(&mut candidate_metadata, 3, last, seek.len())?;
                candidate_metadata.extend_from_slice(&seek);
            }
            kind => {
                write_metadata_header(&mut candidate_metadata, kind, last, block.len)?;
                candidate_metadata.extend_from_slice(&input[block.data_range()]);
            }
        }
    }
    Ok(Some((spool, file, starts, candidate_metadata)))
}

pub fn optimize(input: &[u8], dst: &mut dyn Write, options: &Options) -> Result<Cand> {
    let Some((mut spool, file, _starts, metadata)) = rewrite(input, options.strip_metadata)? else {
        return Ok(Cand::NoGain);
    };
    let candidate_len = MARKER.len() as u64 + metadata.len() as u64 + spool.file.metadata()?.len();
    let original_metadata_len = file.frame_start.saturating_sub(MARKER.len());
    let metadata_removed = metadata.len() < original_metadata_len;
    if candidate_len > input.len() as u64 {
        return Ok(Cand::NoGain);
    }
    let candidate_size = usize::try_from(candidate_len)
        .map_err(|_| Error::Internal("FLAC candidate length does not fit this platform".into()))?;
    if !super::size_ok(input.len(), candidate_size, options.strip_metadata)
        || (candidate_len == input.len() as u64 && !metadata_removed)
    {
        return Ok(Cand::NoGain);
    }
    dst.write_all(MARKER)?;
    dst.write_all(&metadata)?;
    spool.file.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut spool.file, dst)?;
    Ok(Cand::Written(candidate_len))
}

pub fn verify(original: &[u8], candidate: &[u8], options: &Options) -> Result<()> {
    if !super::size_ok(original.len(), candidate.len(), options.strip_metadata) {
        return Err(Error::verify(
            "FLAC candidate violates the configured size rule",
        ));
    }
    let left = parse(original).map_err(|error| Error::verify(format!("original: {error}")))?;
    let right = parse(candidate).map_err(|error| Error::verify(format!("candidate: {error}")))?;
    let left_info =
        decode_info(original).map_err(|error| Error::verify(format!("original: {error}")))?;
    let right_info =
        decode_info(candidate).map_err(|error| Error::verify(format!("candidate: {error}")))?;
    if left_info.channels != right_info.channels
        || left_info.sample_rate != right_info.sample_rate
        || left_info.bits_per_sample != right_info.bits_per_sample
        || left_info.samples != right_info.samples
    {
        return Err(Error::verify("FLAC STREAMINFO sample identity changed"));
    }
    validate_frame_sequence(original, left_info)
        .map_err(|error| Error::verify(format!("original FLAC frame sequence: {error}")))?;
    validate_frame_sequence(candidate, right_info)
        .map_err(|error| Error::verify(format!("candidate FLAC frame sequence: {error}")))?;
    compare_metadata(original, &left, candidate, &right, options.strip_metadata)?;
    if candidate.len() == original.len()
        && !left
            .blocks
            .iter()
            .any(|block| removable_metadata(block.kind))
    {
        return Err(Error::verify(
            "equal-size FLAC rewrite did not remove requested metadata",
        ));
    }
    seektable_valid(
        candidate,
        &right,
        right_info.samples.unwrap_or(0),
        right_info.max_block_size,
    )?;
    compare_pcm(original, candidate)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flacenc::component::MetadataBlockData;
    use flacenc::source::MemSource;

    fn sample_flac(with_seek: bool, verbatim: bool) -> Vec<u8> {
        let channels = 2;
        let samples_per_channel = 8192;
        let mut pcm = Vec::with_capacity(samples_per_channel * channels);
        for sample in 0..samples_per_channel {
            let left = (((sample * 31) % 257) as i32 - 128) * 127;
            let right = (((sample * 17) % 251) as i32 - 125) * 129;
            pcm.push(left);
            pcm.push(right);
        }
        let mut encoder = flacenc::config::Encoder::default();
        encoder.multithread = false;
        if verbatim {
            encoder.block_size = 2048;
            encoder.subframe_coding.use_constant = false;
            encoder.subframe_coding.use_fixed = false;
            encoder.subframe_coding.use_lpc = false;
        }
        let encoder = encoder.into_verified().unwrap();
        let source = MemSource::from_samples(&pcm, channels, 16, 44_100);
        let mut stream =
            flacenc::encode_with_fixed_block_size(&encoder, source, encoder.block_size).unwrap();
        stream.add_metadata_block(MetadataBlockData::new_unknown(2, b"testAPPmetadata").unwrap());
        if with_seek {
            let mut seek = Vec::new();
            let mut offset = 0u64;
            let mut sample = 0u64;
            for index in 0..stream.frame_count() {
                let frame = stream.frame(index).unwrap();
                if index % 2 == 0 {
                    seek.extend_from_slice(&sample.to_be_bytes());
                    seek.extend_from_slice(&offset.to_be_bytes());
                    seek.extend_from_slice(&(frame.block_size() as u16).to_be_bytes());
                }
                let mut bytes = ByteSink::new();
                frame.write(&mut bytes).unwrap();
                offset += bytes.as_slice().len() as u64;
                sample += frame.block_size() as u64;
            }
            seek.extend_from_slice(&u64::MAX.to_be_bytes());
            seek.extend_from_slice(&0u64.to_be_bytes());
            seek.extend_from_slice(&0u16.to_be_bytes());
            stream.add_metadata_block(MetadataBlockData::new_unknown(3, &seek).unwrap());
        }
        let mut sink = ByteSink::new();
        stream.write(&mut sink).unwrap();
        sink.as_slice().to_vec()
    }

    #[test]
    fn streaming_rewrite_preserves_pcm_and_metadata_and_remaps_seekpoints() {
        let input = sample_flac(true, true);
        let mut candidate = Vec::new();
        let result = optimize(
            &input,
            &mut candidate,
            &Options {
                zopfli: false,
                max_pixels: 1,
                max_uncompressed: 1,
                max_ratio: 1,
                strip_metadata: false,
                nested: false,
                max_depth: 0,
                depth: 0,
                allow: crate::formats::Allow::default(),
                fits_tile: false,
            },
        )
        .unwrap();
        assert!(matches!(result, Cand::Written(_)));
        assert!(candidate.len() < input.len());
        verify(
            &input,
            &candidate,
            &Options {
                zopfli: false,
                max_pixels: 1,
                max_uncompressed: 1,
                max_ratio: 1,
                strip_metadata: false,
                nested: false,
                max_depth: 0,
                depth: 0,
                allow: crate::formats::Allow::default(),
                fits_tile: false,
            },
        )
        .unwrap();
        let before = parse(&input).unwrap();
        let after = parse(&candidate).unwrap();
        assert_eq!(
            before.blocks.iter().map(|b| b.kind).collect::<Vec<_>>(),
            after.blocks.iter().map(|b| b.kind).collect::<Vec<_>>()
        );
        assert_eq!(
            &input[before.blocks[1].data_range()],
            &candidate[after.blocks[1].data_range()],
            "APPLICATION metadata is copied exactly"
        );
        assert_ne!(
            &input[before.blocks[2].data_range()],
            &candidate[after.blocks[2].data_range()],
            "seek offsets and frame samples are updated"
        );
    }

    #[test]
    fn independent_verifier_rejects_pcm_and_seektable_negative_controls() {
        let input = sample_flac(true, true);
        let mut candidate = Vec::new();
        let options = Options {
            zopfli: false,
            max_pixels: 1,
            max_uncompressed: 1,
            max_ratio: 1,
            strip_metadata: false,
            nested: false,
            max_depth: 0,
            depth: 0,
            allow: crate::formats::Allow::default(),
            fits_tile: false,
        };
        let result = optimize(&input, &mut candidate, &options).unwrap();
        if !matches!(result, Cand::Written(_)) {
            return; // Some encoder upgrades may make this fixture already optimal.
        }
        let file = parse(&candidate).unwrap();
        let mut pcm_bad = candidate.clone();
        let frame = file.frame_start;
        pcm_bad[frame + 5] ^= 0x10;
        assert!(verify(&input, &pcm_bad, &options).is_err());

        if let Some(index) = file.seektable_index {
            let point = file.blocks[index].data_start;
            let mut seek_bad = candidate.clone();
            seek_bad[point + 8..point + 16].copy_from_slice(&u64::MAX.to_be_bytes());
            assert!(verify(&input, &seek_bad, &options).is_err());
        }
    }

    #[test]
    fn reserved_flac_sample_size_codes_are_rejected() {
        for sample_size_code in [3, 7] {
            let mut header = vec![0xff, 0xf8, 0x89, sample_size_code << 1, 0, 0];
            let crc = flac_crc8(&header[..5]);
            header[5] = crc;
            assert!(
                frame_header(&header, 0).is_none(),
                "reserved code {sample_size_code}"
            );
        }
        for sample_size_code in [0, 1, 2, 4, 5, 6] {
            let mut header = vec![0xff, 0xf8, 0x89, sample_size_code << 1, 0, 0];
            let crc = flac_crc8(&header[..5]);
            header[5] = crc;
            assert!(
                frame_header(&header, 0).is_some(),
                "valid code {sample_size_code}"
            );
        }
    }

    #[test]
    fn distinct_seekpoints_that_map_to_one_output_frame_are_refused() {
        let mut data = Vec::new();
        for (sample, offset) in [(100u64, 0u64), (200, 10)] {
            data.extend_from_slice(&sample.to_be_bytes());
            data.extend_from_slice(&offset.to_be_bytes());
            data.extend_from_slice(&1u16.to_be_bytes());
        }
        data.resize(100, 0);
        let file = FlacFile {
            blocks: Vec::new(),
            frame_start: 36,
            seektable_index: None,
        };
        let seektable = MetadataBlock {
            kind: 3,
            data_start: 0,
            len: 36,
        };
        let starts = [
            FrameStart {
                sample: 0,
                offset: 0,
                samples: 4096,
            },
            FrameStart {
                sample: 4096,
                offset: 20,
                samples: 4096,
            },
        ];
        let error = map_seek_table(&data, &file, seektable, &starts, 1000).unwrap_err();
        assert!(error
            .to_string()
            .contains("collapse to the same re-encoded frame"));
    }

    #[test]
    fn malformed_and_unsupported_streams_are_refused() {
        assert!(parse(b"ID3abc").is_err());
        assert!(parse(b"fLaC\x80\x00\x00\x21").is_err());
    }
}
