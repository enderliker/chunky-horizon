//! Region files: reading what a chunk's saved data says, and taking a chunk
//! out of one.
//!
//! Horizon keeps worlds the way Java Edition does, in `r.<x>.<z>.mca` region
//! files, so the port can read them itself. Three of the four payload codecs
//! are read here (gzip, zlib and uncompressed) and the fourth, lz4-java's
//! block framing, as `LZ4BlockInputStream` reads it; Horizon writes zlib by
//! default and takes `region-file-compression` for the others.
//!
//! Removing a chunk only ever zeroes its entry in the file's header, exactly
//! as Chunky does: the chunk's bytes stay where they are, the file does not
//! shrink, and every other chunk keeps its own bytes untouched. That is why
//! trimming cannot corrupt what it does not delete.

use crate::geometry::ChunkPos;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// One region file's side, in chunks.
pub const REGION_SIZE: i32 = 32;
/// How many chunks a region file holds.
pub const ENTRIES: usize = 1024;
/// The size of a region file's sectors.
pub const SECTOR_SIZE: usize = 4096;
/// The header's size: 1024 locations and 1024 timestamps.
pub const HEADER_SIZE: usize = ENTRIES * 4 * 2;

/// Which codec a chunk's payload uses. The external bit says the payload is
/// in a file of its own beside the region file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Gzip,
    Zlib,
    None,
    Lz4,
    External(u8),
    Unknown(u8),
}

impl Compression {
    pub fn from_byte(byte: u8) -> Self {
        if byte & 128 != 0 {
            return Self::External(byte & !128);
        }
        match byte {
            1 => Self::Gzip,
            2 => Self::Zlib,
            3 => Self::None,
            4 => Self::Lz4,
            other => Self::Unknown(other),
        }
    }
}

/// What a chunk's `Status` tag says it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkStatus(pub String);

impl ChunkStatus {
    /// A chunk the world has not filled in at all.
    pub fn is_empty(&self) -> bool {
        matches!(self.0.as_str(), "" | "empty" | "minecraft:empty")
    }

    /// A chunk that is finished: the only kind a `world` pattern queues.
    pub fn is_full(&self) -> bool {
        matches!(self.0.as_str(), "full" | "minecraft:full")
    }
}

/// An error from a region file.
#[derive(Debug)]
pub enum AnvilError {
    Io(std::io::Error),
    /// A payload whose codec could not be read, or whose bytes are corrupt.
    Malformed(String),
    /// A codec this port does not read.
    UnsupportedCompression(u8),
}

impl std::fmt::Display for AnvilError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Malformed(why) => write!(f, "{why}"),
            Self::UnsupportedCompression(byte) => {
                write!(f, "compression type {byte} is not one this mod reads")
            }
        }
    }
}

impl std::error::Error for AnvilError {}

impl From<std::io::Error> for AnvilError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The region coordinates of every `r.<x>.<z>.mca` in a folder, in the order
/// the folder lists them.
pub fn region_files(dir: &Path) -> Vec<(i32, i32)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| parse_region_name(&entry.file_name().to_string_lossy()))
        .collect()
}

/// `r.<x>.<z>.mca`, as `ChunkCoordinate.fromRegionFile` reads it.
pub fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?;
    let rest = rest.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

/// The name a region file has.
pub fn region_name(x: i32, z: i32) -> String {
    format!("r.{x}.{z}.mca")
}

/// The name an external chunk payload has, which is the region file's sibling.
pub fn external_name(chunk: ChunkPos) -> String {
    format!("c.{}.{}.mcc", chunk.x, chunk.z)
}

/// A region file open for reading, and for taking chunks out.
pub struct RegionFile {
    file: File,
    path: PathBuf,
    locations: [u32; ENTRIES],
    timestamps: [u32; ENTRIES],
    /// Whether the file was too short to hold a header.
    empty: bool,
}

impl RegionFile {
    pub fn open(path: &Path) -> Result<Self, AnvilError> {
        let mut file = File::open(path)?;
        let mut header = [0u8; HEADER_SIZE];
        let read = file.read(&mut header).unwrap_or(0);
        let empty = read < HEADER_SIZE;
        let mut locations = [0u32; ENTRIES];
        let mut timestamps = [0u32; ENTRIES];
        if !empty {
            for (index, location) in locations.iter_mut().enumerate() {
                *location =
                    u32::from_be_bytes(header[index * 4..index * 4 + 4].try_into().unwrap());
            }
            for (index, timestamp) in timestamps.iter_mut().enumerate() {
                let at = ENTRIES * 4 + index * 4;
                *timestamp = u32::from_be_bytes(header[at..at + 4].try_into().unwrap());
            }
        }
        Ok(Self {
            file,
            path: path.to_path_buf(),
            locations,
            timestamps,
            empty,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the file is too short to be a region file.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// The entry a chunk's location and timestamp live at, as the format
    /// indexes them: column first, then row.
    fn index(chunk: ChunkPos) -> usize {
        ((chunk.x & 31) + (chunk.z & 31) * 32) as usize
    }

    /// Whether the file holds a chunk there.
    pub fn has_chunk(&self, chunk: ChunkPos) -> bool {
        !self.empty && self.locations[Self::index(chunk)] != 0
    }

    /// The chunk's saved modification time in seconds, 0 when unknown.
    pub fn timestamp(&self, chunk: ChunkPos) -> u32 {
        if self.empty {
            0
        } else {
            self.timestamps[Self::index(chunk)]
        }
    }

    /// The codec a chunk's payload uses, without reading the payload.
    pub fn compression(&mut self, chunk: ChunkPos) -> Option<Compression> {
        let location = self.locations[Self::index(chunk)];
        if location == 0 {
            return None;
        }
        let offset = ((location >> 8) as u64 & 0xFF_FFFF) * SECTOR_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset)).ok()?;
        let mut byte = [0u8; 1];
        self.file.read_exact(&mut byte).ok()?;
        Some(Compression::from_byte(byte[0]))
    }

    /// A chunk's payload, decompressed.
    pub fn chunk_bytes(&mut self, chunk: ChunkPos) -> Result<Option<Vec<u8>>, AnvilError> {
        let location = self.locations[Self::index(chunk)];
        if location == 0 {
            return Ok(None);
        }
        let sector_count = (location & 0xFF) as usize;
        let offset = ((location >> 8) as u64 & 0xFF_FFFF) * SECTOR_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 5];
        if self.file.read_exact(&mut head).is_err() {
            return Ok(None);
        }
        let length = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
        let compression = Compression::from_byte(head[4]);
        if let Compression::External(_) = compression {
            return self.external_bytes(chunk).map(Some);
        }
        if length <= 1 || length > sector_count * SECTOR_SIZE - 4 {
            return Ok(None);
        }
        let mut payload = vec![0u8; length - 1];
        if self.file.read_exact(&mut payload).is_err() {
            return Ok(None);
        }
        decode(compression, &payload).map(Some)
    }

    /// A chunk kept in a file of its own beside the region file.
    fn external_bytes(&self, chunk: ChunkPos) -> Result<Vec<u8>, AnvilError> {
        let path = self
            .path
            .parent()
            .unwrap_or(Path::new("."))
            .join(external_name(chunk));
        let mut bytes = Vec::new();
        File::open(&path)?.read_to_end(&mut bytes)?;
        // An external chunk is a bare payload, compressed with the low bits of
        // the type byte.
        let mut file = File::open(&self.path)?;
        let mut byte = [0u8; 1];
        let location = self.locations[Self::index(chunk)];
        let offset = ((location >> 8) as u64 & 0xFF_FFFF) * SECTOR_SIZE as u64 + 4;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut byte)?;
        decode(Compression::from_byte(byte[0]), &bytes)
    }

    /// The chunk's `Status`, or `None` where no chunk is saved there.
    pub fn chunk_status(&mut self, chunk: ChunkPos) -> Option<ChunkStatus> {
        let tag = self.chunk_nbt(chunk).ok().flatten()?;
        let status = tag.get("Status")?.as_str()?;
        Some(ChunkStatus(status.to_owned()))
    }

    /// Whether the chunk is finished: full, lit, and updated at least once
    /// (`RegionStatusReader.isFull`).
    pub fn is_generated(&mut self, chunk: ChunkPos) -> bool {
        let Ok(Some(tag)) = self.chunk_nbt(chunk) else {
            return false;
        };
        let full = tag
            .get("Status")
            .and_then(|status| status.as_str())
            .is_some_and(|status| ChunkStatus(status.to_owned()).is_full());
        let lit = tag.get("isLightOn").is_some();
        let updated = tag
            .get("LastUpdate")
            .and_then(Tag::as_long)
            .is_some_and(|value| value > 0);
        let right_place = tag
            .get("xPos")
            .and_then(Tag::as_long)
            .zip(tag.get("zPos").and_then(Tag::as_long))
            .is_some_and(|(x, z)| x == chunk.x as i64 && z == chunk.z as i64);
        full && lit && updated && right_place
    }

    /// How long players have spent in the chunk, 0 when it says nothing.
    pub fn inhabited_time(&mut self, chunk: ChunkPos) -> i64 {
        let Ok(Some(tag)) = self.chunk_nbt(chunk) else {
            return 0;
        };
        tag.get("InhabitedTime").and_then(Tag::as_long).unwrap_or(0)
    }

    /// The chunk's saved data.
    pub fn chunk_nbt(&mut self, chunk: ChunkPos) -> Result<Option<Tag>, AnvilError> {
        let Some(bytes) = self.chunk_bytes(chunk)? else {
            return Ok(None);
        };
        match Tag::parse(&bytes) {
            Ok(tag) => Ok(Some(tag)),
            Err(error) => Err(AnvilError::Malformed(error)),
        }
    }

    /// Takes a chunk out of the file: its entry in the location table becomes
    /// zero, which is how the format says a chunk is not there. Nothing else in
    /// the file moves — not the chunk's bytes, not its timestamp, which is what
    /// Chunky's trim leaves behind.
    ///
    /// Returns whether the chunk was there. An external payload, if the chunk
    /// had one, is removed with it.
    pub fn clear_chunk(&mut self, chunk: ChunkPos) -> Result<bool, AnvilError> {
        let index = Self::index(chunk);
        if self.empty || self.locations[index] == 0 {
            return Ok(false);
        }
        let external = matches!(self.compression(chunk), Some(Compression::External(_)));
        self.locations[index] = 0;
        let mut file = std::fs::OpenOptions::new().write(true).open(&self.path)?;
        file.seek(SeekFrom::Start((index * 4) as u64))?;
        file.write_all(&0u32.to_be_bytes())?;
        file.flush()?;
        if external {
            let path = self
                .path
                .parent()
                .unwrap_or(Path::new("."))
                .join(external_name(chunk));
            let _ = std::fs::remove_file(path);
        }
        Ok(true)
    }
}

/// Decompresses a chunk payload.
pub fn decode(compression: Compression, payload: &[u8]) -> Result<Vec<u8>, AnvilError> {
    match compression {
        Compression::None => Ok(payload.to_vec()),
        Compression::Gzip => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(payload)
                .read_to_end(&mut out)
                .map_err(|error| AnvilError::Malformed(format!("gzip chunk: {error}")))?;
            Ok(out)
        }
        Compression::Zlib => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(payload)
                .read_to_end(&mut out)
                .map_err(|error| AnvilError::Malformed(format!("zlib chunk: {error}")))?;
            Ok(out)
        }
        Compression::Lz4 => decode_lz4(payload),
        Compression::External(inner) => decode(Compression::from_byte(inner), payload),
        Compression::Unknown(byte) => Err(AnvilError::UnsupportedCompression(byte)),
    }
}

/// The `LZ4Block` framing `LZ4BlockInputStream` reads: a 21-byte header a
/// block, then its bytes, ended by a block of no bytes.
const LZ4_MAGIC: &[u8; 8] = b"LZ4Block";

fn decode_lz4(mut bytes: &[u8]) -> Result<Vec<u8>, AnvilError> {
    let mut out = Vec::new();
    loop {
        if bytes.len() < 21 {
            return Err(AnvilError::Malformed("truncated LZ4 header".into()));
        }
        let header = &bytes[..21];
        if &header[..8] != LZ4_MAGIC {
            return Err(AnvilError::Malformed("invalid LZ4 block magic".into()));
        }
        let method = header[8] & 0xF0;
        let compressed = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;
        let raw = u32::from_le_bytes(header[13..17].try_into().unwrap()) as usize;
        bytes = &bytes[21..];
        if raw == 0 {
            return Ok(out);
        }
        if method != 0x10 && method != 0x20 {
            return Err(AnvilError::Malformed("invalid LZ4 block method".into()));
        }
        if bytes.len() < compressed {
            return Err(AnvilError::Malformed("truncated LZ4 payload".into()));
        }
        let block = &bytes[..compressed];
        bytes = &bytes[compressed..];
        let start = out.len();
        if method == 0x10 {
            if raw != compressed {
                return Err(AnvilError::Malformed("invalid LZ4 stored block".into()));
            }
            out.extend_from_slice(block);
        } else {
            out.resize(start + raw, 0);
            lz4_flex::block::decompress_into(block, &mut out[start..])
                .map_err(|error| AnvilError::Malformed(format!("LZ4 chunk: {error}")))?;
        }
    }
}

/// An NBT tag, as much of one as a chunk's saved data needs.
///
/// Only the tags a pre-generator reads are kept whole; a list or an array
/// keeps its shape so a caller can walk it, and everything else is read past.
#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    /// A tag's field of a compound, by name.
    pub fn get(&self, name: &str) -> Option<&Tag> {
        match self {
            Self::Compound(fields) => fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, tag)| tag),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match self {
            Self::Byte(value) => Some(*value as i64),
            Self::Short(value) => Some(*value as i64),
            Self::Int(value) => Some(*value as i64),
            Self::Long(value) => Some(*value),
            _ => None,
        }
    }

    /// Reads one tag, its type byte first, as a chunk's saved data is.
    pub fn parse(bytes: &[u8]) -> Result<Tag, String> {
        let mut reader = Reader { bytes, at: 0 };
        let kind = reader.byte()?;
        if kind != 10 {
            return Err(format!("a chunk's root is a compound, not tag {kind}"));
        }
        // The root's name is usually empty.
        let _ = reader.string()?;
        reader.compound(0)
    }
}

/// How deep a compound may nest, and how many tags one may hold: a corrupt
/// file must not be able to exhaust memory.
const MAX_DEPTH: u32 = 64;
const MAX_TAGS: usize = 1 << 20;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .ok_or("an NBT tag runs past the end")?;
        if end > self.bytes.len() {
            return Err("an NBT tag runs past the end".into());
        }
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn short(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn int(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn long(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, String> {
        let length = self.short()?;
        if length < 0 {
            return Err("a negative string length".into());
        }
        let bytes = self.take(length as usize)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn compound(&mut self, depth: u32) -> Result<Tag, String> {
        if depth > MAX_DEPTH {
            return Err("an NBT compound nests too deeply".into());
        }
        let mut fields = Vec::new();
        loop {
            let kind = self.byte()?;
            if kind == 0 {
                return Ok(Tag::Compound(fields));
            }
            if fields.len() >= MAX_TAGS {
                return Err("an NBT compound holds too many tags".into());
            }
            let name = self.string()?;
            let value = self.tag(kind, depth + 1)?;
            fields.push((name, value));
        }
    }

    fn list(&mut self, depth: u32) -> Result<Tag, String> {
        let kind = self.byte()?;
        let length = self.int()?;
        if length < 0 || length as usize > MAX_TAGS {
            return Err("an NBT list has a length out of range".into());
        }
        let mut items = Vec::with_capacity(length as usize);
        for _ in 0..length {
            items.push(self.tag(kind, depth + 1)?);
        }
        Ok(Tag::List(items))
    }

    fn tag(&mut self, kind: u8, depth: u32) -> Result<Tag, String> {
        if depth > MAX_DEPTH {
            return Err("an NBT tag nests too deeply".into());
        }
        Ok(match kind {
            1 => Tag::Byte(self.byte()? as i8),
            2 => Tag::Short(self.short()?),
            3 => Tag::Int(self.int()?),
            4 => Tag::Long(self.long()?),
            5 => Tag::Float(f32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            6 => Tag::Double(f64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            7 => {
                let length = self.int()?;
                if length < 0 {
                    return Err("a negative byte array".into());
                }
                Tag::ByteArray(self.take(length as usize)?.to_vec())
            }
            8 => Tag::String(self.string()?),
            9 => self.list(depth)?,
            10 => self.compound(depth)?,
            11 => {
                let length = self.int()?;
                if length < 0 {
                    return Err("a negative int array".into());
                }
                let mut values = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    values.push(self.int()?);
                }
                Tag::IntArray(values)
            }
            12 => {
                let length = self.int()?;
                if length < 0 {
                    return Err("a negative long array".into());
                }
                let mut values = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    values.push(self.long()?);
                }
                Tag::LongArray(values)
            }
            other => return Err(format!("tag type {other} is not one this reader knows")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_names_parse_the_way_the_format_writes_them() {
        assert_eq!(parse_region_name("r.0.0.mca"), Some((0, 0)));
        assert_eq!(parse_region_name("r.-3.12.mca"), Some((-3, 12)));
        assert_eq!(parse_region_name("r.0.0.mcc"), None);
        assert_eq!(parse_region_name("region.mca"), None);
        assert_eq!(region_name(-3, 12), "r.-3.12.mca");
        assert_eq!(external_name(ChunkPos::new(5, -2)), "c.5.-2.mcc");
    }

    #[test]
    fn a_chunks_entry_is_indexed_column_first() {
        assert_eq!(RegionFile::index(ChunkPos::new(0, 0)), 0);
        assert_eq!(RegionFile::index(ChunkPos::new(1, 0)), 1);
        assert_eq!(RegionFile::index(ChunkPos::new(0, 1)), 32);
        assert_eq!(RegionFile::index(ChunkPos::new(-1, -1)), 1023);
    }

    #[test]
    fn compression_bytes_map_as_the_format_says() {
        assert_eq!(Compression::from_byte(1), Compression::Gzip);
        assert_eq!(Compression::from_byte(2), Compression::Zlib);
        assert_eq!(Compression::from_byte(3), Compression::None);
        assert_eq!(Compression::from_byte(4), Compression::Lz4);
        assert_eq!(Compression::from_byte(130), Compression::External(2));
    }

    #[test]
    fn a_status_knows_what_is_full() {
        assert!(ChunkStatus("minecraft:full".into()).is_full());
        assert!(ChunkStatus("full".into()).is_full());
        assert!(ChunkStatus("minecraft:empty".into()).is_empty());
        assert!(!ChunkStatus("minecraft:features".into()).is_full());
    }

    /// A chunk's saved data, as the game writes it: a root compound holding
    /// the tags a pre-generator reads.
    fn chunk_nbt_bytes() -> Vec<u8> {
        let mut out = vec![10u8, 0, 0]; // compound, empty name
        let string_tag = |out: &mut Vec<u8>, name: &str, value: &str| {
            out.push(8);
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&(value.len() as u16).to_be_bytes());
            out.extend_from_slice(value.as_bytes());
        };
        string_tag(&mut out, "Status", "minecraft:full");
        out.push(3); // Int xPos
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(b"xPos");
        out.extend_from_slice(&0i32.to_be_bytes());
        out.push(3);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(b"zPos");
        out.extend_from_slice(&0i32.to_be_bytes());
        out.push(1); // Byte isLightOn
        out.extend_from_slice(&9u16.to_be_bytes());
        out.extend_from_slice(b"isLightOn");
        out.push(1);
        out.push(4); // Long LastUpdate
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(b"LastUpdate");
        out.extend_from_slice(&7i64.to_be_bytes());
        out.push(4); // Long InhabitedTime
        out.extend_from_slice(&13u16.to_be_bytes());
        out.extend_from_slice(b"InhabitedTime");
        out.extend_from_slice(&24000i64.to_be_bytes());
        out.push(0); // end of the root
        out
    }

    #[test]
    fn a_chunks_tags_are_read_back() {
        let bytes = chunk_nbt_bytes();
        let tag = Tag::parse(&bytes).unwrap();
        assert_eq!(
            tag.get("Status").and_then(Tag::as_str),
            Some("minecraft:full")
        );
        assert_eq!(tag.get("InhabitedTime").and_then(Tag::as_long), Some(24000));
        assert_eq!(tag.get("xPos").and_then(Tag::as_long), Some(0));
        assert!(tag.get("isLightOn").is_some());
    }

    #[test]
    fn a_truncated_chunk_is_refused_not_guessed_at() {
        let bytes = chunk_nbt_bytes();
        assert!(Tag::parse(&bytes[..bytes.len() / 2]).is_err());
        assert!(Tag::parse(&[10, 0, 0]).is_err());
    }

    #[test]
    fn a_zlib_payload_round_trips() {
        let data = b"a chunk's worth of bytes".repeat(20);
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data).unwrap();
        let compressed = encoder.finish().unwrap();
        assert_eq!(decode(Compression::Zlib, &compressed).unwrap(), data);
        assert_eq!(decode(Compression::None, &data).unwrap(), data);
    }
}
