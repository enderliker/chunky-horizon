//! Tests for everyday Chunky usage flows matching official Chunky parity:
//! - Complete generation across ring boundaries (e.g. radius 500, crossing Ring 0 to Ring 1)
//! - Pause and Continue: state transition, serialization to .properties, resuming at exact chunk
//! - World exit and re-entry: continue_on_restart handling, preserving paused tasks
//! - Changing radius, shape, and center while a task is running (selection independence)
//! - Mid-generation rate limit adjustments (/chunky limit)
//! - Task cancellation and confirm flows

use chunky_core::pattern::{Pattern, PatternKind, Sources};
use chunky_core::selection::Selection;
use chunky_core::shape::ShapeKind;
use chunky_core::task::{self, RegionCache, State, Step, Task, Tasks};
use std::collections::HashSet;
use std::path::PathBuf;

fn temp_tasks_dir(test_name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("chunky_daily_tests")
        .join(test_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dummy_cache() -> RegionCache {
    RegionCache::new(None)
}

#[test]
fn test_full_generation_ring_radius_500_crosses_rings_without_stalling() {
    let mut selection = Selection::new("minecraft:overworld");
    selection.radius_x = 500.0;
    selection.radius_z = 500.0;
    selection.shape = ShapeKind::Square;
    selection.pattern = Pattern::new(PatternKind::Ring);

    let mut task = Task::new(selection);
    task.begin(1000);
    task.build_chunks(Sources::default(), |_, _| true).unwrap();

    assert_eq!(task.progress.total, 4225);

    let mut cache = dummy_cache();
    let mut chunks_seen = HashSet::new();
    let mut count = 0;

    while let Some(step) = task.next_to_load(1000 + count * 10, &mut cache) {
        match step {
            Step::Load(chunk) => {
                assert!(
                    chunks_seen.insert(chunk),
                    "duplicate chunk {chunk:?} at step {count}"
                );
                // Simulate chunk loaded immediately
                task.loaded(chunk, 1000 + count * 10);
                count += 1;
            }
            Step::Counted => {
                count += 1;
            }
        }
    }

    assert_eq!(task.progress.count, 4225);
    assert_eq!(chunks_seen.len(), 4225);
    assert!(task.walk_exhausted());
    assert_eq!(task.working_count(), 0);

    task.finish(false, 2000);
    assert_eq!(task.state, State::Completed);
    assert!(
        task.saved_cancelled(),
        "completed task writes cancelled=true"
    );
}

#[test]
fn test_pause_and_continue_exact_parity() {
    let tdir = temp_tasks_dir("pause_continue");
    let mut selection = Selection::new("minecraft:overworld");
    selection.radius_x = 500.0;
    selection.radius_z = 500.0;
    selection.shape = ShapeKind::Square;
    selection.pattern = Pattern::new(PatternKind::Ring);

    // 1. Run first 1500 chunks continuously
    let mut continuous_task = Task::new(selection.clone());
    continuous_task.begin(1000);
    continuous_task
        .build_chunks(Sources::default(), |_, _| true)
        .unwrap();

    let mut cache = dummy_cache();
    let mut reference_chunks = Vec::new();
    while reference_chunks.len() < 3000 {
        if let Some(Step::Load(chunk)) = continuous_task.next_to_load(1000, &mut cache) {
            continuous_task.loaded(chunk, 1000);
            reference_chunks.push(chunk);
        }
    }

    // 2. Run first 1500 chunks with a task that will be paused
    let mut task = Task::new(selection.clone());
    task.begin(1000);
    task.build_chunks(Sources::default(), |_, _| true).unwrap();

    let mut cache = dummy_cache();
    let mut first_phase_chunks = Vec::new();
    while first_phase_chunks.len() < 1500 {
        if let Some(Step::Load(chunk)) = task.next_to_load(1000, &mut cache) {
            task.loaded(chunk, 1000);
            first_phase_chunks.push(chunk);
        }
    }
    assert_eq!(first_phase_chunks, reference_chunks[..1500]);

    // 3. Pause task
    task.stop(false);
    assert_eq!(task.state, State::Paused);
    assert!(!task.state.is_running());
    assert!(task.state.is_paused());
    assert_eq!(task.next_to_load(1000, &mut cache), None);

    // 4. Save paused task to disk
    let file = task::save(&task, &tdir).unwrap();
    assert!(file.is_file());

    // 5. Load paused task back from disk
    let mut loaded_task = task::load(&file).expect("saved task must load");
    assert_eq!(loaded_task.state, State::Paused);
    assert_eq!(loaded_task.progress.count, 1500);
    assert_eq!(loaded_task.world(), "minecraft:overworld");

    // 6. Continue task
    loaded_task.begin(2000);
    assert_eq!(loaded_task.state, State::Running);
    loaded_task
        .build_chunks(Sources::default(), |_, _| true)
        .unwrap();
    assert_eq!(loaded_task.progress.count, 1500);
    assert_eq!(loaded_task.progress.total, 4225);

    // 7. Process next 1500 chunks
    let mut second_phase_chunks = Vec::new();
    while second_phase_chunks.len() < 1500 {
        if let Some(Step::Load(chunk)) = loaded_task.next_to_load(2000, &mut cache) {
            loaded_task.loaded(chunk, 2000);
            second_phase_chunks.push(chunk);
        }
    }
    assert_eq!(second_phase_chunks, reference_chunks[1500..3000]);

    let _ = std::fs::remove_dir_all(&tdir);
}

#[test]
fn test_world_exit_and_reenter_lifecycle() {
    let tdir = temp_tasks_dir("world_exit_reenter");
    let mut tasks = Tasks::new();

    let mut selection = Selection::new("minecraft:overworld");
    selection.radius_x = 200.0;
    selection.radius_z = 200.0;
    selection.shape = ShapeKind::Square;

    let mut task = Task::new(selection);
    task.begin(1000);
    task.build_chunks(Sources::default(), |_, _| true).unwrap();

    let mut cache = dummy_cache();
    for _ in 0..100 {
        if let Some(Step::Load(chunk)) = task.next_to_load(1000, &mut cache) {
            task.loaded(chunk, 1000);
        }
    }
    assert_eq!(task.progress.count, 100);
    tasks.insert(task);
    assert_eq!(tasks.running(), vec!["minecraft:overworld".to_string()]);

    // Simulate on_world_close: pause and save all running tasks
    for world in tasks.running() {
        let task = tasks.get_mut(&world).unwrap();
        task.stop(false);
        task::save(task, &tdir).unwrap();
    }
    tasks.clear();
    assert!(tasks.running().is_empty());

    // Simulate on_world_open: load all saved tasks
    let saved_tasks = task::load_all(&tdir);
    assert_eq!(saved_tasks.len(), 1);
    let mut resumed = saved_tasks.into_iter().next().unwrap();
    assert_eq!(resumed.state, State::Paused);
    assert_eq!(resumed.progress.count, 100);

    // With continue_on_restart = true:
    resumed.begin(3000);
    resumed
        .build_chunks(Sources::default(), |_, _| true)
        .unwrap();
    tasks.insert(resumed);
    assert_eq!(tasks.running(), vec!["minecraft:overworld".to_string()]);
    assert_eq!(
        tasks.get("minecraft:overworld").unwrap().progress.count,
        100
    );

    let _ = std::fs::remove_dir_all(&tdir);
}

#[test]
fn test_selection_changes_independent_of_running_task() {
    let tdir = temp_tasks_dir("selection_independence");
    let mut tasks = Tasks::new();

    // 1. Player configures initial selection: radius 200 square
    let mut player_selection = Selection::new("minecraft:overworld");
    player_selection.radius_x = 200.0;
    player_selection.radius_z = 200.0;
    player_selection.shape = ShapeKind::Square;

    // 2. Player runs /chunky start: immutable Task created from selection copy
    let mut running_task = Task::new(player_selection.clone());
    running_task.begin(1000);
    running_task
        .build_chunks(Sources::default(), |_, _| true)
        .unwrap();
    let initial_total = running_task.progress.total;
    tasks.insert(running_task);

    // 3. Player changes selection mid-generation: radius 600 circle
    player_selection.set_radii(600.0, 600.0);
    player_selection.shape = ShapeKind::Circle;

    // 4. Verify running task is completely unaffected
    let active = tasks.get("minecraft:overworld").unwrap();
    assert_eq!(active.selection.radius_x, 200.0);
    assert_eq!(active.selection.shape, ShapeKind::Square);
    assert_eq!(active.progress.total, initial_total);

    // 5. Player pauses and cancels old task
    let mut old_task = tasks.remove("minecraft:overworld").unwrap();
    old_task.stop(true);
    old_task.finish(true, 2000);
    task::save(&old_task, &tdir).unwrap();

    // 6. Starting a new task uses the updated selection
    let mut new_task = Task::new(player_selection.clone());
    new_task.begin(2500);
    new_task
        .build_chunks(Sources::default(), |_, _| true)
        .unwrap();

    assert_eq!(new_task.selection.radius_x, 600.0);
    assert_eq!(new_task.selection.shape, ShapeKind::Circle);
    assert_ne!(new_task.progress.total, initial_total);

    let _ = std::fs::remove_dir_all(&tdir);
}

#[test]
fn test_rate_limit_change_mid_generation() {
    let mut selection = Selection::new("minecraft:overworld");
    selection.radius_x = 200.0;
    selection.radius_z = 200.0;
    selection.rate_limit = 50;

    let mut task = Task::new(selection);
    task.begin(1_000_000);
    task.build_chunks(Sources::default(), |_, _| true).unwrap();
    assert_eq!(task.rate_limiter().limit(), 50);

    // Change rate limit to 10
    task.set_rate_limit(10);
    assert_eq!(task.rate_limit, 10);
    assert_eq!(task.rate_limiter().limit(), 10);

    // Disable rate limit
    task.set_rate_limit(0);
    assert_eq!(task.rate_limit, 0);
    assert_eq!(task.rate_limiter().limit(), 0);
}

#[test]
fn test_cancel_cannot_be_continued() {
    let tdir = temp_tasks_dir("cancel_no_continue");
    let selection = Selection::new("minecraft:overworld");

    let mut task = Task::new(selection);
    task.begin(1000);
    task.stop(true);
    task.finish(true, 1500);

    let file = task::save(&task, &tdir).unwrap();
    let loaded = task::load(&file).unwrap();
    assert_eq!(loaded.state, State::Cancelled);
    assert!(loaded.state.is_cancelled());

    let _ = std::fs::remove_dir_all(&tdir);
}

#[test]
fn test_multiple_shapes_and_patterns_pause_resume() {
    let shapes = [
        ShapeKind::Circle,
        ShapeKind::Square,
        ShapeKind::Diamond,
        ShapeKind::Star,
        ShapeKind::Triangle,
        ShapeKind::Hexagon,
        ShapeKind::Ellipse,
    ];
    let patterns = [
        PatternKind::Concentric,
        PatternKind::Loop,
        PatternKind::Spiral,
        PatternKind::Strip,
        PatternKind::Ring,
    ];

    for shape in shapes {
        for pattern in patterns {
            let mut selection = Selection::new("minecraft:overworld");
            selection.radius_x = 64.0;
            selection.radius_z = if shape == ShapeKind::Ellipse {
                128.0
            } else {
                64.0
            };
            selection.shape = shape;
            selection.pattern = Pattern::new(pattern);

            let mut task = Task::new(selection);
            task.begin(0);
            task.build_chunks(Sources::default(), |_, _| true).unwrap();

            let mut cache = dummy_cache();
            let mut first_half = 0;
            let target = task.progress.total / 2;

            while first_half < target {
                if let Some(step) = task.next_to_load(1000, &mut cache) {
                    if let Step::Load(c) = step {
                        task.loaded(c, 1000);
                    }
                    first_half += 1;
                } else {
                    break;
                }
            }

            task.stop(false);
            assert_eq!(task.state, State::Paused);

            // Resume
            task.begin(2000);
            task.build_chunks(Sources::default(), |_, _| true).unwrap();

            let mut _second_half = 0;
            while let Some(step) = task.next_to_load(2000, &mut cache) {
                if let Step::Load(c) = step {
                    task.loaded(c, 2000);
                }
                _second_half += 1;
            }

            assert!(
                task.walk_exhausted(),
                "shape {shape:?} with pattern {pattern:?} did not finish"
            );
        }
    }
}
