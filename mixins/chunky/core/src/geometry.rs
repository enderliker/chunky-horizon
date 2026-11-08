//! Chunk geometry and the number handling the commands read.
//!
//! This is the port of Chunky's `util/ChunkMath`, `platform/util/Vector2` and
//! `util/Input`/`Formatting`, and the chunk/region conversions `Selection`
//! derives. Every conversion keeps Java's own arithmetic, quirks included:
//! block coordinates truncate towards zero before the shift, so a negative
//! centre maps to a chunk the way `(int) centerX >> 4` does and not the way
//! `floorDiv` would.

/// One chunk is 16 blocks on a side.
pub const CHUNK_SIZE: i32 = 16;
/// One region file is 32 by 32 chunks.
pub const REGION_SIZE: i32 = 32;

/// A block coordinate past this is outside every world (`Input.isPastWorldLimit`).
pub const WORLD_LIMIT: f64 = 3.0e7;

/// The chunk a block coordinate is in: Java's `(int) block >> 4` (the cast
/// truncates towards zero, the shift is arithmetic).
pub fn chunk_of_block(block: f64) -> i32 {
    (block as i32) >> 4
}

/// A chunk coordinate from an integer block coordinate, as `/forceload` reads it.
pub fn chunk_of_block_i32(block: i32) -> i32 {
    block >> 4
}

/// The block coordinate of a chunk's centre (`(chunk << 4) + 8`).
pub fn chunk_center(chunk: i32) -> i32 {
    (chunk << 4) + 8
}

/// The region file a chunk belongs to.
pub fn region_of_chunk(chunk: i32) -> i32 {
    chunk >> 5
}

/// Whether `value` is past the world's edge, as `Input.isPastWorldLimit`.
pub fn is_past_world_limit(value: f64) -> bool {
    value.abs() > WORLD_LIMIT
}

/// A chunk position: chunk coordinates, not blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// The chunk's centre in blocks, which every shape test is given.
    pub fn center(self) -> (f64, f64) {
        (chunk_center(self.x) as f64, chunk_center(self.z) as f64)
    }

    /// The region file this chunk is in.
    pub fn region(self) -> (i32, i32) {
        (region_of_chunk(self.x), region_of_chunk(self.z))
    }

    /// The offset inside its region file, 0 to 31 on each axis.
    pub fn offset_in_region(self) -> (i32, i32) {
        (self.x & 31, self.z & 31)
    }
}

/// What a selection's block coordinates come to in chunks and in region
/// files, which every iterator and every shape is built on (`Selection`'s
/// derived values).
///
/// The radii are *ceilings*: a radius of 500 blocks reaches into the 32nd
/// chunk on each side, so the selection is 65 chunks wide, not 63. That
/// rounds outward, which is Chunky's choice and the reason a task's stated
/// radius and the chunks it covers never disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionMath {
    pub center_chunk_x: i32,
    pub center_chunk_z: i32,
    pub radius_chunks_x: i32,
    pub radius_chunks_z: i32,
    pub diameter_chunks_x: i32,
    pub diameter_chunks_z: i32,
    pub center_region_x: i32,
    pub center_region_z: i32,
    pub radius_regions_x: i32,
    pub radius_regions_z: i32,
    pub diameter_regions_x: i32,
    pub diameter_regions_z: i32,
}

impl SelectionMath {
    pub fn new(center_x: f64, center_z: f64, radius_x: f64, radius_z: f64) -> Self {
        let center_chunk_x = chunk_of_block(center_x);
        let center_chunk_z = chunk_of_block(center_z);
        let radius_chunks_x = (radius_x / CHUNK_SIZE as f64).ceil() as i32;
        let radius_chunks_z = (radius_z / CHUNK_SIZE as f64).ceil() as i32;
        let center_region_x = region_of_chunk(center_chunk_x);
        let center_region_z = region_of_chunk(center_chunk_z);
        let radius_regions_x = (radius_chunks_x as f64 / REGION_SIZE as f64).ceil() as i32;
        let radius_regions_z = (radius_chunks_z as f64 / REGION_SIZE as f64).ceil() as i32;
        Self {
            center_chunk_x,
            center_chunk_z,
            radius_chunks_x,
            radius_chunks_z,
            diameter_chunks_x: 2 * radius_chunks_x + 1,
            diameter_chunks_z: 2 * radius_chunks_z + 1,
            center_region_x,
            center_region_z,
            radius_regions_x,
            radius_regions_z,
            diameter_regions_x: 2 * radius_regions_x + 1,
            diameter_regions_z: 2 * radius_regions_z + 1,
        }
    }

    /// Every chunk the selection's bounding box holds, corners included.
    pub fn chunk_region(&self) -> Region {
        Region::new(
            self.center_chunk_x - self.radius_chunks_x,
            self.center_chunk_z - self.radius_chunks_z,
            self.center_chunk_x + self.radius_chunks_x,
            self.center_chunk_z + self.radius_chunks_z,
        )
    }

    /// The region files the bounding box reaches into.
    pub fn region_region(&self) -> Region {
        Region::new(
            self.center_region_x - self.radius_regions_x,
            self.center_region_z - self.radius_regions_z,
            self.center_region_x + self.radius_regions_x,
            self.center_region_z + self.radius_regions_z,
        )
    }
}

/// A rectangle of chunks, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub min_x: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_z: i32,
}

impl Region {
    pub const fn new(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> Self {
        Self {
            min_x,
            min_z,
            max_x,
            max_z,
        }
    }

    /// The chunks of the region, in an order of the caller's choosing.
    pub fn chunks(self) -> impl Iterator<Item = ChunkPos> {
        (self.min_x..=self.max_x)
            .flat_map(move |x| (self.min_z..=self.max_z).map(move |z| ChunkPos::new(x, z)))
    }

    pub fn contains(self, chunk: ChunkPos) -> bool {
        chunk.x >= self.min_x
            && chunk.x <= self.max_x
            && chunk.z >= self.min_z
            && chunk.z <= self.max_z
    }

    /// How many chunks the region holds, negative sides included.
    pub fn count(self) -> u64 {
        let width = (self.max_x as i64 - self.min_x as i64 + 1).max(0) as u64;
        let depth = (self.max_z as i64 - self.min_z as i64 + 1).max(0) as u64;
        width * depth
    }
}

/// A number written the way the commands accept it: an optional sign, then a
/// plain number, optionally with a `c` (chunks, 16), `r` (regions, 512) or
/// `k` (thousands, 1000) suffix (`Input.tryDoubleSuffixed`).
pub fn parse_suffixed(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (number, multiplier) = match text.chars().last() {
        Some('c') | Some('C') => (&text[..text.len() - 1], 16.0),
        Some('r') | Some('R') => (&text[..text.len() - 1], 512.0),
        Some('k') | Some('K') => (&text[..text.len() - 1], 1000.0),
        _ => (text, 1.0),
    };
    number.parse::<f64>().ok().map(|value| value * multiplier)
}

/// The same, as a whole number (`Input.tryInteger`), truncating a fraction as
/// Java's `Integer.parseInt` refuses it.
pub fn parse_int(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

/// An integer with the same suffixes, truncating a fraction towards zero
/// (`Input.tryIntegerSuffixed` goes through the double form).
pub fn parse_int_suffixed(text: &str) -> Option<i64> {
    let value = parse_suffixed(text)?;
    if !value.is_finite() {
        return None;
    }
    let truncated = value.trunc();
    // Java's cast to int saturates only in the other direction; keep it simple
    // and refuse what does not fit instead of wrapping.
    if truncated < i64::MIN as f64 || truncated > i64::MAX as f64 {
        None
    } else {
        Some(truncated as i64)
    }
}

/// The sign a radius argument can carry: `-` or `+`, else nothing
/// (`Input.trySign`).
pub fn parse_sign(text: &str) -> Option<i32> {
    match text.chars().next() {
        Some('-') => Some(-1),
        Some('+') => Some(1),
        _ => None,
    }
}

/// A number as the messages print it: Java's `DecimalFormat("#.##")`, so at
/// most two decimals, no trailing zeros and no thousands separator.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_owned()
        } else if value > 0.0 {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    let rounded = (value * 100.0).round() / 100.0;
    let mut text = format!("{rounded:.2}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".to_owned() } else { text }
}

/// The radius as `format_start` and friends print it: one number when both
/// axes agree, two when they differ (`Formatting.radius`).
pub fn format_radius(radius_x: f64, radius_z: f64) -> String {
    if (radius_x - radius_z).abs() < f64::EPSILON {
        format_number(radius_x)
    } else {
        format!("{}, {}", format_number(radius_x), format_number(radius_z))
    }
}

/// A `%s%%` percentage with two decimals: Java's `String.format("%.2f", ...)`,
/// which is what the progress reports pass.
pub fn format_percent(percent: f64) -> String {
    format!("{percent:.2}")
}

/// A rate with one decimal, as `String.format("%.1f", ...)` prints it.
pub fn format_rate(rate: f64) -> String {
    format!("{rate:.1}")
}

/// A duration as `h:mm:ss`, with the hours unpadded as `%01d` leaves them.
pub fn format_duration(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    format!("{hours}:{minutes:02}:{seconds:02}")
}

/// The three `%s` arguments of a duration, which the progress templates take
/// separately (`String.format("%01d", hours)` and so on).
pub fn duration_parts(seconds: u64) -> (String, String, String) {
    (
        format!("{}", seconds / 3600),
        format!("{:02}", (seconds / 60) % 60),
        format!("{:02}", seconds % 60),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_of_block_truncates_towards_zero_as_java_does() {
        assert_eq!(chunk_of_block(0.0), 0);
        assert_eq!(chunk_of_block(15.9), 0);
        assert_eq!(chunk_of_block(16.0), 1);
        assert_eq!(chunk_of_block(-0.5), 0); // (int) -0.5 == 0, not -1
        assert_eq!(chunk_of_block(-16.0), -1);
        assert_eq!(chunk_of_block(-17.0), -2);
    }

    #[test]
    fn chunks_and_regions_line_up() {
        assert_eq!(chunk_center(0), 8);
        assert_eq!(chunk_center(-1), -8);
        assert_eq!(region_of_chunk(31), 0);
        assert_eq!(region_of_chunk(32), 1);
        assert_eq!(region_of_chunk(-1), -1);
        assert_eq!(ChunkPos::new(-1, 32).offset_in_region(), (31, 0));
    }

    #[test]
    fn suffixes_multiply_the_way_chunky_reads_them() {
        assert_eq!(parse_suffixed("16c"), Some(256.0));
        assert_eq!(parse_suffixed("1r"), Some(512.0));
        assert_eq!(parse_suffixed("2k"), Some(2000.0));
        assert_eq!(parse_suffixed("1.5"), Some(1.5));
        assert_eq!(parse_suffixed("-3"), Some(-3.0));
        assert_eq!(parse_suffixed("x"), None);
        assert_eq!(parse_int_suffixed("5m"), None);
    }

    #[test]
    fn numbers_print_without_trailing_zeros() {
        assert_eq!(format_number(500.0), "500");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(1.005), "1");
        assert_eq!(format_number(0.25), "0.25");
        assert_eq!(format_radius(500.0, 500.0), "500");
        assert_eq!(format_radius(500.0, 250.0), "500, 250");
    }

    #[test]
    fn durations_and_percentages_are_two_decimals() {
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_percent(25.0), "25.00");
        assert_eq!(format_percent(0.0), "0.00");
    }

    #[test]
    fn a_region_counts_its_chunks() {
        let region = Region::new(0, 0, 1, 1);
        assert_eq!(region.count(), 4);
        assert!(region.contains(ChunkPos::new(1, 1)));
        assert!(!region.contains(ChunkPos::new(2, 1)));
    }
}
