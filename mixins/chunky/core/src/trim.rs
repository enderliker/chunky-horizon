//! `/chunky trim`: deleting the chunks outside (or inside) a selection.
//!
//! The port of Chunky's `TrimCommand` and `trim/`. The work is done straight on
//! the region files, as Chunky does it: the 4-byte location entry of a chunk
//! that goes becomes zero, and nothing else in the file moves, so the deletion
//! is cheap and a region whose chunks all go is removed whole. `poi/` and
//! `entities/` region files of the same name are trimmed with the terrain's, so
//! a deleted chunk does not leave a point-of-interest or entity record behind.
//!
//! Two filters narrow what counts as deletable, and they are the reason the
//! command asks for a confirmation: `inhabited=` looks at the ticks players
//! have spent in a chunk and `unmodified=` at when the chunk was last written.

use std::path::{Path, PathBuf};

use crate::anvil::{self, RegionFile};
use crate::geometry::ChunkPos;
use crate::shape::{Bounding, is_bounding};

/// The sub-folder a world keeps its region files in.
pub const REGION_DIR: &str = "region";
/// And its point-of-interest and entity records.
pub const POI_DIR: &str = "poi";
pub const ENTITIES_DIR: &str = "entities";

/// The filter names `TrimFilters.KEYS` lists, for the error message.
pub const FILTER_KEYS: [&str; 2] = ["inhabited", "unmodified"];

/// How many chunks a whole region file holds, which is what deleting one at a
/// time counts as.
pub const CHUNKS_PER_REGION: u64 = 1024;

const RANGE: &str = "..";
const TICKS_PER_SECOND: i64 = 20;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// A time unit a filter value may end in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl Unit {
    fn seconds(self) -> i64 {
        match self {
            Unit::Seconds => 1,
            Unit::Minutes => SECONDS_PER_MINUTE,
            Unit::Hours => SECONDS_PER_HOUR,
            Unit::Days => SECONDS_PER_DAY,
        }
    }

    /// The `time_...` key this unit's notice uses.
    pub fn key(self) -> &'static str {
        match self {
            Unit::Seconds => "seconds",
            Unit::Minutes => "minutes",
            Unit::Hours => "hours",
            Unit::Days => "days",
        }
    }
}

/// One of the two filters, parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimFilter {
    /// Keeps chunks whose inhabited time falls between the two, in ticks.
    Inhabited { min_ticks: i64, max_ticks: i64 },
    /// Keeps chunks last written at or before the cutoff, in Unix seconds.
    Unmodified {
        amount: i64,
        unit: Unit,
        cutoff: i64,
    },
}

impl TrimFilter {
    /// Whether a chunk passes, which is what makes it deletable. The filters
    /// are read with `allMatch`, so a chunk must pass every one of them.
    pub fn test(&self, chunk: &TrimChunk) -> bool {
        match *self {
            TrimFilter::Inhabited {
                min_ticks,
                max_ticks,
            } => chunk.inhabited_time >= min_ticks && chunk.inhabited_time <= max_ticks,
            TrimFilter::Unmodified { cutoff, .. } => {
                if !chunk.exists {
                    return true;
                }
                chunk.last_modified != 0 && chunk.last_modified <= cutoff
            }
        }
    }

    /// Whether the filter has to read each chunk's data, not just the header.
    pub fn reads_chunk_data(&self) -> bool {
        matches!(self, TrimFilter::Inhabited { .. })
    }

    /// The notice the command sends before asking for a confirmation.
    pub fn notice(&self) -> crate::messages::Message {
        match *self {
            TrimFilter::Inhabited {
                min_ticks,
                max_ticks,
            } => {
                if min_ticks <= 0 {
                    crate::messages::trim_confirm_inhabited(max_ticks)
                } else if max_ticks == i64::MAX {
                    crate::messages::trim_confirm_inhabited_minimum(min_ticks)
                } else {
                    crate::messages::trim_confirm_inhabited_range(min_ticks, max_ticks)
                }
            }
            TrimFilter::Unmodified { amount, unit, .. } => {
                crate::messages::trim_confirm_unmodified(amount as f64, unit.key())
            }
        }
    }
}

/// The three region files of one region, as they exist.
#[derive(Clone, Debug, Default)]
pub struct RegionPaths {
    pub region: PathBuf,
    pub poi: Option<PathBuf>,
    pub entities: Option<PathBuf>,
}

impl RegionPaths {
    /// The region files of `world_root`'s region `name` (`r.0.0.mca`).
    pub fn of(world_root: &Path, name: &str) -> Self {
        let region = world_root.join(REGION_DIR).join(name);
        // Chunky only touches the siblings that exist and are at least one
        // sector long; a shorter file is not a region file.
        let sibling = |dir: &str| {
            let path = world_root.join(dir).join(name);
            (path.metadata().map(|m| m.len()).unwrap_or(0) >= anvil::SECTOR_SIZE as u64)
                .then_some(path)
        };
        Self {
            region,
            poi: sibling(POI_DIR),
            entities: sibling(ENTITIES_DIR),
        }
    }
}

/// What is known about one chunk when the filters are asked about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrimChunk {
    /// Ticks players have spent in it, 0 when it says nothing.
    pub inhabited_time: i64,
    /// Whether any of the three files holds a chunk there.
    pub exists: bool,
    /// When it was last written, in Unix seconds; 0 when unknown.
    pub last_modified: i64,
}

/// Reads the three files of a region to answer the filters.
pub struct TrimRegion {
    region: RegionFile,
    poi: Option<RegionFile>,
    entities: Option<RegionFile>,
}

impl TrimRegion {
    /// Opens a region's files. `reads_inhabited` opens the terrain file for
    /// chunk data as well as its header.
    pub fn open(
        paths: &RegionPaths,
        reads_inhabited: bool,
    ) -> Result<Option<Self>, anvil::AnvilError> {
        if paths.region.metadata().map(|m| m.len()).unwrap_or(0) < anvil::SECTOR_SIZE as u64 {
            return Ok(None);
        }
        let region = RegionFile::open(&paths.region)?;
        let poi = paths.poi.as_ref().and_then(|p| RegionFile::open(p).ok());
        let entities = paths
            .entities
            .as_ref()
            .and_then(|p| RegionFile::open(p).ok());
        // A header that could not be read at all means the filters cannot say
        // what is in the region, and Chunky leaves it alone.
        if region.is_empty() {
            return Ok(None);
        }
        let _ = reads_inhabited;
        Ok(Some(Self {
            region,
            poi,
            entities,
        }))
    }

    /// What the filters are told about a chunk.
    pub fn chunk(&mut self, chunk: ChunkPos) -> TrimChunk {
        let inhabited_time = if self.region.has_chunk(chunk) {
            self.region.inhabited_time(chunk)
        } else {
            0
        };
        let mut exists = false;
        let mut unknown = false;
        let mut last_modified = 0u32;
        for file in [
            Some(&self.region),
            self.poi.as_ref(),
            self.entities.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !file.has_chunk(chunk) {
                continue;
            }
            exists = true;
            let timestamp = file.timestamp(chunk);
            if timestamp == 0 {
                unknown = true;
            }
            last_modified = last_modified.max(timestamp);
        }
        TrimChunk {
            inhabited_time,
            exists,
            // An unknown time is reported as 0, which no cutoff accepts.
            last_modified: if unknown { 0 } else { last_modified as i64 },
        }
    }
}

/// Parses the filter tokens of `/chunky trim ...`, as `TrimFilters.parse`.
///
/// A token without an `=` is an inhabited-time maximum, and only as the first
/// token. A filter named twice, or one that does not parse, is the error.
pub fn parse_filters(tokens: &[String], now_seconds: i64) -> Result<Vec<TrimFilter>, String> {
    let mut filters: Vec<TrimFilter> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let (key, filter) = match token.find('=') {
            None => {
                let filter = if index == 0 {
                    parse_suffixed_int(token).map(|max_ticks| TrimFilter::Inhabited {
                        min_ticks: 0,
                        max_ticks,
                    })
                } else {
                    None
                };
                ("inhabited".to_string(), filter)
            }
            Some(separator) => {
                let key = token[..separator].to_ascii_lowercase();
                let value = &token[separator + 1..];
                let filter = match key.as_str() {
                    "inhabited" => parse_inhabited(value),
                    "unmodified" => parse_unmodified(value, now_seconds),
                    _ => None,
                };
                (key, filter)
            }
        };
        let Some(filter) = filter else {
            return Err(token.clone());
        };
        if seen.contains(&key) {
            return Err(token.clone());
        }
        seen.push(key);
        filters.push(filter);
    }
    Ok(filters)
}

/// `inhabited=<ticks>`, `inhabited=<max>` or `inhabited=<min>..<max>`.
fn parse_inhabited(value: &str) -> Option<TrimFilter> {
    let Some(range) = value.find(RANGE) else {
        return parse_ticks(value).map(|max_ticks| TrimFilter::Inhabited {
            min_ticks: 0,
            max_ticks,
        });
    };
    let min = &value[..range];
    let max = &value[range + RANGE.len()..];
    if min.is_empty() && max.is_empty() {
        return None;
    }
    let min_ticks = if min.is_empty() { 0 } else { parse_ticks(min)? };
    let max_ticks = if max.is_empty() {
        i64::MAX
    } else {
        parse_ticks(max)?
    };
    if min_ticks > max_ticks {
        return None;
    }
    Some(TrimFilter::Inhabited {
        min_ticks,
        max_ticks,
    })
}

/// `unmodified=<amount><unit>`, the unit required.
fn parse_unmodified(value: &str, now_seconds: i64) -> Option<TrimFilter> {
    let unit = time_unit(value)?;
    let amount = parse_amount(&value[..value.len() - 1])?;
    let cutoff = now_seconds - (amount.round() as i64) * unit.seconds();
    Some(TrimFilter::Unmodified {
        amount: amount.round() as i64,
        unit,
        cutoff,
    })
}

/// A tick count, with an optional `s`/`m`/`h`/`d` unit on the end.
fn parse_ticks(value: &str) -> Option<i64> {
    match time_unit(value) {
        Some(unit) => {
            let amount = parse_amount(&value[..value.len() - 1])?;
            Some((amount * (unit.seconds() * TICKS_PER_SECOND) as f64).round() as i64)
        }
        None => parse_amount(value).map(|amount| amount.round() as i64),
    }
}

fn time_unit(value: &str) -> Option<Unit> {
    match value.chars().last()?.to_ascii_lowercase() {
        's' => Some(Unit::Seconds),
        'm' => Some(Unit::Minutes),
        'h' => Some(Unit::Hours),
        'd' => Some(Unit::Days),
        _ => None,
    }
}

/// A non-negative finite number whose last character is a digit.
fn parse_amount(amount: &str) -> Option<f64> {
    if amount.is_empty() || !amount.chars().last()?.is_ascii_digit() {
        return None;
    }
    let number: f64 = amount.parse().ok()?;
    (number.is_finite() && number >= 0.0).then_some(number)
}

/// An integer with an optional `c`(hunks), `r`(egions) or `k` suffix.
fn parse_suffixed_int(value: &str) -> Option<i64> {
    let (digits, multiplier) = match value.chars().last()?.to_ascii_lowercase() {
        'c' => (&value[..value.len() - 1], 16i64),
        'r' => (&value[..value.len() - 1], 512),
        'k' => (&value[..value.len() - 1], 1000),
        _ => (value, 1),
    };
    let number: f64 = digits.parse().ok()?;
    number
        .is_finite()
        .then_some((number * multiplier as f64) as i64)
}

/// The shape snapped to whole chunks, which is what a trim tests against.
pub fn bounding(center_x: f64, center_z: f64, radius_x: f64, radius_z: f64) -> Bounding {
    Bounding::chunk_aligned(&crate::geometry::SelectionMath::new(
        center_x, center_z, radius_x, radius_z,
    ))
}

/// Whether a whole region file goes: every one of its 1024 chunks is on the
/// same side of the shape as `inside` asks for.
pub fn whole_region_goes(
    shape: crate::shape::ShapeKind,
    bounds: &Bounding,
    inside: bool,
    region: (i32, i32),
) -> bool {
    let min_chunk_x = region.0 << 5;
    let min_chunk_z = region.1 << 5;
    for offset_x in 0..32 {
        for offset_z in 0..32 {
            let center_x = ((min_chunk_x + offset_x) << 4) + 8;
            let center_z = ((min_chunk_z + offset_z) << 4) + 8;
            if inside != is_bounding(shape, bounds, center_x as f64, center_z as f64) {
                return false;
            }
        }
    }
    true
}

/// What a trim of one region did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trimmed {
    /// Chunks whose entry was zeroed or which went with a deleted region.
    pub deleted: u64,
    /// Whether the region file was removed whole.
    pub region_deleted: bool,
}

/// Trims one region file, as `TrimCommand.trimRegion` does.
///
/// The region is named by its coordinate, so its file name and the range of
/// chunks in it are both known. `now_seconds` is only used by the filters'
/// notices, not here; the filters carry their own cutoffs.
pub fn trim_region(
    paths: &RegionPaths,
    shape: crate::shape::ShapeKind,
    bounds: &Bounding,
    inside: bool,
    filters: &[TrimFilter],
) -> Result<Trimmed, anvil::AnvilError> {
    let Some((region_x, region_z)) = region_of(&paths.region) else {
        return Ok(Trimmed::default());
    };
    if paths.region.metadata().map(|m| m.len()).unwrap_or(0) < anvil::SECTOR_SIZE as u64 {
        return Ok(Trimmed::default());
    }

    if filters.is_empty() && whole_region_goes(shape, bounds, inside, (region_x, region_z)) {
        let mut deleted = CHUNKS_PER_REGION;
        for path in [
            Some(&paths.region),
            paths.poi.as_ref(),
            paths.entities.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if std::fs::remove_file(path).is_err() {
                deleted = 0;
            }
        }
        return Ok(Trimmed {
            deleted: if deleted == 0 { 0 } else { CHUNKS_PER_REGION },
            region_deleted: true,
        });
    }

    let filtered = !filters.is_empty();
    let mut filter_region = if filtered {
        TrimRegion::open(paths, filters.iter().any(TrimFilter::reads_chunk_data))?
    } else {
        None
    };
    if filtered && filter_region.is_none() {
        return Ok(Trimmed::default());
    }

    let mut region = RegionFile::open(&paths.region)?;
    let mut poi = paths.poi.as_ref().and_then(|p| RegionFile::open(p).ok());
    let mut entities = paths
        .entities
        .as_ref()
        .and_then(|p| RegionFile::open(p).ok());

    let min_chunk_x = region_x << 5;
    let min_chunk_z = region_z << 5;
    let mut marked = 0u64;
    let mut deleted = 0u64;
    for offset_x in 0..32 {
        for offset_z in 0..32 {
            let chunk = ChunkPos::new(min_chunk_x + offset_x, min_chunk_z + offset_z);
            let center_x = (chunk.x << 4) + 8;
            let center_z = (chunk.z << 4) + 8;
            let in_shape = is_bounding(shape, bounds, center_x as f64, center_z as f64);
            let mut goes = if inside { in_shape } else { !in_shape };
            if goes && filtered {
                let Some(filter_region) = filter_region.as_mut() else {
                    continue;
                };
                let about = filter_region.chunk(chunk);
                goes = filters.iter().all(|filter| filter.test(&about));
            }
            if !goes {
                continue;
            }
            marked += 1;
            if region.clear_chunk(chunk)? {
                deleted += 1;
            }
            if let Some(poi) = poi.as_mut() {
                poi.clear_chunk(chunk)?;
            }
            if let Some(entities) = entities.as_mut() {
                entities.clear_chunk(chunk)?;
            }
        }
    }

    // A filtered region whose every chunk went is removed whole, which is the
    // only way it can be: the filter's answer for each chunk is not saved.
    if filtered && marked == CHUNKS_PER_REGION {
        for path in [
            Some(&paths.region),
            paths.poi.as_ref(),
            paths.entities.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = std::fs::remove_file(path);
        }
        return Ok(Trimmed {
            deleted,
            region_deleted: true,
        });
    }

    Ok(Trimmed {
        deleted,
        region_deleted: false,
    })
}

/// The coordinate in a region file's name.
fn region_of(path: &Path) -> Option<(i32, i32)> {
    anvil::parse_region_name(path.file_name()?.to_str()?)
}

/// Every region file of a world, sorted, as the trim walks them.
pub fn regions(world_root: &Path) -> Vec<(i32, i32)> {
    anvil::region_files(&world_root.join(REGION_DIR))
}

/// Trims every region file of a world, calling `progress` after each one so the
/// caller can report and stop. Returns the chunks deleted and the regions seen.
pub fn trim_world(
    world_root: &Path,
    shape: crate::shape::ShapeKind,
    bounds: &Bounding,
    inside: bool,
    filters: &[TrimFilter],
    mut progress: impl FnMut(u64, u64, u64) -> bool,
) -> Result<(u64, u64), anvil::AnvilError> {
    let regions = regions(world_root);
    let total = regions.len() as u64;
    let mut deleted = 0u64;
    let mut finished = 0u64;
    for (x, z) in regions {
        let paths = RegionPaths::of(world_root, &anvil::region_name(x, z));
        let trimmed = trim_region(&paths, shape, bounds, inside, filters)?;
        deleted += trimmed.deleted;
        finished += 1;
        if !progress(finished, total, deleted) {
            break;
        }
    }
    Ok((deleted, finished))
}

/// The names a filter may be given by.
pub fn filter_keys() -> String {
    FILTER_KEYS.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::ShapeKind;

    fn tokens(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_lone_number_is_an_inhabited_maximum() {
        let filters = parse_filters(&tokens(&["2000"]), 0).unwrap();
        assert_eq!(
            filters,
            vec![TrimFilter::Inhabited {
                min_ticks: 0,
                max_ticks: 2000
            }]
        );
    }

    #[test]
    fn inhabited_reads_ticks_and_units() {
        assert_eq!(
            parse_filters(&tokens(&["inhabited=600"]), 0).unwrap(),
            vec![TrimFilter::Inhabited {
                min_ticks: 0,
                max_ticks: 600
            }]
        );
        // `5m` is five minutes: 300 seconds, 6000 ticks.
        assert_eq!(
            parse_filters(&tokens(&["inhabited=5m"]), 0).unwrap(),
            vec![TrimFilter::Inhabited {
                min_ticks: 0,
                max_ticks: 6000
            }]
        );
        assert_eq!(
            parse_filters(&tokens(&["inhabited=100..500"]), 0).unwrap(),
            vec![TrimFilter::Inhabited {
                min_ticks: 100,
                max_ticks: 500
            }]
        );
        assert_eq!(
            parse_filters(&tokens(&["inhabited=..500"]), 0).unwrap(),
            vec![TrimFilter::Inhabited {
                min_ticks: 0,
                max_ticks: 500
            }]
        );
        assert_eq!(
            parse_filters(&tokens(&["inhabited=100.."]), 0).unwrap(),
            vec![TrimFilter::Inhabited {
                min_ticks: 100,
                max_ticks: i64::MAX
            }]
        );
    }

    #[test]
    fn unmodified_counts_back_from_now() {
        let now = 1_000_000;
        let filters = parse_filters(&tokens(&["unmodified=7d"]), now).unwrap();
        assert_eq!(
            filters,
            vec![TrimFilter::Unmodified {
                amount: 7,
                unit: Unit::Days,
                cutoff: now - 7 * 86_400,
            }]
        );
        // The unit is required, as Chunky's parser requires it.
        assert!(parse_filters(&tokens(&["unmodified=7"]), now).is_err());
    }

    #[test]
    fn a_bad_or_repeated_filter_is_an_error() {
        assert_eq!(
            parse_filters(&tokens(&["nonsense=1"]), 0),
            Err("nonsense=1".to_string())
        );
        // A filter named twice is refused, as Chunky's map refuses it.
        assert_eq!(
            parse_filters(&tokens(&["inhabited=1", "inhabited=2"]), 0),
            Err("inhabited=2".to_string())
        );
        assert!(parse_filters(&tokens(&["inhabited=abc"]), 0).is_err());
        assert!(parse_filters(&tokens(&["inhabited=500..100"]), 0).is_err());
        assert!(parse_filters(&tokens(&["inhabited=.."]), 0).is_err());
        // A second bare token is not an inhabited maximum.
        assert!(parse_filters(&tokens(&["100", "200"]), 0).is_err());
    }

    #[test]
    fn the_filters_keep_what_chunky_keeps() {
        let inhabited = TrimFilter::Inhabited {
            min_ticks: 100,
            max_ticks: 500,
        };
        assert!(inhabited.test(&TrimChunk {
            inhabited_time: 100,
            ..Default::default()
        }));
        assert!(inhabited.test(&TrimChunk {
            inhabited_time: 500,
            ..Default::default()
        }));
        assert!(!inhabited.test(&TrimChunk {
            inhabited_time: 99,
            ..Default::default()
        }));
        assert!(!inhabited.test(&TrimChunk {
            inhabited_time: 501,
            ..Default::default()
        }));

        let unmodified = TrimFilter::Unmodified {
            amount: 7,
            unit: Unit::Days,
            cutoff: 1000,
        };
        // A chunk that is not there at all is deletable.
        assert!(unmodified.test(&TrimChunk {
            exists: false,
            ..Default::default()
        }));
        // One with no known time is not.
        assert!(!unmodified.test(&TrimChunk {
            exists: true,
            last_modified: 0,
            ..Default::default()
        }));
        assert!(unmodified.test(&TrimChunk {
            exists: true,
            last_modified: 999,
            ..Default::default()
        }));
        assert!(!unmodified.test(&TrimChunk {
            exists: true,
            last_modified: 1001,
            ..Default::default()
        }));
    }

    #[test]
    fn a_region_is_whole_only_when_every_chunk_agrees() {
        // Chunk-aligned: a radius of 8 blocks is one chunk of radius, so the
        // shape is three chunks across and its radius is 24 blocks.
        let bounds = bounding(8.0, 8.0, 8.0, 8.0);
        assert_eq!(bounds.radius_x, 24.0);

        // A selection far from region (0, 0) leaves every one of its chunks
        // outside, so the file goes whole.
        let far = bounding(100_000.0, 100_000.0, 8.0, 8.0);
        assert!(whole_region_goes(ShapeKind::Square, &far, false, (0, 0)));
        assert!(!whole_region_goes(ShapeKind::Square, &far, true, (0, 0)));

        // A square that covers region (0, 0) whole is inside it.
        let wide = bounding(256.0, 256.0, 600.0, 600.0);
        assert!(whole_region_goes(ShapeKind::Square, &wide, true, (0, 0)));
        assert!(!whole_region_goes(ShapeKind::Square, &wide, false, (0, 0)));

        // A selection at the origin straddles the region's corner, so it is
        // neither wholly inside nor wholly outside: the file is trimmed chunk
        // by chunk instead.
        assert!(!whole_region_goes(
            ShapeKind::Square,
            &bounds,
            false,
            (0, 0)
        ));
        assert!(!whole_region_goes(ShapeKind::Square, &bounds, true, (0, 0)));
    }

    #[test]
    fn suffixed_integers_scale() {
        assert_eq!(parse_suffixed_int("2"), Some(2));
        assert_eq!(parse_suffixed_int("2c"), Some(32));
        assert_eq!(parse_suffixed_int("2r"), Some(1024));
        assert_eq!(parse_suffixed_int("2k"), Some(2000));
        assert_eq!(parse_suffixed_int("nope"), None);
    }
}
