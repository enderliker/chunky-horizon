//! The patterns a generation task walks its selection in.
//!
//! The port of Chunky's `iterator` package. A pattern turns a selection into
//! an order over chunks; the task reads that order one chunk at a time, and a
//! task that was paused resumes at the count it saved, which is why every
//! pattern has to be able to skip ahead to a position without walking there.
//!
//! Two things the port keeps from Chunky: the factory's rules, which are not
//! obvious (a rectangle or an ellipse always lays its chunks in strips, no
//! matter the pattern asked for), and the totals, which count the selection's
//! whole bounding box and not the chunks the shape actually keeps. A task's
//! percentage is therefore relative to the bounding box, exactly as Chunky
//! reports it.

use crate::geometry::{ChunkPos, REGION_SIZE, Region};
use crate::hilbert;
use crate::selection::Selection;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// How many chunks a strip or a ring is wide (`chunky.stripWidth`,
/// `chunky.ringWidth`).
pub const WIDTH: i32 = 64;

/// A pattern by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatternKind {
    Concentric,
    Loop,
    Spiral,
    Csv,
    Region,
    World,
    Strip,
    Ring,
}

/// The patterns a player can choose, in the order the messages list them.
pub const PATTERNS: [PatternKind; 8] = [
    PatternKind::Concentric,
    PatternKind::Loop,
    PatternKind::Spiral,
    PatternKind::Csv,
    PatternKind::Region,
    PatternKind::World,
    PatternKind::Strip,
    PatternKind::Ring,
];

impl PatternKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Concentric => "concentric",
            Self::Loop => "loop",
            Self::Spiral => "spiral",
            Self::Csv => "csv",
            Self::Region => "region",
            Self::World => "world",
            Self::Strip => "strip",
            Self::Ring => "ring",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "concentric" => Some(Self::Concentric),
            "loop" => Some(Self::Loop),
            "spiral" => Some(Self::Spiral),
            "csv" => Some(Self::Csv),
            "region" => Some(Self::Region),
            "world" => Some(Self::World),
            "strip" => Some(Self::Strip),
            "ring" => Some(Self::Ring),
            _ => None,
        }
    }
}

/// A pattern and the value it carries: `csv` names a file, the others none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub value: Option<String>,
}

impl Pattern {
    pub const fn new(kind: PatternKind) -> Self {
        Self { kind, value: None }
    }

    pub fn with_value(kind: PatternKind, value: Option<String>) -> Self {
        Self { kind, value }
    }

    /// The default a fresh selection starts with.
    pub const fn default_pattern() -> Self {
        Self::new(PatternKind::Ring)
    }

    /// `ring`, or `csv=file` when the pattern carries a value, which is what
    /// the task's log and events name it by.
    pub fn describe(&self) -> String {
        match &self.value {
            Some(value) => format!("{}={value}", self.kind.name()),
            None => self.kind.name().to_owned(),
        }
    }
}

/// Where a task reads its inputs: the mod's data folder (saved task files,
/// CSV patterns, the world scan's output), the mod's config folder (a CSV
/// pattern's second chance) and the world's `region` folder (the `world`
/// pattern's scan).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sources<'a> {
    pub data_dir: Option<&'a Path>,
    pub config_dir: Option<&'a Path>,
    pub region_dir: Option<&'a Path>,
}

/// Why a pattern could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    /// The `world` pattern found no region folder to scan.
    NoRegionDirectory,
    /// A `csv` pattern names a file that isn't there.
    CsvNotFound(String),
    /// A `csv` pattern names a file that could not be read.
    CsvUnreadable(String, String),
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRegionDirectory => write!(f, "the world has no region directory to scan"),
            Self::CsvNotFound(name) => {
                write!(f, "no {name}.csv in the mod's data or config folder")
            }
            Self::CsvUnreadable(name, why) => write!(f, "{name}.csv could not be read: {why}"),
        }
    }
}

impl std::error::Error for PatternError {}

/// The chunks of a selection, in the order the pattern asks for them.
///
/// `partial` is a region file whose bounds cut into the selection, which the
/// `region` pattern walks in a plain row-major order instead of the Hilbert
/// curve it uses for a region it covers whole.
pub struct Chunks {
    state: State,
    total: u64,
    name: String,
    done: u64,
}

enum State {
    Loop {
        x: i32,
        z: i32,
        x1: i32,
        z1: i32,
        x2: i32,
        z2: i32,
        exhausted: bool,
    },
    Strip {
        current: u64,
        min_x: i32,
        min_z: i32,
        size_x: i32,
        size_z: i32,
    },
    Concentric {
        x: i32,
        z: i32,
        center_x: i32,
        center_z: i32,
        radius: i32,
        annulus: i32,
        span: i32,
        down: i32,
        left: i32,
        up: i32,
        right: i32,
        exhausted: bool,
    },
    Spiral {
        x: i32,
        z: i32,
        stop_x: i32,
        stop_z: i32,
        span: i32,
        span_count: i32,
        span_progress: i32,
        direction: u8,
        exhausted: bool,
    },
    Ring {
        center_x: i32,
        center_z: i32,
        radius: i32,
        ring: i32,
        inner: i32,
        outer: i32,
        current: u64,
        total_ring: u64,
        started: bool,
        exhausted: bool,
    },
    Region {
        regions: Box<dyn Iterator<Item = (i32, i32)> + Send>,
        current: Option<RegionChunks>,
        bounds: Region,
        complete: u64,
    },
    Queue {
        chunks: VecDeque<ChunkPos>,
    },
}

/// One region file's worth of chunks, in the order that region is walked.
struct RegionChunks {
    min_x: i32,
    min_z: i32,
    size_z: i32,
    full: bool,
    current: u64,
    total: u64,
}

impl RegionChunks {
    fn new(region: (i32, i32), bounds: Region) -> Self {
        let low_x = region.0 << 5;
        let low_z = region.1 << 5;
        let high_x = low_x + REGION_SIZE - 1;
        let high_z = low_z + REGION_SIZE - 1;
        let min_x = low_x.max(bounds.min_x);
        let min_z = low_z.max(bounds.min_z);
        let max_x = high_x.min(bounds.max_x);
        let max_z = high_z.min(bounds.max_z);
        if min_x > high_x || min_z > high_z || max_x < low_x || max_z < low_z {
            return Self {
                min_x,
                min_z,
                size_z: 0,
                full: false,
                current: 0,
                total: 0,
            };
        }
        let size_x = max_x - min_x + 1;
        let size_z = max_z - min_z + 1;
        let total = (size_x as u64) * (size_z as u64);
        Self {
            min_x,
            min_z,
            size_z,
            full: total == 1024,
            current: 0,
            total,
        }
    }

    fn resume(&mut self, count: u64) {
        self.current = count.min(self.total);
    }

    fn next(&mut self) -> Option<ChunkPos> {
        if self.current >= self.total {
            return None;
        }
        let chunk = if self.full {
            let offset = hilbert::offset(self.current as u32);
            ChunkPos::new(self.min_x + offset.x, self.min_z + offset.z)
        } else {
            let offset_x = (self.current / self.size_z as u64) as i32;
            let offset_z = (self.current % self.size_z as u64) as i32;
            ChunkPos::new(self.min_x + offset_x, self.min_z + offset_z)
        };
        self.current += 1;
        Some(chunk)
    }
}

impl Chunks {
    /// The chunks of `selection` in its pattern's order, having already
    /// produced `count` of them (which is how a paused task resumes).
    ///
    /// A `world` pattern reads the region files while it is built, so the
    /// caller is told how far it has got through `scan` and may stop it by
    /// returning false.
    pub fn new(
        selection: &Selection,
        count: u64,
        sources: Sources<'_>,
        mut scan: impl FnMut(u64, u64) -> bool,
    ) -> Result<Self, PatternError> {
        let math = selection.math();
        let shape = selection.shape;
        let kind = match selection.pattern.kind {
            PatternKind::World => PatternKind::World,
            _ if matches!(
                shape,
                crate::shape::ShapeKind::Rectangle | crate::shape::ShapeKind::Ellipse
            ) =>
            {
                match selection.pattern.kind {
                    PatternKind::Loop => PatternKind::Loop,
                    _ => PatternKind::Strip,
                }
            }
            other => other,
        };
        let bounds = math.chunk_region();
        let name = selection.pattern.describe();
        let mut chunks = match kind {
            PatternKind::Loop => {
                let (x, z) = if count == 0 {
                    (bounds.min_x, bounds.min_z)
                } else {
                    let stride = math.diameter_chunks_z as u64;
                    (
                        bounds.min_x + (count / stride) as i32,
                        bounds.min_z + (count % stride) as i32,
                    )
                };
                let exhausted = x > bounds.max_x;
                Self {
                    state: State::Loop {
                        x,
                        z,
                        x1: bounds.min_x,
                        z1: bounds.min_z,
                        x2: bounds.max_x,
                        z2: bounds.max_z,
                        exhausted,
                    },
                    total: (math.diameter_chunks_x as u64) * (math.diameter_chunks_z as u64),
                    name,
                    done: count,
                }
            }
            PatternKind::Strip => {
                let use_z = matches!(
                    shape,
                    crate::shape::ShapeKind::Rectangle | crate::shape::ShapeKind::Ellipse
                );
                let size_z = if use_z {
                    math.diameter_chunks_z
                } else {
                    math.diameter_chunks_x
                };
                let min_z = if use_z {
                    math.center_chunk_z - math.radius_chunks_z
                } else {
                    math.center_chunk_z - math.radius_chunks_x
                };
                let total = (math.diameter_chunks_x as u64) * (size_z as u64);
                Self {
                    state: State::Strip {
                        current: count.min(total),
                        min_x: math.center_chunk_x - math.radius_chunks_x,
                        min_z,
                        size_x: math.diameter_chunks_x,
                        size_z,
                    },
                    total,
                    name,
                    done: count,
                }
            }
            PatternKind::Concentric => {
                let radius = math.radius_chunks_x;
                let center_x = math.center_chunk_x;
                let center_z = math.center_chunk_z;
                let total = (math.diameter_chunks_x as u64) * (math.diameter_chunks_x as u64);
                let mut state = State::Concentric {
                    x: center_x,
                    z: center_z,
                    center_x,
                    center_z,
                    radius,
                    annulus: 0,
                    span: 0,
                    down: 0,
                    left: 0,
                    up: 0,
                    right: 0,
                    exhausted: false,
                };
                if count > 0 {
                    concentric_resume(&mut state, count, radius);
                }
                Self {
                    state,
                    total,
                    name,
                    done: count,
                }
            }
            PatternKind::Spiral => {
                let radius = math.radius_chunks_x;
                let center_x = math.center_chunk_x;
                let center_z = math.center_chunk_z;
                let total = (math.diameter_chunks_x as u64) * (math.diameter_chunks_x as u64);
                let mut state = State::Spiral {
                    x: center_x,
                    z: center_z,
                    stop_x: center_x + radius,
                    stop_z: center_z + radius,
                    span: 1,
                    span_count: 0,
                    span_progress: 0,
                    direction: 0,
                    exhausted: false,
                };
                if count > 0 {
                    spiral_resume(&mut state, count, radius);
                }
                Self {
                    state,
                    total,
                    name,
                    done: count,
                }
            }
            PatternKind::Ring => {
                let radius = math.radius_chunks_x;
                let mut state = State::Ring {
                    center_x: math.center_chunk_x,
                    center_z: math.center_chunk_z,
                    radius,
                    ring: -1,
                    inner: -1,
                    outer: 0,
                    current: 0,
                    total_ring: 0,
                    started: false,
                    exhausted: false,
                };
                let total = (math.diameter_chunks_x as u64) * (math.diameter_chunks_x as u64);
                ring_advance(&mut state, count.min(total));
                Self {
                    state,
                    total,
                    name,
                    done: count,
                }
            }
            PatternKind::Region => {
                let region_bounds = Region::new(
                    bounds.min_x >> 5,
                    bounds.min_z >> 5,
                    bounds.max_x >> 5,
                    bounds.max_z >> 5,
                );
                let regions = region_order(
                    math.center_region_x,
                    math.center_region_z,
                    math.radius_regions_x,
                )
                .filter(move |r| {
                    r.0 >= region_bounds.min_x
                        && r.0 <= region_bounds.max_x
                        && r.1 >= region_bounds.min_z
                        && r.1 <= region_bounds.max_z
                })
                .collect::<Vec<_>>();
                let bounds_x = Region::new(
                    math.center_chunk_x - math.radius_chunks_x,
                    math.center_chunk_z - math.radius_chunks_x,
                    math.center_chunk_x + math.radius_chunks_x,
                    math.center_chunk_z + math.radius_chunks_x,
                );
                let total = (math.diameter_chunks_x as u64) * (math.diameter_chunks_x as u64);
                let mut chunks = Self {
                    state: State::Region {
                        regions: Box::new(regions.into_iter()),
                        current: None,
                        bounds: bounds_x,
                        complete: 0,
                    },
                    total,
                    name,
                    done: count,
                };
                chunks.region_advance(count);
                chunks
            }
            PatternKind::Csv => {
                let value = selection.pattern.value.clone().unwrap_or_default();
                let chunks = read_csv(&value, sources)?;
                let total = chunks.len() as u64;
                let mut queue: VecDeque<ChunkPos> = chunks.into();
                for _ in 0..count.min(total) {
                    queue.pop_front();
                }
                Self {
                    state: State::Queue { chunks: queue },
                    total,
                    name: format!("csv={value}"),
                    done: count,
                }
            }
            PatternKind::World => {
                let value = selection.pattern.value.clone().unwrap_or_default();
                let chunks = scan_world(selection, sources, &mut scan)?;
                let total = chunks.len() as u64;
                let mut queue: VecDeque<ChunkPos> = chunks.into();
                for _ in 0..count.min(total) {
                    queue.pop_front();
                }
                Self {
                    state: State::Queue { chunks: queue },
                    total,
                    name: format!("csv={value}"),
                    done: count,
                }
            }
        };
        chunks.done = count;
        Ok(chunks)
    }

    /// How many chunks the pattern will produce in all.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// What the task's log and events call this walk.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many chunks it has produced.
    pub fn produced(&self) -> u64 {
        self.done
    }

    #[allow(clippy::should_implement_trait)] // Chunky's own `next`, not `Iterator`'s
    pub fn next(&mut self) -> Option<ChunkPos> {
        let chunk = self.step();
        if chunk.is_some() {
            self.done += 1;
        }
        chunk
    }

    fn step(&mut self) -> Option<ChunkPos> {
        // Read before the state is borrowed mutably: the walk asks for this
        // while it is inside a `State::Concentric` arm.
        let concentric_radius = self.concentric_radius();
        match &mut self.state {
            State::Loop {
                x,
                z,
                x1,
                z1,
                x2,
                z2,
                exhausted,
            } => {
                if *exhausted {
                    return None;
                }
                let chunk = ChunkPos::new(*x, *z);
                *z += 1;
                if *z > *z2 {
                    *z = *z1;
                    *x += 1;
                    if *x > *x2 {
                        *exhausted = true;
                    }
                }
                let _ = (x1, z1);
                Some(chunk)
            }
            State::Strip {
                current,
                min_x,
                min_z,
                size_x,
                size_z,
            } => {
                if *current >= self.total {
                    return None;
                }
                let strip_size = (WIDTH as u64) * (*size_z as u64);
                let strip_x = *current / strip_size;
                let offset = *current % strip_size;
                let current_width = WIDTH.min(*size_x - strip_x as i32);
                let value = *current;
                *current += 1;
                if current_width <= 0 {
                    return None;
                }
                let _ = value;
                Some(ChunkPos::new(
                    *min_x + strip_x as i32 + (offset % current_width as u64) as i32,
                    *min_z + (offset / current_width as u64) as i32,
                ))
            }
            State::Concentric {
                x,
                z,
                center_x,
                center_z,
                annulus,
                span,
                down,
                left,
                up,
                right,
                exhausted,
                ..
            } => {
                if *exhausted {
                    return None;
                }
                let chunk = ChunkPos::new(*x, *z);
                if *x == *center_x + *annulus && *z == *center_z + *annulus {
                    *annulus += 1;
                    *x += 1;
                    *z += 1;
                    if *annulus > concentric_radius {
                        *exhausted = true;
                    }
                    *span = 2 * *annulus;
                    *down = 0;
                    *left = 0;
                    *up = 0;
                    *right = 0;
                }
                if *down < *span {
                    *z -= 1;
                    *down += 1;
                } else if *left < *span {
                    *x -= 1;
                    *left += 1;
                } else if *up < *span {
                    *z += 1;
                    *up += 1;
                } else if *right < *span {
                    *x += 1;
                    *right += 1;
                }
                Some(chunk)
            }
            State::Spiral {
                x,
                z,
                stop_x,
                stop_z,
                span,
                span_count,
                span_progress,
                direction,
                exhausted,
            } => {
                if *exhausted {
                    return None;
                }
                let chunk = ChunkPos::new(*x, *z);
                if *x == *stop_x && *z == *stop_z {
                    *exhausted = true;
                    return Some(chunk);
                }
                if *span_count == 2 {
                    *span_count = 0;
                    *span += 1;
                }
                match *direction {
                    0 => *x += 1,
                    1 => *z -= 1,
                    2 => *x -= 1,
                    _ => *z += 1,
                }
                *span_progress += 1;
                if *span_progress == *span {
                    *span_progress = 0;
                    *span_count += 1;
                    *direction = if *direction == 3 { 0 } else { *direction + 1 };
                }
                Some(chunk)
            }
            State::Ring {
                center_x,
                center_z,
                radius,
                ring,
                inner,
                outer,
                current,
                total_ring,
                started,
                exhausted,
            } => {
                if *exhausted {
                    return None;
                }
                if !*started {
                    if !ring_next_ring(*ring, *radius, inner, outer, total_ring) {
                        *exhausted = true;
                        return None;
                    }
                    *ring += 1;
                    *current = 0;
                    *started = true;
                }
                if *current >= *total_ring {
                    *started = false;
                    if !ring_next_ring(*ring, *radius, inner, outer, total_ring) {
                        *exhausted = true;
                        return None;
                    }
                    *ring += 1;
                    *current = 0;
                }
                let side = 2 * *outer + 1;
                let length = (*outer - *inner) as u64;
                let (dx, dz) = if *inner < 0 {
                    (
                        (*current % side as u64) as i32 - *outer,
                        (*current / side as u64) as i32 - *outer,
                    )
                } else {
                    let first_side_chunks = (side as u64) * length;
                    let mirrored = *current >= *total_ring / 2;
                    let step = if mirrored {
                        *current - *total_ring / 2
                    } else {
                        *current
                    };
                    let sign = if mirrored { -1 } else { 1 };
                    if step < first_side_chunks {
                        (
                            sign * ((step / length) as i32 - *outer),
                            sign * (-*inner - 1 - (step % length) as i32),
                        )
                    } else {
                        let turn_step = step - first_side_chunks;
                        (
                            sign * (*inner + 1 + (turn_step % length) as i32),
                            sign * ((turn_step / length) as i32 - *inner),
                        )
                    }
                };
                *current += 1;
                Some(ChunkPos::new(*center_x + dx, *center_z + dz))
            }
            State::Region {
                regions,
                current,
                bounds,
                complete,
            } => loop {
                if let Some(chunk) = current.as_mut().and_then(RegionChunks::next) {
                    *complete += 1;
                    return Some(chunk);
                }
                *current = Some(RegionChunks::new(regions.next()?, *bounds));
            },
            State::Queue { chunks } => chunks.pop_front(),
        }
    }

    fn concentric_radius(&self) -> i32 {
        match &self.state {
            State::Concentric { radius, .. } => *radius,
            _ => 0,
        }
    }

    /// Skip `count` chunks of the region walk, which is how a paused region
    /// task resumes: whole regions are dropped first, then the rest.
    fn region_advance(&mut self, mut count: u64) {
        if count == 0 {
            return;
        }
        let State::Region {
            regions,
            current,
            bounds,
            complete,
        } = &mut self.state
        else {
            return;
        };
        let _ = complete;
        loop {
            if current.is_none() {
                match regions.next() {
                    Some(region) => *current = Some(RegionChunks::new(region, *bounds)),
                    None => return,
                }
            }
            let Some(region) = current.as_mut() else {
                return;
            };
            if count >= region.total {
                count -= region.total;
                *current = None;
                continue;
            }
            region.resume(count);
            return;
        }
    }
}

/// The regions of a selection, in the expanding square ring order the region
/// pattern walks them.
fn region_order(center_x: i32, center_z: i32, radius: i32) -> impl Iterator<Item = (i32, i32)> {
    let mut x = center_x;
    let mut z = center_z;
    let mut annulus = 0;
    let mut span = 0;
    let (mut down, mut left, mut up, mut right) = (0, 0, 0, 0);
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let region = (x, z);
        if x == center_x + annulus && z == center_z + annulus {
            annulus += 1;
            x += 1;
            z += 1;
            if annulus > radius {
                done = true;
                return Some(region);
            }
            span = 2 * annulus;
            down = 0;
            left = 0;
            up = 0;
            right = 0;
        }
        if down < span {
            z -= 1;
            down += 1;
        } else if left < span {
            x -= 1;
            left += 1;
        } else if up < span {
            z += 1;
            up += 1;
        } else if right < span {
            x += 1;
            right += 1;
        }
        Some(region)
    })
}

/// Puts the concentric walk at the position `count` chunks in.
fn concentric_resume(state: &mut State, count: u64, radius: i32) {
    let State::Concentric {
        x,
        z,
        center_x,
        center_z,
        annulus,
        span,
        down,
        left,
        up,
        right,
        exhausted,
        ..
    } = state
    else {
        return;
    };
    let mut diameter = (count as f64).sqrt().floor() as i64;
    if diameter % 2 == 0 {
        diameter -= 1;
    }
    let annulus_value = (diameter / 2 + 1) as i32;
    *annulus = annulus_value;
    *x = *center_x + annulus_value;
    *z = *center_z + annulus_value;
    *span = 2 * annulus_value;
    let perimeter = count as i64 - diameter * diameter + 1;
    let draw = |used: &mut i32, spent: &mut i64| {
        let take = (*span as i64).min(*spent);
        *used = take as i32;
        *spent -= take;
    };
    let mut remaining = perimeter;
    draw(&mut *down, &mut remaining);
    draw(&mut *left, &mut remaining);
    draw(&mut *up, &mut remaining);
    draw(&mut *right, &mut remaining);
    *x += *right - *left;
    *z += *up - *down;
    *exhausted = *annulus > radius;
}

/// Puts the spiral walk at the position `count` chunks in
/// (`SpiralChunkIterator`'s counting constructor).
///
/// The four segments are written out one by one rather than looped over
/// because `spanCount` is not set the same way by each: a completed `DOWN` or
/// `UP` segment leaves it at 0, a completed `LEFT` or `RIGHT` one at 1. A loop
/// cannot tell those apart, and getting it wrong makes a resumed walk turn one
/// corner too early.
fn spiral_resume(state: &mut State, count: u64, radius: i32) {
    // Read before the state is borrowed below.
    let stop_x = state_stop_x(state);
    let State::Spiral {
        x,
        z,
        span,
        span_count,
        span_progress,
        direction,
        exhausted,
        ..
    } = state
    else {
        return;
    };
    let mut diameter = (count as f64).sqrt().floor() as i64;
    if diameter % 2 == 0 {
        diameter -= 1;
    }
    *span = diameter as i32;
    *span_count = 1;
    *direction = 1; // DOWN
    let finished_radius = (diameter / 2) as i32;
    *x += finished_radius + 1;
    *z += finished_radius;
    let mut perimeter = (count as i64 - diameter * diameter).max(0);

    // DOWN
    let mut spanned = (*span as i64).min(perimeter);
    *z -= spanned as i32;
    if spanned == *span as i64 {
        *span += 1;
        *span_count = 0;
        *direction = 2; // LEFT
    } else {
        *span_progress += spanned as i32;
    }
    perimeter -= spanned;

    // LEFT
    spanned = (*span as i64).min(perimeter);
    *x -= spanned as i32;
    if spanned == *span as i64 {
        *span_count = 1;
        *direction = 3; // UP
    } else {
        *span_progress += spanned as i32;
    }
    perimeter -= spanned;

    // UP
    spanned = (*span as i64).min(perimeter);
    *z += spanned as i32;
    if spanned == *span as i64 {
        *span += 1;
        *span_count = 0;
        *direction = 0; // RIGHT
    } else {
        *span_progress += spanned as i32;
    }
    perimeter -= spanned;

    // RIGHT
    spanned = (*span as i64).min(perimeter);
    *x += spanned as i32;
    let _ = spanned == *span as i64;
    if spanned == *span as i64 {
        *span_count = 1;
        *direction = 1; // DOWN
    } else {
        *span_progress += spanned as i32;
    }

    let _ = radius;
    *exhausted = *x > stop_x;
}

fn state_stop_x(state: &State) -> i32 {
    match state {
        State::Spiral { stop_x, .. } => *stop_x,
        _ => i32::MAX,
    }
}

/// Opens the next ring of the ring walk: its inner and outer half-widths and
/// how many chunks it holds. False once the walk has left the selection.
fn ring_next_ring(
    ring: i32,
    radius: i32,
    inner: &mut i32,
    outer: &mut i32,
    total: &mut u64,
) -> bool {
    let center_radius = (WIDTH - 1) / 2;
    let inner_value = if ring == 0 {
        -1
    } else {
        center_radius + (ring - 1) * WIDTH
    };
    if inner_value >= radius {
        return false;
    }
    let outer_value = (center_radius + ring * WIDTH).min(radius);
    *inner = inner_value;
    *outer = outer_value;
    let side = (2 * outer_value + 1) as u64;
    let inner_side = if inner_value < 0 {
        0
    } else {
        (2 * inner_value + 1) as u64
    };
    *total = side * side - inner_side * inner_side;
    true
}

/// Moves the ring walk forward to the ring `count` chunks in.
fn ring_advance(state: &mut State, mut count: u64) {
    let State::Ring {
        ring,
        inner,
        outer,
        current,
        total_ring,
        started,
        exhausted,
        radius,
        ..
    } = state
    else {
        return;
    };
    *ring = 0;
    *started = false;
    *current = 0;
    *exhausted = false;
    let center_radius = (WIDTH - 1) / 2;
    loop {
        if !*started {
            if !ring_next_ring(*ring, *radius, inner, outer, total_ring) {
                *exhausted = true;
                return;
            }
            *ring += 1;
            *current = 0;
            *started = true;
        }
        if count < *total_ring {
            *current = count;
            return;
        }
        let _ = center_radius;
        count -= *total_ring;
        *started = false;
    }
}

/// The chunks a `csv` pattern names, in file order (`CsvChunkIterator`).
fn read_csv(value: &str, sources: Sources<'_>) -> Result<Vec<ChunkPos>, PatternError> {
    let file = format!("{value}.csv");
    let path = sources
        .data_dir
        .map(|dir| dir.join(&file))
        .filter(|path| path.is_file())
        .or_else(|| {
            sources
                .config_dir
                .map(|dir| dir.join(&file))
                .filter(|path| path.is_file())
        });
    let Some(path) = path else {
        return Err(PatternError::CsvNotFound(value.to_owned()));
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|error| PatternError::CsvUnreadable(value.to_owned(), error.to_string()))?;
    Ok(parse_csv(&text))
}

/// A CSV pattern's lines: `x,z`, extra columns ignored, unparsable lines
/// skipped.
pub fn parse_csv(text: &str) -> Vec<ChunkPos> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            let x = parts
                .next()
                .and_then(|value| value.trim().parse::<i32>().ok())?;
            let z = parts
                .next()
                .and_then(|value| value.trim().parse::<i32>().ok())?;
            // A line with more columns than two keeps its first two, and one
            // with a number that does not parse is skipped, as Chunky does.
            Some(ChunkPos::new(x, z))
        })
        .collect()
}

/// The chunks a `world` pattern finds: the selection's region files are read,
/// and every chunk they hold that is full and inside the selection is queued,
/// together with the partial chunks that are surrounded by other chunks and
/// not by the edge of what has been generated.
fn scan_world(
    selection: &Selection,
    sources: Sources<'_>,
    scan: &mut impl FnMut(u64, u64) -> bool,
) -> Result<Vec<ChunkPos>, PatternError> {
    let Some(region_dir) = sources.region_dir else {
        return Err(PatternError::NoRegionDirectory);
    };
    if !region_dir.is_dir() {
        return Err(PatternError::NoRegionDirectory);
    }
    let math = selection.math();
    let bounds = crate::geometry::Region::new(
        math.center_chunk_x - math.radius_chunks_x,
        math.center_chunk_z - math.radius_chunks_z,
        math.center_chunk_x + math.radius_chunks_x,
        math.center_chunk_z + math.radius_chunks_z,
    );
    let region_bounds = crate::geometry::Region::new(
        bounds.min_x >> 5,
        bounds.min_z >> 5,
        bounds.max_x >> 5,
        bounds.max_z >> 5,
    );
    let mut regions: Vec<(i32, i32)> = crate::anvil::region_files(region_dir)
        .into_iter()
        .filter(|region| region_bounds.contains(ChunkPos::new(region.0, region.1)))
        .collect();
    regions.sort_unstable();
    let total_regions = regions.len() as u64;

    let mut full: Vec<ChunkPos> = Vec::new();
    let mut partial: Vec<ChunkPos> = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        if !scan(index as u64 + 1, total_regions) {
            return Err(PatternError::NoRegionDirectory);
        }
        let Ok(mut file) = crate::anvil::RegionFile::open(
            &region_dir.join(format!("r.{}.{}.mca", region.0, region.1)),
        ) else {
            continue;
        };
        for offset in hilbert::offsets() {
            let chunk = ChunkPos::new((region.0 << 5) + offset.x, (region.1 << 5) + offset.z);
            let status = file.chunk_status(chunk);
            match status {
                None => continue,
                Some(status) if status.is_empty() => continue,
                Some(status) if status.is_full() => full.push(chunk),
                Some(_) => partial.push(chunk),
            }
        }
    }

    let mut queued: Vec<ChunkPos> = Vec::new();
    let use_full = selection.force_load_existing_chunks;
    for chunk in &partial {
        if use_full {
            break;
        }
        if bounds.contains(*chunk) && !is_exterior(*chunk, &partial, &full) {
            queued.push(*chunk);
        }
    }
    let mut full_in_bounds: Vec<ChunkPos> = full
        .into_iter()
        .filter(|chunk| use_full || bounds.contains(*chunk))
        .collect();
    full_in_bounds.extend(queued);
    full_in_bounds.sort_unstable();
    Ok(full_in_bounds)
}

/// Whether a partial chunk touches the edge of what has been generated: a
/// chunk with a neighbour that is neither full nor partial is a seed, and the
/// seeds flood outwards through the partial chunks.
fn is_exterior(chunk: ChunkPos, partial: &[ChunkPos], full: &[ChunkPos]) -> bool {
    let known = |x: i32, z: i32| {
        full.contains(&ChunkPos::new(x, z)) || partial.contains(&ChunkPos::new(x, z))
    };
    let mut queue = VecDeque::from([chunk]);
    let mut seen = std::collections::HashSet::from([chunk]);
    while let Some(current) = queue.pop_front() {
        for (dx, dz) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let neighbour = ChunkPos::new(current.x + dx, current.z + dz);
            if known(neighbour.x, neighbour.z) {
                if seen.insert(neighbour) {
                    queue.push_back(neighbour);
                }
            } else if !full.contains(&neighbour) {
                // A neighbour that is neither full nor partial: the region
                // ends here, so this chunk is on the outside.
                return true;
            }
        }
    }
    false
}

/// The region directory name of a world's dimension, as Horizon stores it.
pub fn region_dir_name(world_root: &Path) -> PathBuf {
    world_root.join("region")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::ShapeKind;

    fn selection(shape: ShapeKind, radius: f64, pattern: PatternKind) -> Selection {
        Selection {
            shape,
            radius_x: radius,
            radius_z: radius,
            pattern: Pattern::new(pattern),
            ..Selection::default()
        }
    }

    fn collect(mut chunks: Chunks, limit: usize) -> Vec<ChunkPos> {
        (0..limit).filter_map(|_| chunks.next()).collect()
    }

    #[test]
    fn loop_walks_x_then_z() {
        let selection = selection(ShapeKind::Square, 16.0, PatternKind::Loop);
        let chunks = Chunks::new(&selection, 0, Sources::default(), |_, _| true).unwrap();
        assert_eq!(chunks.total(), 9); // radius 1 chunk -> 3x3
        assert_eq!(
            collect(chunks, 9),
            vec![
                ChunkPos::new(-1, -1),
                ChunkPos::new(-1, 0),
                ChunkPos::new(-1, 1),
                ChunkPos::new(0, -1),
                ChunkPos::new(0, 0),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, -1),
                ChunkPos::new(1, 0),
                ChunkPos::new(1, 1),
            ]
        );
    }

    #[test]
    fn concentric_rings_outwards_from_the_centre() {
        let selection = selection(ShapeKind::Square, 16.0, PatternKind::Concentric);
        let chunks = Chunks::new(&selection, 0, Sources::default(), |_, _| true).unwrap();
        let first = collect(chunks, 9);
        assert_eq!(first[0], ChunkPos::new(0, 0));
        assert_eq!(first[1], ChunkPos::new(1, 0));
        assert_eq!(first[2], ChunkPos::new(1, -1));
        assert_eq!(first[8], ChunkPos::new(1, 1));
    }

    #[test]
    fn a_rectangle_lays_its_chunks_in_strips_whatever_the_pattern() {
        let selection = selection(ShapeKind::Rectangle, 16.0, PatternKind::Ring);
        let mut chunks = Chunks::new(&selection, 0, Sources::default(), |_, _| true).unwrap();
        assert_eq!(chunks.name(), "ring");
        // The strip walk still starts at the selection's corner.
        assert_eq!(chunks.next(), Some(ChunkPos::new(-1, -1)));
    }

    #[test]
    fn a_paused_walk_resumes_where_it_stopped() {
        for pattern in [
            PatternKind::Loop,
            PatternKind::Concentric,
            PatternKind::Spiral,
            PatternKind::Ring,
            PatternKind::Region,
        ] {
            // Radius 320 blocks is 20 chunks, so 41x41 = 1681 chunks: enough
            // for the 400 the comparison walks.
            let selection = selection(ShapeKind::Square, 320.0, pattern);
            let all = collect(
                Chunks::new(&selection, 0, Sources::default(), |_, _| true).unwrap(),
                600,
            );
            let count = 100;
            let resumed = collect(
                Chunks::new(&selection, count as u64, Sources::default(), |_, _| true).unwrap(),
                500,
            );
            assert_eq!(
                &all[count..],
                &resumed[..],
                "{pattern:?} did not resume where it stopped"
            );
        }
    }

    #[test]
    fn a_strip_walks_bands_of_64() {
        let selection = selection(ShapeKind::Square, 16.0 * 70.0, PatternKind::Strip);
        let mut chunks = Chunks::new(&selection, 0, Sources::default(), |_, _| true).unwrap();
        let min_x = selection.math().center_chunk_x - selection.math().radius_chunks_x;
        let min_z = min_x;
        let first = chunks.next().unwrap();
        assert_eq!(first, ChunkPos::new(min_x, min_z));
        // The 71st chunk of the band is the first of the second column.
        // A band is 64 chunks wide in x, so the 65th chunk starts the next row
        // of the band at the same x, and the 71st is six along it.
        let mut last = first;
        for _ in 0..64 {
            last = chunks.next().unwrap();
        }
        assert_eq!(last, ChunkPos::new(min_x, min_z + 1));
        for _ in 0..6 {
            last = chunks.next().unwrap();
        }
        assert_eq!(last, ChunkPos::new(min_x + 6, min_z + 1));
    }

    #[test]
    fn a_csv_pattern_reads_x_z_lines() {
        let chunks = parse_csv("0,0\n1,2\nnonsense\n-3,4,extra\n");
        assert_eq!(
            chunks,
            vec![
                ChunkPos::new(0, 0),
                ChunkPos::new(1, 2),
                ChunkPos::new(-3, 4)
            ]
        );
    }
}
