//! A generation task: what it is doing, how far it has got, and what it keeps
//! between runs.
//!
//! The port of Chunky's `GenerationTask`, `GenerationProgress`, `RateLimiter`,
//! `RegionStatusReader` and `TaskLoader`. Chunky runs one task per world on a
//! thread of its own, with a semaphore of up to 64 chunks in flight and a
//! `RateLimiter` that parks the thread until the budget allows another; Horizon
//! ticks the game, so the same decisions are made by [`Task::next_to_load`],
//! which the glue calls from a tick hook and answers by forcing the chunk in.
//!
//! What that changes is where the chunks are asked for, and one thing a player
//! could notice: Chunky starts a chunk the moment its budget allows, while this
//! starts them in a batch once a tick, so a task's rate has a 50 ms granularity
//! rather than a nanosecond one. Everything else — the counts, the percentage,
//! the ETA, the rate, the pause, cancel and resume, and the saved file — is
//! Chunky's own.
//!
//! The saved file is Chunky's own: `tasks/<world with : replaced by />.properties`
//! beside the world, one `key=value` a line, which is what makes a task started
//! before the port usable after it.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use crate::anvil::RegionFile;
use crate::geometry::{ChunkPos, format_duration};
use crate::pattern::Chunks;
use crate::selection::Selection;
use crate::shape::ShapeKind;

/// The folder tasks are saved in, beside a world's `level.dat`.
pub const TASKS_DIR: &str = "tasks";
/// The log of what tasks have done, in the same folder.
pub const TASKS_LOG: &str = "tasks.log";
/// How often a running task writes its progress, in seconds
/// (`chunky.autoSaveInterval`, default 60).
pub const SAVE_INTERVAL_SECONDS: u64 = 60;
/// The most chunks one task may have in flight at once
/// (`chunky.maxWorkingCount`, default 64).
///
/// The glue lowers this for Horizon: a forced chunk there is held with the 5x5
/// ring around it, so a few dozen is already a large part of the world loaded
/// at once. Chunky's own number is the default here so the maths is the same.
pub const MAX_WORKING_COUNT: usize = 64;
/// How long a rate limiter's window is, in milliseconds
/// (`chunky.sampleInterval`, default 30 seconds).
pub const SAMPLE_INTERVAL_MILLIS: u64 = 30_000;
/// A rate limiter's slot, in milliseconds.
pub const SLOT_MILLIS: u64 = 100;
/// How long one rate sample covers, `SAMPLE_INTERVAL_MILLIS / 30`.
const SAMPLE_SUB_INTERVAL_MILLIS: u64 = SAMPLE_INTERVAL_MILLIS / 30;
/// The most chunks one call to [`Task::next_to_load`] counts over without
/// loading one, before it hands back to the caller. Chunky's thread spins
/// through them for as long as it likes; a tick may not, since a chunk already
/// on disk costs a region file read.
pub const COUNTED_PER_CALL: u64 = 64;

/// Where a task is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Saved and not running, with nothing asked for.
    Paused,
    /// Running.
    Running,
    /// Asked to stop, and finishing the chunks it had out.
    Pausing,
    /// Asked to cancel, and finishing the chunks it had out.
    Cancelling,
    /// Stopped and cancelled: it will not be continued.
    Cancelled,
    /// Every chunk it wanted is done.
    Completed,
}

impl State {
    pub fn is_running(self) -> bool {
        self == State::Running
    }

    pub fn is_paused(self) -> bool {
        self == State::Paused
    }

    /// Whether the task is over, which is what keeps `continue` from picking it
    /// up. Chunky writes `cancelled=true` for a completed task too, so the
    /// answer to "may this be continued" is the same either way.
    pub fn is_cancelled(self) -> bool {
        matches!(self, State::Cancelled | State::Completed)
    }
}

/// Chunky's `RateLimiter`: a budget of `limit` chunks a second, spent over a
/// window of 100 ms slots, counting the chunks that have *finished* rather than
/// the ones that started, and with the chunks still in flight counted against
/// the budget until they do.
///
/// Chunky's `delay` has a second term besides the budget: a clock that lets one
/// chunk start every `1 / limit` seconds, so the starts are spread out rather
/// than arriving in a clump whenever the budget grows. There is no sub-tick
/// time to spend here — the game ticks 20 times a second and the glue starts a
/// batch of chunks in each of them — so the budget is the whole of it, and a
/// task loads the same number of chunks a second spread over the ticks instead
/// of over the milliseconds.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    /// Slots in the window, `window / SLOT_MILLIS + 1`.
    slots: usize,
    /// The window's length, `(slots - 1) * SLOT_MILLIS`.
    window_millis: u64,
    /// The 100 ms slot each count belongs to, so a stale one can be cleared.
    slot_indexes: Vec<u64>,
    slot_counts: Vec<u64>,
    /// 0 for no limit.
    limit: u64,
    /// Set when the limit changes, so the window starts over.
    restart: bool,
    /// The window's number, handed out with each chunk and checked on release.
    window: u64,
    /// When the window started, in Unix millis.
    start: u64,
}

impl RateLimiter {
    pub fn new(window_millis: u64, limit: u64) -> Self {
        let slots = (window_millis / SLOT_MILLIS) as usize + 1;
        Self {
            slots,
            window_millis: (slots as u64 - 1) * SLOT_MILLIS,
            slot_indexes: vec![0; slots],
            slot_counts: vec![0; slots],
            limit,
            restart: true,
            window: 0,
            start: 0,
        }
    }

    /// The limit in chunks a second, 0 for none.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Sets the limit, as `/chunky limit` does. A change restarts the window.
    pub fn set_limit(&mut self, limit: u64) {
        if limit != self.limit {
            self.limit = limit;
            self.restart = true;
        }
    }

    /// Whether the window is about to start over, which the tests read.
    pub fn is_restarting(&self) -> bool {
        self.restart
    }

    /// The window a chunk started now belongs to, which [`Self::release`] is
    /// handed back when the chunk finishes.
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Whether one more chunk may start: Chunky's `delay(...) == 0`, without
    /// the parking, which the tick's own pacing replaces.
    ///
    /// `working` is how many chunks the task has in flight; they count against
    /// the budget until they finish, as they do in Chunky.
    pub fn allows(&mut self, now: u64, working: u64) -> bool {
        if self.limit == 0 {
            return true;
        }
        if self.restart {
            self.restart = false;
            self.window += 1;
            self.slot_counts.iter_mut().for_each(|count| *count = 0);
            self.start = now;
        }
        let allowance = self.limit * self.window_millis.min(now.saturating_sub(self.start)) / 1000;
        self.count(now, self.slots) + working <= allowance
    }

    /// Counts a finished chunk against the window it started in. A chunk whose
    /// window is not the current one is forgotten, as Chunky forgets it.
    pub fn release(&mut self, window: u64, now: u64) {
        if window == 0 || window != self.window {
            return;
        }
        let index = now / SLOT_MILLIS;
        let slot = (index % self.slots as u64) as usize;
        if self.slot_indexes[slot] != index {
            self.slot_indexes[slot] = index;
            self.slot_counts[slot] = 0;
        }
        self.slot_counts[slot] = self.slot_counts[slot].saturating_add(1);
    }

    /// Chunks a second over the window, which is the `Rate` of a report.
    pub fn rate(&self, now: u64) -> f64 {
        let millis = self.window_millis.min(now.saturating_sub(self.start));
        if self.limit == 0 || self.restart || millis == 0 {
            return 0.0;
        }
        1000.0 * self.count(now, self.slots - 1) as f64 / millis as f64
    }

    /// How many chunks were finished in the last `slots` slots.
    fn count(&self, now: u64, slots: usize) -> u64 {
        let index = now / SLOT_MILLIS;
        let mut count = 0;
        for slot in 0..self.slots {
            if self.slot_indexes[slot] > index.saturating_sub(slots as u64) {
                count += self.slot_counts[slot];
            }
        }
        count
    }
}

/// Chunky's `updateSamples`: finished chunks in bins of a second over the last
/// thirty, which is how the rate is measured when no limit is set.
#[derive(Clone, Debug, Default)]
pub struct Samples {
    bins: VecDeque<(u64, u64)>,
}

impl Samples {
    /// Records one finished chunk at `now`. A chunk that was not loaded does
    /// not count towards the rate, but it does start a bin, as Chunky's does.
    pub fn record(&mut self, now: u64, loaded: bool) {
        match self.bins.back_mut() {
            None => {
                self.bins.push_back((now, u64::from(loaded)));
            }
            Some(bin) if loaded && now.saturating_sub(bin.0) < SAMPLE_SUB_INTERVAL_MILLIS => {
                bin.1 += 1;
            }
            Some(_) => {
                self.bins.push_back((now, u64::from(loaded)));
            }
        }
        while let Some(front) = self.bins.front() {
            if now.saturating_sub(front.0) > SAMPLE_INTERVAL_MILLIS {
                self.bins.pop_front();
            } else {
                break;
            }
        }
    }

    /// The oldest bin's time, which is what the rate is averaged over.
    pub fn oldest(&self) -> Option<u64> {
        self.bins.front().map(|bin| bin.0)
    }

    /// How many chunks the window holds.
    pub fn count(&self) -> u64 {
        self.bins.iter().map(|bin| bin.1).sum()
    }
}

/// A task's progress, as `GenerationProgress` keeps it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// Chunks finished, whether they were loaded or already there.
    pub count: u64,
    /// Chunks the task wants in all.
    pub total: u64,
    /// The chunk being worked on, for the report's `Current:`.
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// Chunks a second, as the last report measured it.
    pub rate: f64,
    /// Whether the task is done.
    pub complete: bool,
    /// The ETA the last report showed, in seconds.
    pub eta_seconds: u64,
    /// When the task started, in Unix millis.
    pub started_at: u64,
    /// When the last chunk finished, in Unix millis.
    pub finished_at: u64,
    /// Time a previous run took, for a task that was continued.
    pub previous_seconds: u64,
}

impl Progress {
    /// How far along, as a percentage.
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }
        100.0 * self.count.min(self.total) as f64 / self.total as f64
    }

    /// How long the task has been running, this run and any before it.
    pub fn elapsed_seconds(&self) -> u64 {
        let this_run = self.finished_at.saturating_sub(self.started_at);
        self.previous_seconds + this_run / 1000
    }

    /// The ETA as the three arguments of a report.
    pub fn eta_parts(&self) -> (String, String, String) {
        crate::geometry::duration_parts(self.eta_seconds)
    }

    /// The whole run's time, as the three arguments of the finished report.
    pub fn time_parts(&self) -> (String, String, String) {
        let total = if self.complete {
            self.elapsed_seconds()
        } else {
            self.eta_seconds
        };
        crate::geometry::duration_parts(total)
    }
}

/// The region file a task is reading to tell whether a chunk is already done,
/// kept open while the walk stays in that region (`RegionStatusReader` and
/// `RegionCache`).
///
/// It answers two questions, which Chunky asks in this order:
/// [`Self::already_generated`] is the session's own record of the chunks it has
/// seen loaded — a chunk the world has in memory is done, whatever the disk
/// says — and [`Self::is_generated`] reads the region file for the rest.
pub struct RegionCache {
    region_dir: Option<PathBuf>,
    open: Option<((i32, i32), RegionFile)>,
    /// What has been decided about a chunk so far, so a region file is read
    /// once for it however many tasks ask (`RegionCache.WorldState`).
    known: HashMap<(i32, i32), bool>,
}

impl RegionCache {
    /// A cache over a world's `region/` folder, which may not be there yet.
    pub fn new(world_dir: Option<&Path>) -> Self {
        let region_dir = world_dir
            .map(|dir| dir.join(crate::trim::REGION_DIR))
            .filter(|dir| dir.is_dir());
        Self {
            region_dir,
            open: None,
            known: HashMap::new(),
        }
    }

    /// Records a chunk as generated, which the glue does for every chunk the
    /// game reports loaded (`WorldState.setGenerated`).
    pub fn set_generated(&mut self, chunk: ChunkPos) {
        self.known.insert((chunk.x, chunk.z), true);
    }

    /// Whether this session has already seen the chunk generated, which is
    /// Chunky's `WorldState.isGenerated`.
    pub fn already_generated(&self, chunk: ChunkPos) -> bool {
        self.known
            .get(&(chunk.x, chunk.z))
            .copied()
            .unwrap_or(false)
    }

    /// Whether the chunk is already on disk, finished: full, lit, updated and
    /// in its right place. A chunk the cache cannot read is not generated, so
    /// the task loads it, which is what Chunky does when the region status is
    /// unknown.
    pub fn is_generated(&mut self, chunk: ChunkPos) -> bool {
        let key = (chunk.x, chunk.z);
        if let Some(known) = self.known.get(&key) {
            return *known;
        }
        let generated = self.read_region(chunk);
        self.known.insert(key, generated);
        generated
    }

    fn read_region(&mut self, chunk: ChunkPos) -> bool {
        let Some(dir) = self.region_dir.clone() else {
            return false;
        };
        let region = (chunk.x >> 5, chunk.z >> 5);
        if self.open.as_ref().is_none_or(|(open, _)| *open != region) {
            let path = dir.join(crate::anvil::region_name(region.0, region.1));
            self.open = match RegionFile::open(&path) {
                Ok(file) => Some((region, file)),
                Err(_) => None,
            };
        }
        match self.open.as_mut() {
            Some((_, file)) => file.is_generated(chunk),
            None => false,
        }
    }
}

/// What a task asks the game for next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The game is to hold this chunk until it loads
    /// (`native_force_chunk_in`); the task keeps it until [`Task::loaded`]
    /// says it arrived.
    Load(ChunkPos),
    /// Nothing to ask for: the chunk was counted — it lies outside the shape,
    /// or it was already there — and the caller goes on to the next.
    Counted,
}

/// A saved task, or a running one: everything `/chunky continue` needs.
pub struct Task {
    pub selection: Selection,
    pub progress: Progress,
    pub state: State,
    /// Chunks a second to start, 0 for no limit.
    pub rate_limit: u64,
    /// The player who started it, empty for the console.
    pub owner: String,
    /// When the task was last written, in Unix millis.
    pub saved_at: u64,
    /// Whether chunks already on disk are to be loaded anyway.
    pub force_load_existing_chunks: bool,
    /// How many chunks may be in flight at once.
    pub max_working_count: usize,
    /// The chunks still to produce, rebuilt when the task starts or resumes.
    pub(crate) chunks: Option<Chunks>,
    /// The chunk taken from the walk that the rate limit would not let through
    /// yet. Chunky's thread parks on the limit with the chunk already in hand;
    /// a tick cannot park, so it is kept here until the next call.
    pub(crate) pending: Option<ChunkPos>,
    /// The chunks asked for and not yet seen loaded, with the rate window each
    /// belongs to.
    pub(crate) working: Vec<(ChunkPos, u64)>,
    pub(crate) rate_limiter: RateLimiter,
    pub(crate) samples: Samples,
    /// When the last report went out, and when the last save was made.
    pub(crate) reported_at: u64,
    pub(crate) saved_automatically_at: u64,
}

impl Task {
    /// A task for a selection, not yet started.
    pub fn new(selection: Selection) -> Self {
        let rate_limit = selection.rate_limit;
        let force_load_existing_chunks = selection.force_load_existing_chunks;
        Self {
            selection,
            progress: Progress::default(),
            state: State::Paused,
            rate_limit,
            force_load_existing_chunks,
            owner: String::new(),
            saved_at: 0,
            max_working_count: MAX_WORKING_COUNT,
            chunks: None,
            pending: None,
            working: Vec::new(),
            rate_limiter: RateLimiter::new(SAMPLE_INTERVAL_MILLIS, rate_limit),
            samples: Samples::default(),
            reported_at: 0,
            saved_automatically_at: 0,
        }
    }

    pub fn world(&self) -> &str {
        &self.selection.world
    }

    /// How many chunks are in flight.
    pub fn working_count(&self) -> usize {
        self.working.len()
    }

    /// The chunks the task is holding, which the glue must release.
    pub fn held(&self) -> Vec<ChunkPos> {
        self.working.iter().map(|(chunk, _)| *chunk).collect()
    }

    /// Asks a running task to stop. `cancelled` decides whether it may be
    /// continued; Chunky's `stop`.
    pub fn stop(&mut self, cancelled: bool) {
        self.state = if cancelled {
            State::Cancelled
        } else {
            State::Paused
        };
        self.working.clear();
    }

    /// Sets the rate limit, as `/chunky limit` does.
    pub fn set_rate_limit(&mut self, limit: u64) {
        self.rate_limit = limit;
        self.rate_limiter.set_limit(limit);
    }

    /// The rate limit the limiter is working to, which follows the selection
    /// until `/chunky limit` changes it.
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    /// Starts or resumes the walk. The pattern is rebuilt from the count the
    /// saved progress reached, so the walk goes on where it stopped.
    pub fn begin(&mut self, now: u64) {
        if self.progress.started_at == 0 {
            self.progress.started_at = now;
        }
        self.progress.finished_at = now;
        self.progress.complete = false;
        self.state = State::Running;
        self.reported_at = now;
        self.saved_automatically_at = now;
    }

    /// Builds the walk for the task's selection, at the count the progress has
    /// reached, so a task that was paused carries on where it stopped.
    ///
    /// `scan` is told how far a `world` pattern has got through reading the
    /// region files, and may stop it by returning false.
    pub fn build_chunks(
        &mut self,
        sources: crate::pattern::Sources<'_>,
        scan: impl FnMut(u64, u64) -> bool,
    ) -> Result<(), crate::pattern::PatternError> {
        let chunks = Chunks::new(&self.selection, self.progress.count, sources, scan)?;
        self.progress.total = chunks.total();
        self.chunks = Some(chunks);
        Ok(())
    }

    /// Marks the task over, whatever stopped it.
    pub fn finish(&mut self, cancelled: bool, now: u64) {
        self.progress.finished_at = now;
        self.state = if cancelled {
            State::Cancelled
        } else {
            State::Completed
        };
        self.working.clear();
    }

    /// Whether every chunk has been asked for.
    pub fn walk_exhausted(&self) -> bool {
        self.pending.is_none()
            && match &self.chunks {
                None => true,
                Some(chunks) => chunks.produced() >= chunks.total(),
            }
    }

    /// The next thing a running task wants: Chunky's main loop, one step at a
    /// time, counting as it goes.
    ///
    /// `None` means this tick is done with, either because the walk is over,
    /// because the task is not running, or because the rate limit or the
    /// in-flight cap says to wait.
    ///
    /// A chunk outside the shape, and one this session has already loaded, are
    /// counted and passed over without touching the rate limit — Chunky
    /// `continue`s before it takes a permit for either. A chunk the region
    /// files say is finished is counted as *loaded*, permit and all, which is
    /// what Chunky does: it only knows the answer once it has asked for it.
    ///
    /// Chunky's thread spins through those freely — nothing blocks until the
    /// permit is asked for — but one of them costs a region file read here, on
    /// the server's own thread, so a call passes over at most
    /// [`COUNTED_PER_CALL`] of them and then hands back to the caller.
    pub fn next_to_load(&mut self, now: u64, cache: &mut RegionCache) -> Option<Step> {
        if self.state != State::Running {
            return None;
        }
        let mut counted = 0;
        loop {
            let chunk = match self.pending.take() {
                Some(chunk) => chunk,
                None => self.chunks.as_mut().and_then(Chunks::next)?,
            };
            // Outside the shape: counted, and no permit is taken for it.
            if !self.selection.keeps(chunk) {
                self.count(chunk, now, false);
                counted += 1;
                if counted >= COUNTED_PER_CALL {
                    return Some(Step::Counted);
                }
                continue;
            }
            // Loaded in this session already: the same, without the disk.
            if cache.already_generated(chunk) {
                self.count(chunk, now, false);
                counted += 1;
                if counted >= COUNTED_PER_CALL {
                    return Some(Step::Counted);
                }
                continue;
            }
            if self.working.len() >= self.max_working_count
                || !self.rate_limiter.allows(now, self.working.len() as u64)
            {
                // Nothing to spend on it this tick: it waits where it is.
                self.pending = Some(chunk);
                return None;
            }
            if !self.force_load_existing_chunks && cache.is_generated(chunk) {
                // Nothing to force in: it is already there, and it counts as
                // one the task has done. Its permit goes straight back.
                let window = self.rate_limiter.window();
                self.rate_limiter.release(window, now);
                self.count(chunk, now, true);
                counted += 1;
                if counted >= COUNTED_PER_CALL {
                    return Some(Step::Counted);
                }
                continue;
            }
            let window = self.rate_limiter.window();
            self.working.push((chunk, window));
            return Some(Step::Load(chunk));
        }
    }

    /// Counts one chunk towards the progress, as Chunky's `update` does.
    fn count(&mut self, chunk: ChunkPos, now: u64, loaded: bool) {
        self.progress.count += 1;
        self.progress.chunk_x = chunk.x;
        self.progress.chunk_z = chunk.z;
        self.samples.record(now, loaded);
    }

    /// Records a held chunk as loaded. Returns whether it was one of ours.
    ///
    /// A task that has been asked to stop ignores the chunks it had out, as
    /// Chunky's `update` does once `stopped` is set: they are let go and their
    /// rate permit is given back, but they are not counted.
    pub fn loaded(&mut self, chunk: ChunkPos, now: u64) -> bool {
        let Some(at) = self.working.iter().position(|(held, _)| *held == chunk) else {
            return false;
        };
        let (_, window) = self.working.swap_remove(at);
        self.rate_limiter.release(window, now);
        if self.state == State::Running {
            self.progress.finished_at = now;
            self.count(chunk, now, true);
        }
        true
    }

    /// Measures the rate and the ETA, as Chunky's `update` does, and says
    /// whether the numbers moved.
    pub fn measure(&mut self, now: u64) -> bool {
        let chunks_left = self.progress.total.saturating_sub(self.progress.count);
        let oldest = self.samples.oldest().unwrap_or(now);
        let time_diff = now.saturating_sub(oldest) as f64 / 1000.0;
        if chunks_left > 0 && time_diff < 0.1 {
            return false;
        }
        let rate = if self.rate_limiter.limit() > 0 {
            self.rate_limiter.rate(now)
        } else if time_diff > 0.0 {
            self.samples.count() as f64 / time_diff
        } else {
            0.0
        };
        if chunks_left > 0 && rate <= 0.0 {
            return false;
        }
        self.progress.rate = rate;
        if chunks_left == 0 {
            self.progress.eta_seconds = 0;
            self.progress.complete = true;
        } else {
            self.progress.eta_seconds = (chunks_left as f64 / rate) as u64;
        }
        true
    }

    /// Whether enough time has passed since the last report
    /// (`config.updateInterval` seconds).
    pub fn report_due(&self, now: u64, interval_seconds: u64) -> bool {
        now.saturating_sub(self.reported_at) / 1000 > interval_seconds
    }

    /// Marks the report as sent.
    pub fn reported(&mut self, now: u64) {
        self.reported_at = now;
    }

    /// Whether the task should write itself out again (`autoSaveInterval`).
    pub fn autosave_due(&self, now: u64) -> bool {
        SAVE_INTERVAL_SECONDS > 0
            && now.saturating_sub(self.saved_automatically_at) >= SAVE_INTERVAL_SECONDS * 1000
    }

    /// Marks the automatic save as made.
    pub fn saved_automatically(&mut self, now: u64) {
        self.saved_automatically_at = now;
    }

    /// The report a running task sends. `None` when the config is silent.
    pub fn describe(&self, silent: bool) -> Option<crate::messages::Message> {
        if silent {
            return None;
        }
        Some(self.describe_always())
    }

    /// The report, whether or not the config is silent: a finished task always
    /// says so.
    pub fn describe_always(&self) -> crate::messages::Message {
        let world = self.world();
        if self.progress.complete {
            crate::messages::task_done(
                world,
                self.progress.count,
                self.progress.percent(),
                self.progress.time_parts(),
            )
        } else {
            crate::messages::task_update(
                world,
                self.progress.count,
                self.progress.percent(),
                self.progress.eta_parts(),
                self.progress.rate,
                self.progress.chunk_x,
                self.progress.chunk_z,
            )
        }
    }

    /// The `h:mm:ss` of a finished task.
    pub fn elapsed_text(&self) -> String {
        format_duration(self.progress.elapsed_seconds())
    }

    /// The saved file's `cancelled`: true for a task that is over, whether it
    /// was cancelled or completed, as Chunky writes it.
    pub fn saved_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }
}

/// The tasks of a server, one per world, as `Chunky.getGenerationTasks`.
#[derive(Default)]
pub struct Tasks {
    by_world: HashMap<String, Task>,
}

impl Tasks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.by_world.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_world.len()
    }

    pub fn get(&self, world: &str) -> Option<&Task> {
        self.by_world.get(world)
    }

    pub fn get_mut(&mut self, world: &str) -> Option<&mut Task> {
        self.by_world.get_mut(world)
    }

    pub fn contains(&self, world: &str) -> bool {
        self.by_world.contains_key(world)
    }

    /// Puts a task in, replacing whatever the world had.
    pub fn insert(&mut self, task: Task) {
        self.by_world.insert(task.selection.world.clone(), task);
    }

    pub fn remove(&mut self, world: &str) -> Option<Task> {
        self.by_world.remove(world)
    }

    pub fn clear(&mut self) {
        self.by_world.clear();
    }

    /// Every world with a task, in a stable order.
    pub fn worlds(&self) -> Vec<String> {
        let mut worlds: Vec<String> = self.by_world.keys().cloned().collect();
        worlds.sort();
        worlds
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Task)> {
        self.by_world.iter_mut()
    }

    /// The worlds with a task that has not settled.
    pub fn running(&self) -> Vec<String> {
        let mut worlds: Vec<String> = self
            .by_world
            .iter()
            .filter(|(_, task)| task.state.is_running())
            .map(|(world, _)| world.clone())
            .collect();
        worlds.sort();
        worlds
    }

    /// Whether a world has a task that has not settled, which is what stops a
    /// second `/chunky start` on it.
    pub fn is_running(&self, world: &str) -> bool {
        self.by_world
            .get(world)
            .is_some_and(|task| task.state.is_running())
    }
}

/// A task file: `<world with : replaced by />.properties`.
pub fn task_file(tasks_dir: &Path, world: &str) -> PathBuf {
    tasks_dir.join(format!("{}.properties", world.replace(':', "/")))
}

/// Writes a task the way `TaskLoader.saveTask` does: one `key=value` a line, in
/// `TaskProperty`'s order, each line only when the task has the value.
///
/// The keys are Chunky's own — `center-x`, `radius`, `radius-z`, `csv`,
/// `rate-limit`, `chunks`, `time` — so a file written here is Chunky's, and a
/// file Chunky wrote loads here.
pub fn save(task: &Task, tasks_dir: &Path) -> std::io::Result<PathBuf> {
    let path = task_file(tasks_dir, task.world());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let selection = &task.selection;
    let mut lines = vec![
        format!("world={}", selection.world),
        format!("cancelled={}", task.saved_cancelled()),
        format!("center-x={}", selection.center_x),
        format!("center-z={}", selection.center_z),
        format!("radius={}", selection.radius_x),
    ];
    if !task.owner.is_empty() {
        lines.insert(1, format!("owner={}", task.owner));
    }
    // Only the shapes with two radii have a second one to write.
    if matches!(selection.shape, ShapeKind::Rectangle | ShapeKind::Ellipse) {
        lines.push(format!("radius-z={}", selection.radius_z));
    }
    lines.push(format!("shape={}", selection.shape.name()));
    lines.push(format!("pattern={}", selection.pattern.kind.name()));
    if let Some(value) = &selection.pattern.value {
        lines.push(format!("csv={value}"));
    }
    lines.push(format!("rate-limit={}", task.rate_limit));
    lines.push(format!("chunks={}", task.progress.count));
    lines.push(format!("time={}", task.progress.elapsed_seconds()));
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&path, text)?;
    Ok(path)
}

/// Reads a task file back. Anything missing takes Chunky's default.
pub fn load(path: &Path) -> Option<Task> {
    let text = std::fs::read_to_string(path).ok()?;
    parse(&text)
}

/// Every saved task under a folder, in a stable order, one to a world.
///
/// The folder is walked, not listed: a world whose name has a namespace in it
/// is saved one folder down (`minecraft/overworld.properties`), as Chunky's
/// `Files.walk` finds.
pub fn load_all(tasks_dir: &Path) -> Vec<Task> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut dirs = vec![tasks_dir.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "properties")
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    let mut tasks: Vec<Task> = Vec::new();
    for task in paths.iter().filter_map(|path| load(path)) {
        // Chunky's map keeps the last file it read for a world.
        match tasks.iter_mut().find(|other| other.world() == task.world()) {
            Some(other) => *other = task,
            None => tasks.push(task),
        }
    }
    tasks
}

/// Parses a task file's text, as `TaskLoader.loadTask` reads one. Split out so
/// it can be tested without a disk.
///
/// A key that is missing or unreadable takes Chunky's default: the two radii
/// are one value (`radius-z` falls back to `radius`), the pattern is `region`
/// and the shape `square`. A file that names no world is Chunky's
/// `Optional.empty` — `None` here.
pub fn parse(text: &str) -> Option<Task> {
    let mut values: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim(), value.trim());
        }
    }
    let get = |key: &str| values.get(key).copied().unwrap_or("");
    let number = |key: &str, fallback: f64| get(key).parse::<f64>().ok().unwrap_or(fallback);
    let world = get("world");
    if world.is_empty() {
        return None;
    }

    let mut selection = Selection::new(world);
    selection.center_x = number("center-x", crate::selection::DEFAULT_CENTER);
    selection.center_z = number("center-z", crate::selection::DEFAULT_CENTER);
    selection.radius_x = number("radius", crate::selection::DEFAULT_RADIUS);
    selection.radius_z = number("radius-z", selection.radius_x);
    selection.shape = ShapeKind::parse_saved(get("shape")).unwrap_or(selection.shape);
    selection.rate_limit = get("rate-limit").parse().unwrap_or(0);
    let kind = if get("pattern").is_empty() {
        crate::pattern::PatternKind::Region
    } else {
        crate::pattern::PatternKind::parse(get("pattern"))
            .unwrap_or(crate::pattern::PatternKind::Region)
    };
    let value = (!get("csv").is_empty()).then(|| get("csv").to_string());
    selection.pattern = crate::pattern::Pattern::with_value(kind, value);

    let mut task = Task::new(selection);
    task.progress.count = get("chunks").parse().unwrap_or(0);
    task.progress.previous_seconds = get("time").parse().unwrap_or(0);
    task.owner = get("owner").to_string();
    task.state = if get("cancelled") == "true" {
        State::Cancelled
    } else {
        State::Paused
    };
    Some(task)
}

/// One line of a task's own log, `tasks.log`.
pub fn log_line(world: &str, text: &str) -> String {
    format!("[{world}] {text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::ShapeKind;

    fn a_selection() -> Selection {
        let mut selection = Selection::new("minecraft:overworld");
        selection.set_corners(-100.0, -100.0, 100.0, 100.0);
        selection
    }

    /// A cache with no world folder behind it, so nothing is on disk.
    fn a_cache() -> RegionCache {
        RegionCache::new(None)
    }

    /// A running task with its walk built and nothing on disk.
    fn a_running_task(selection: Selection) -> (Task, RegionCache) {
        let mut task = Task::new(selection);
        task.begin(0);
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .expect("a concentric walk needs nothing to read");
        (task, a_cache())
    }

    /// Loads the whole walk, saying which chunks had to be loaded.
    fn run(task: &mut Task, cache: &mut RegionCache) -> Vec<ChunkPos> {
        let mut loaded = Vec::new();
        while let Some(step) = task.next_to_load(0, cache) {
            if let Step::Load(chunk) = step {
                assert!(task.loaded(chunk, 0), "the task is holding {chunk:?}");
                loaded.push(chunk);
            }
        }
        loaded
    }

    #[test]
    fn a_stopped_task_may_be_continued_and_a_cancelled_one_may_not() {
        let mut task = Task::new(a_selection());
        task.begin(0);
        assert_eq!(task.state, State::Running);

        task.stop(false);
        assert_eq!(task.state, State::Paused);
        task.finish(false, 0);
        assert_eq!(task.state, State::Completed);
        assert!(task.saved_cancelled(), "a finished task is not continued");

        let mut task = Task::new(a_selection());
        task.begin(0);
        task.stop(true);
        assert_eq!(task.state, State::Cancelled);
        task.finish(true, 0);
        assert_eq!(task.state, State::Cancelled);
        assert!(task.state.is_cancelled());
    }

    #[test]
    fn a_cancel_that_arrives_while_pausing_wins() {
        let mut task = Task::new(a_selection());
        task.begin(0);
        task.stop(false);
        task.stop(true);
        assert_eq!(task.state, State::Cancelled);
    }

    #[test]
    fn the_progress_maths_is_chunkys() {
        let progress = Progress {
            count: 500,
            total: 1000,
            previous_seconds: 10,
            ..Default::default()
        };
        assert_eq!(progress.percent(), 50.0);
        assert_eq!(progress.elapsed_seconds(), 10);
        assert_eq!(progress.eta_parts(), ("0".into(), "00".into(), "00".into()));

        let done = Progress {
            count: 1000,
            total: 1000,
            complete: true,
            previous_seconds: 62,
            ..Default::default()
        };
        assert_eq!(done.percent(), 100.0);
        assert_eq!(done.time_parts(), ("0".into(), "01".into(), "02".into()));
    }

    #[test]
    fn an_empty_task_is_all_the_way_done() {
        assert_eq!(Progress::default().percent(), 100.0);
    }

    #[test]
    fn the_rate_limiter_hands_out_its_budget_a_second_at_a_time() {
        // 10 chunks a second, over a 30-second window.
        let mut limiter = RateLimiter::new(SAMPLE_INTERVAL_MILLIS, 10);
        // The window starts at the first question, which has run no time at
        // all, so the budget is nothing: one chunk is allowed with nothing in
        // flight, and not a second one. Chunky's own first call does the same.
        assert!(limiter.allows(1_000_000, 0));
        assert!(!limiter.allows(1_000_000, 1));

        // A second in, ten are allowed but not eleven.
        assert!(limiter.allows(1_001_000, 0));
        assert!(limiter.allows(1_001_000, 10));
        assert!(!limiter.allows(1_001_000, 11));

        // A chunk that finishes is counted against the window's budget itself,
        // so it does not hand its room back; it was one of the ten all along.
        let window = limiter.window();
        limiter.release(window, 1_001_000);
        assert!(
            !limiter.allows(1_001_000, 10),
            "ten done or in flight is the budget"
        );
        assert!(limiter.allows(1_001_000, 9));
        assert_eq!(
            limiter.rate(1_001_000),
            1.0,
            "one chunk finished in one second"
        );

        // Ten seconds in, a hundred are allowed and the window is the full
        // thirty seconds of a rate sample.
        assert!(limiter.allows(1_010_000, 99));
        assert_eq!(limiter.rate(1_010_000), 0.1, "one chunk in ten seconds");
    }

    #[test]
    fn a_rate_limiter_with_no_limit_allows_everything() {
        let mut limiter = RateLimiter::new(SAMPLE_INTERVAL_MILLIS, 0);
        assert!(limiter.allows(0, 0));
        assert!(limiter.allows(0, 100_000));
        assert_eq!(limiter.rate(0), 0.0);
    }

    #[test]
    fn changing_the_limit_restarts_the_window() {
        let mut limiter = RateLimiter::new(SAMPLE_INTERVAL_MILLIS, 10);
        assert!(limiter.allows(1_000_000, 0));
        assert!(!limiter.is_restarting());
        limiter.set_limit(100);
        assert!(limiter.is_restarting());
        // The window starts over at the new limit, so its budget is nothing
        // again however long the old one had been running.
        assert!(limiter.allows(1_001_000, 0));
        assert!(!limiter.allows(1_001_000, 1));
        assert!(!limiter.is_restarting());
    }

    #[test]
    fn a_release_from_an_old_window_is_forgotten() {
        let mut limiter = RateLimiter::new(SAMPLE_INTERVAL_MILLIS, 10);
        limiter.allows(1_000_000, 0);
        let old = limiter.window();
        limiter.release(old, 1_001_000);
        assert!(
            limiter.rate(1_002_000) > 0.0,
            "a release counts in its own window"
        );

        // A limit change starts a new window, and the old window's chunks are
        // no longer its own.
        limiter.set_limit(20);
        limiter.allows(1_003_000, 0);
        assert_ne!(old, limiter.window());
        limiter.release(old, 1_003_000);
        assert_eq!(
            limiter.rate(1_004_000),
            0.0,
            "a chunk from the window before the change is dropped"
        );
    }

    #[test]
    fn the_samples_average_over_thirty_seconds() {
        let mut samples = Samples::default();
        samples.record(0, true);
        assert_eq!(samples.count(), 1);
        // Within the same second the bin grows rather than a new one starting.
        samples.record(500, true);
        assert_eq!(samples.count(), 2);
        assert_eq!(samples.oldest(), Some(0));
        // A chunk that was not loaded does not count, but does start a bin.
        samples.record(2_000, false);
        assert_eq!(samples.count(), 2);
        // Past the window the old bins fall out.
        samples.record(40_000, true);
        assert_eq!(samples.count(), 1);
    }

    #[test]
    fn a_circle_counts_its_corner_chunks_without_loading_them() {
        let mut selection = Selection::new("minecraft:overworld");
        // Three chunks of radius, so the walk covers a 7x7 of chunks: a
        // circle's radius reaches the edge midpoints but not the corners.
        selection.radius_x = 48.0;
        selection.radius_z = 48.0;
        selection.shape = ShapeKind::Circle;
        let math = selection.math();
        assert_eq!(math.radius_chunks_x, 3);
        let (mut task, mut cache) = a_running_task(selection);
        assert_eq!(
            task.progress.total, 49,
            "the bounding 7x7 is what is walked"
        );
        let loaded = run(&mut task, &mut cache);

        assert_eq!(task.working_count(), 0);
        assert!(task.walk_exhausted());
        assert_eq!(task.progress.count, task.progress.total);
        assert_eq!(task.progress.percent(), 100.0);
        assert_eq!(
            loaded.len(),
            37,
            "the twelve corner chunks are counted, not loaded"
        );
        for corner in [(3, 3), (-3, 3), (3, -3), (-3, -3)] {
            let corner = ChunkPos::new(
                math.center_chunk_x + corner.0,
                math.center_chunk_z + corner.1,
            );
            assert!(
                !loaded.contains(&corner),
                "{corner:?} is outside the circle"
            );
        }
    }

    #[test]
    fn a_walk_loads_what_it_cannot_find_on_disk() {
        // A cache over a folder that is not there says nothing is generated, so
        // every chunk of the walk has to be loaded.
        let (mut task, mut cache) = a_running_task(a_selection());
        let loaded = run(&mut task, &mut cache);
        assert!(!loaded.is_empty());
        assert_eq!(loaded.len() as u64, task.progress.total);
    }

    #[test]
    fn a_paused_task_asks_for_nothing() {
        let mut task = Task::new(a_selection());
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .unwrap();
        let mut cache = a_cache();
        assert_eq!(task.next_to_load(0, &mut cache), None);
    }

    #[test]
    fn a_task_never_has_more_than_its_cap_in_flight() {
        let (mut task, mut cache) = a_running_task(a_selection());
        task.max_working_count = 4;
        let mut loaded = Vec::new();
        while let Some(step) = task.next_to_load(0, &mut cache) {
            if let Step::Load(chunk) = step {
                loaded.push(chunk);
            }
            assert!(task.working_count() <= 4);
        }
        assert_eq!(task.working_count(), 4);
        assert_eq!(task.next_to_load(0, &mut cache), None, "the cap is reached");
        // Finishing one makes room for the next.
        assert!(task.loaded(loaded[0], 0));
        assert!(task.next_to_load(0, &mut cache).is_some());
    }

    #[test]
    fn a_limit_holds_the_walk_back() {
        let mut selection = a_selection();
        selection.rate_limit = 1;
        let mut task = Task::new(selection);
        task.begin(0);
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .unwrap();
        let mut cache = a_cache();
        // The window has run no time at all, so one chunk may start and no
        // more.
        let Some(Step::Load(first)) = task.next_to_load(1_000_000, &mut cache) else {
            panic!("the first chunk of the walk is a load");
        };
        assert_eq!(task.next_to_load(1_000_000, &mut cache), None);
        // A second in, the chunk has finished, and the budget is one again.
        assert!(task.loaded(first, 1_001_000));
        let Some(Step::Load(second)) = task.next_to_load(1_001_000, &mut cache) else {
            panic!("the second chunk of the walk is a load");
        };
        assert!(task.loaded(second, 1_001_000));
        // Its budget is spent, and the next second has not arrived.
        assert_eq!(task.next_to_load(1_001_000, &mut cache), None);
        assert!(task.next_to_load(1_002_000, &mut cache).is_some());
    }

    #[test]
    fn a_limit_does_not_hold_back_the_chunks_that_need_no_loading() {
        // A chunk the session has already seen loaded takes no permit, as
        // Chunky's loop `continue`s before it asks for one: with a limit of one
        // chunk a second and no time gone by at all, the whole walk of a
        // selection that is all there still goes by in one call.
        let mut selection = Selection::new("minecraft:overworld");
        selection.radius_x = 48.0;
        selection.radius_z = 48.0;
        selection.rate_limit = 1;
        let mut task = Task::new(selection.clone());
        task.begin(0);
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .unwrap();
        let mut cache = a_cache();
        for x in -3..=3 {
            for z in -3..=3 {
                cache.set_generated(ChunkPos::new(x, z));
            }
        }

        // The one call walks the whole selection: every chunk is passed over
        // and counted, and the walk ends without a chunk ever being held.
        assert_eq!(task.next_to_load(1_000_000, &mut cache), None);
        assert_eq!(task.progress.count, task.progress.total);
        assert!(task.walk_exhausted());
        assert_eq!(task.working_count(), 0, "no chunk was ever held");
        assert_eq!(
            task.rate_limiter().rate(1_001_000),
            0.0,
            "and no permit was spent"
        );
        // A walk longer than one call's own bound is handed back in pieces.
        selection.radius_x = 2_000.0;
        selection.radius_z = 2_000.0;
        let mut task = Task::new(selection);
        task.begin(0);
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .unwrap();
        let mut cache = a_cache();
        for x in -125..=125 {
            for z in -125..=125 {
                cache.set_generated(ChunkPos::new(x, z));
            }
        }
        assert_eq!(
            task.next_to_load(1_000_000, &mut cache),
            Some(Step::Counted),
            "a call stops after COUNTED_PER_CALL chunks and hands back"
        );
        assert_eq!(task.progress.count, COUNTED_PER_CALL);
        assert_eq!(task.working_count(), 0);
    }

    #[test]
    fn the_rate_of_a_limited_task_follows_the_limit() {
        let mut selection = a_selection();
        selection.rate_limit = 20;
        let mut task = Task::new(selection);
        task.begin(0);
        task.build_chunks(crate::pattern::Sources::default(), |_, _| true)
            .unwrap();
        let mut cache = a_cache();
        let mut now = 1_000_000;
        for _ in 0..20 {
            let Some(Step::Load(chunk)) = task.next_to_load(now, &mut cache) else {
                panic!("a chunk should be there to load");
            };
            assert!(task.loaded(chunk, now));
            // Four ticks a second, as the game runs.
            now += 50;
        }
        // A second of a twenty-a-second limit: the rate has caught up.
        assert_eq!(task.rate_limiter().rate(now), 20.0);
    }

    #[test]
    fn a_task_saves_and_loads_as_chunkys_file() {
        let dir = std::env::temp_dir().join("chunky-horizon-task-round-trip");
        let _ = std::fs::remove_dir_all(&dir);

        let mut task = Task::new(a_selection());
        task.progress.count = 7;
        task.progress.total = 121;
        task.owner = "ender".to_string();
        task.set_rate_limit(32);
        task.selection.rate_limit = 32;
        save(&task, &dir).unwrap();

        let path = task_file(&dir, "minecraft:overworld");
        // The colon becomes a folder, as Chunky's loader writes it.
        assert!(path.ends_with("minecraft/overworld.properties"), "{path:?}");
        let raw = std::fs::read_to_string(&path).unwrap();
        // The keys, in TaskProperty's order, as Chunky writes them.
        let keys: Vec<&str> = raw
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            keys,
            vec![
                "world",
                "owner",
                "cancelled",
                "center-x",
                "center-z",
                "radius",
                "shape",
                "pattern",
                "rate-limit",
                "chunks",
                "time",
            ],
            "{raw}"
        );
        assert!(raw.contains("world=minecraft:overworld"), "{raw}");
        assert!(raw.contains("shape=square"), "{raw}");
        assert!(raw.contains("chunks=7"), "{raw}");
        assert!(raw.contains("cancelled=false"), "{raw}");
        // A square has one radius, so there is no `radius-z` line.
        assert!(!raw.contains("radius-z"), "{raw}");

        let back = load(&path).unwrap();
        assert_eq!(back.world(), "minecraft:overworld");
        assert_eq!(back.progress.count, 7);
        assert_eq!(back.owner, "ender");
        assert_eq!(back.rate_limit, 32);
        assert_eq!(back.selection.shape, ShapeKind::Square);
        assert_eq!(back.selection.radius_x, task.selection.radius_x);
        assert_eq!(back.state, State::Paused);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_ellipse_writes_both_radii() {
        let dir = std::env::temp_dir().join("chunky-horizon-task-ellipse");
        let _ = std::fs::remove_dir_all(&dir);
        let mut selection = Selection::new("minecraft:overworld");
        selection.set_radii(100.0, 200.0);
        selection.shape = ShapeKind::Ellipse;
        save(&Task::new(selection), &dir).unwrap();

        let raw = std::fs::read_to_string(task_file(&dir, "minecraft:overworld")).unwrap();
        assert!(raw.contains("radius=100"), "{raw}");
        assert!(raw.contains("radius-z=200"), "{raw}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_finished_task_is_written_as_cancelled() {
        let mut task = Task::new(a_selection());
        task.finish(false, 0);
        assert!(task.saved_cancelled());
        task.finish(true, 0);
        assert!(task.saved_cancelled());
    }

    #[test]
    fn a_task_file_with_odd_values_still_loads() {
        let text = "world=world_nether\nshape=nonsense\ncenter-x=abc\nradius=-5\n";
        let task = parse(text).unwrap();
        assert_eq!(task.world(), "world_nether");
        // A shape that does not parse keeps the default, and so does a centre.
        assert_eq!(task.selection.shape, ShapeKind::Square);
        assert_eq!(task.selection.center_x, 0.0);
        assert_eq!(task.selection.radius_x, -5.0);
        // One radius means both, and a pattern nobody named is `region`.
        assert_eq!(task.selection.radius_z, -5.0);
        assert_eq!(
            task.selection.pattern.kind,
            crate::pattern::PatternKind::Region
        );
    }

    #[test]
    fn a_file_with_no_world_is_no_task() {
        assert!(parse("shape=circle\n").is_none());
    }

    #[test]
    fn an_oval_in_an_old_task_file_is_an_ellipse() {
        let task = parse("world=minecraft:overworld\nshape=oval\n").unwrap();
        assert_eq!(task.selection.shape, ShapeKind::Ellipse);
    }

    #[test]
    fn tasks_are_found_under_a_worlds_own_folder() {
        let dir = std::env::temp_dir().join("chunky-horizon-task-tree");
        let _ = std::fs::remove_dir_all(&dir);
        let mut overworld = Task::new(a_selection());
        overworld.progress.count = 1;
        save(&overworld, &dir).unwrap();
        // A world whose name carries a namespace is saved one folder down.
        let mut nether = Task::new(Selection::new("minecraft:the_nether"));
        nether.progress.count = 2;
        save(&nether, &dir).unwrap();

        let mut tasks = load_all(&dir);
        tasks.sort_by(|a, b| a.world().cmp(b.world()));
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].world(), "minecraft:overworld");
        assert_eq!(tasks[0].progress.count, 1);
        assert_eq!(tasks[1].world(), "minecraft:the_nether");
        assert_eq!(tasks[1].progress.count, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tasks_are_kept_one_to_a_world() {
        let mut tasks = Tasks::new();
        let mut task = Task::new(a_selection());
        task.begin(0);
        tasks.insert(task);
        assert!(tasks.is_running("minecraft:overworld"));
        assert_eq!(tasks.running(), vec!["minecraft:overworld".to_string()]);

        // A paused task is not running, so running() no longer includes it.
        tasks.get_mut("minecraft:overworld").unwrap().stop(false);
        assert!(!tasks.is_running("minecraft:overworld"));
        assert!(tasks.running().is_empty());
    }
}
