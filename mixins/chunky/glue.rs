// The bridge between Horizon and chunky_core:
// commands, server tick driving chunk pre-generation, chunk loading feedback,
// settings, and world persistence.

use crate::game::Game;
use chunky_core::{
    anvil::RegionFile,
    command::{self, Action, ParseError},
    config::{self, Config},
    geometry::ChunkPos,
    messages,
    pattern::{Pattern, Sources},
    selection::Selection,
    shape::{Bounding, ShapeKind},
    task::{self, RegionCache, Step, Task, Tasks},
    trim::{self, TrimFilter},
};
use horizon_client::text::Text;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub(super) enum Sender {
    Client,
    Player(u128),
}

impl Sender {
    pub(super) fn send(&self, game: &mut Game<'_>, msg: &str) {
        match self {
            Sender::Client => game.native_chat(msg),
            Sender::Player(uuid) => {
                game.native_tell(*uuid, Text::literal(msg));
            }
        }
    }
}

enum PendingAction {
    Trim {
        world: String,
        shape: ShapeKind,
        bounds: Bounding,
        inside: bool,
        filters: Vec<TrimFilter>,
    },
}

#[derive(Default)]
struct State {
    config: Config,
    config_loaded: bool,
    tasks: Tasks,
    selections: HashMap<String, Selection>,
    active_world: String,
    caches: HashMap<String, RegionCache>,
    pending: Option<(PendingAction, Instant)>,
}

impl State {
    fn selection_for(&mut self, world: &str) -> &mut Selection {
        self.selections
            .entry(world.to_string())
            .or_insert_with(|| Selection::new(world))
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(State::default);
    f(state)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn region_dir(world_dir: &Path, dimension: &str) -> PathBuf {
    let dim_path = horizon_client::save::dimension_dir(world_dir, dimension);
    let dim_region = dim_path.join("region");
    if dim_region.is_dir() {
        return dim_region;
    }
    let root_region = world_dir.join("region");
    if root_region.is_dir() && dimension == "minecraft:overworld" {
        return root_region;
    }
    dim_region
}

fn tasks_dir(world_dir: Option<&Path>, data_dir: &Path) -> PathBuf {
    world_dir
        .map(|d| d.join(task::TASKS_DIR))
        .unwrap_or_else(|| data_dir.join(task::TASKS_DIR))
}

fn load_config_if_needed(_game: &Game<'_>, state: &mut State) {
    if state.config_loaded {
        return;
    }
    if let Ok(dir) = Game::native_config_dir() {
        let path = config::Config::path_in(&dir);
        state.config = Config::load(&path);
        state.config_loaded = true;
    }
}

fn save_config(_game: &Game<'_>, config: &Config) {
    if let Ok(dir) = Game::native_config_dir() {
        let path = config::Config::path_in(&dir);
        let _ = config.save(&path);
    }
}

pub(super) fn handle_command(game: &mut Game<'_>, sender: Sender, line: &str) {
    with_state(|state| load_config_if_needed(game, state));

    let action = match command::parse(line) {
        Ok(action) => action,
        Err(err) => {
            let msg = match err {
                ParseError::Usage(sub) => messages::help(sub).line(),
                ParseError::Unknown => messages::help("help").line(),
                ParseError::NotFound(command::NotFound::Shape(s)) => {
                    messages::shape_not_found(&s, &command::shape_names()).line()
                }
                ParseError::NotFound(command::NotFound::Pattern(p)) => {
                    messages::pattern_not_found(&p, &command::pattern_names()).line()
                }
                ParseError::TrimFilter(t) => {
                    messages::trim_filter_invalid(&t, &trim::filter_keys()).line()
                }
                ParseError::TrimUsage => messages::help("trim").line(),
            };
            sender.send(game, &msg);
            return;
        }
    };

    match action {
        Action::Help { page } => {
            let page = page.unwrap_or(1).max(1);
            let key = format!("help_{page}");
            let msg = messages::help(&key);
            sender.send(game, &msg.line());
        }
        Action::Center { x, z } => {
            let (target_x, target_z) = match (x, z) {
                (Some(cx), Some(cz)) => (cx, cz),
                _ => {
                    let player_pos = match sender {
                        Sender::Client => game.native_player().map(|p| (p.x, p.z)),
                        Sender::Player(uuid) => game
                            .native_server_players()
                            .into_iter()
                            .find(|p| p.uuid == uuid)
                            .map(|p| (p.position[0], p.position[2])),
                    };
                    match player_pos {
                        Some(pos) => pos,
                        None => {
                            sender.send(game, "Not in a world or no player position available.");
                            return;
                        }
                    }
                }
            };

            let default_dim = game.dim.id().to_string();
            with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.center_x = target_x;
                sel.center_z = target_z;
            });
            sender.send(game, &messages::center(target_x, target_z).line());
        }
        Action::Corners { x1, z1, x2, z2 } => {
            let default_dim = game.dim.id().to_string();
            let (rx, rz) = with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.set_corners(x1, z1, x2, z2);
                (sel.radius_x, sel.radius_z)
            });
            sender.send(game, &messages::radii(rx, rz).line());
        }
        Action::Radius { radius_x, radius_z } => {
            let default_dim = game.dim.id().to_string();
            let (rx, rz) = with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                let new_rx = match radius_x {
                    Some((val, Some(1))) => sel.radius_x + val,
                    Some((val, Some(-1))) => (sel.radius_x - val).max(0.0),
                    Some((val, _)) => val,
                    None => sel.radius_x,
                };
                let new_rz = match radius_z {
                    Some((val, Some(1))) => sel.radius_z + val,
                    Some((val, Some(-1))) => (sel.radius_z - val).max(0.0),
                    Some((val, _)) => val,
                    None => {
                        if matches!(sel.shape, ShapeKind::Rectangle | ShapeKind::Ellipse) {
                            sel.radius_z
                        } else {
                            new_rx
                        }
                    }
                };
                sel.set_radii(new_rx, new_rz);
                (sel.radius_x, sel.radius_z)
            });
            if rx == rz {
                sender.send(game, &messages::radius(rx).line());
            } else {
                sender.send(game, &messages::radii(rx, rz).line());
            }
        }
        Action::Shape { name } => {
            let default_dim = game.dim.id().to_string();
            let res = with_state(|state| {
                let shape = chunky_core::shape::SHAPES
                    .iter()
                    .find(|s| s.name().eq_ignore_ascii_case(&name))
                    .copied();
                let Some(shape) = shape else {
                    return Err(messages::shape_not_found(&name, &command::shape_names()).line());
                };
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.shape = shape.with_radii(sel.radius_x, sel.radius_z);
                Ok(messages::shape(sel.shape).line())
            });
            match res {
                Ok(msg) => sender.send(game, &msg),
                Err(err) => sender.send(game, &err),
            }
        }
        Action::Pattern { name, value } => {
            let default_dim = game.dim.id().to_string();
            let res = with_state(|state| {
                let pattern = chunky_core::pattern::PATTERNS
                    .iter()
                    .find(|p| p.name().eq_ignore_ascii_case(&name))
                    .copied();
                let Some(pattern) = pattern else {
                    return Err(
                        messages::pattern_not_found(&name, &command::pattern_names()).line(),
                    );
                };
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.pattern = Pattern {
                    kind: pattern,
                    value,
                };
                Ok(messages::pattern(pattern.name()).line())
            });
            match res {
                Ok(msg) => sender.send(game, &msg),
                Err(err) => sender.send(game, &err),
            }
        }
        Action::World { name } => {
            let world_name = name.unwrap_or_else(|| game.dim.id().to_string());
            with_state(|state| {
                state.active_world = world_name.clone();
                let _ = state.selection_for(&world_name);
            });
            sender.send(game, &messages::world(&world_name).line());
        }
        Action::Spawn => {
            let default_dim = game.dim.id().to_string();
            let spawn = game.level.spawn.unwrap_or([0.0, 64.0, 0.0]);
            with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.center_x = spawn[0];
                sel.center_z = spawn[2];
            });
            sender.send(game, &messages::center(spawn[0], spawn[2]).line());
        }
        Action::WorldBorder => {
            let default_dim = game.dim.id().to_string();
            let target_world = with_state(|state| {
                if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                }
            });
            if let Some(bounds) = game.native_world_border(&target_world) {
                let center_x = (bounds[0] + bounds[2]) / 2.0;
                let center_z = (bounds[1] + bounds[3]) / 2.0;
                let radius_x = (bounds[2] - bounds[0]).abs() / 2.0;
                let radius_z = (bounds[3] - bounds[1]).abs() / 2.0;
                with_state(|state| {
                    let sel = state.selection_for(&target_world);
                    sel.center_x = center_x;
                    sel.center_z = center_z;
                    sel.set_radii(radius_x, radius_z);
                });
                sender.send(game, &messages::border_load(&target_world).line());
            } else {
                sender.send(game, &messages::border_no_border(&target_world).line());
            }
        }
        Action::Selection => {
            let default_dim = game.dim.id().to_string();
            with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world).clone();
                sender.send(game, &messages::selection().line());
                sender.send(game, &messages::selection_world(&sel.world).line());
                sender.send(game, &messages::selection_shape(sel.shape).line());
                sender.send(
                    game,
                    &messages::selection_center(sel.center_x, sel.center_z).line(),
                );
                if sel.radius_x == sel.radius_z {
                    sender.send(game, &messages::selection_radius(&sel.radius_text()).line());
                } else {
                    sender.send(
                        game,
                        &messages::selection_radii(sel.radius_x, sel.radius_z).line(),
                    );
                }
                sender.send(game, &messages::pattern(&sel.pattern_name()).line());
            });
        }
        Action::Quiet { seconds } => {
            let sec = seconds.max(0) as u64;
            with_state(|state| {
                state.config.update_interval = sec;
                save_config(game, &state.config);
            });
            sender.send(game, &messages::quiet(sec).line());
        }
        Action::Silent => {
            let silent = with_state(|state| {
                state.config.silent = !state.config.silent;
                save_config(game, &state.config);
                state.config.silent
            });
            sender.send(game, &messages::silent(silent).line());
        }
        Action::Limit { rate } => {
            let rate_val = rate.max(0) as u64;
            let default_dim = game.dim.id().to_string();
            let world = with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                let sel = state.selection_for(&world);
                sel.rate_limit = rate_val;
                if let Some(task) = state.tasks.get_mut(&world) {
                    task.set_rate_limit(rate_val);
                }
                world
            });
            if rate_val == 0 {
                sender.send(game, &messages::limit_disabled(&world).line());
            } else {
                sender.send(game, &messages::limit(&world, rate_val).line());
            }
        }
        Action::Start(args) => {
            let default_dim = game.dim.id().to_string();
            let world = args.world.clone().unwrap_or_else(|| {
                with_state(|state| {
                    if state.active_world.is_empty() {
                        default_dim.clone()
                    } else {
                        state.active_world.clone()
                    }
                })
            });

            let already_running = with_state(|state| state.tasks.is_running(&world));
            if already_running {
                sender.send(game, &messages::started_already(&world).line());
                return;
            }

            let mut sel = with_state(|state| state.selection_for(&world).clone());
            sel.world = world.clone();
            if let Some(shape_str) = args.shape {
                if let Some(s) = chunky_core::shape::SHAPES
                    .iter()
                    .find(|s| s.name().eq_ignore_ascii_case(&shape_str))
                {
                    sel.shape = *s;
                }
            }
            if let Some((cx, cz)) = args.center {
                sel.center_x = cx;
                sel.center_z = cz;
            }
            if let Some(rx) = args.radius_x {
                let rz = args.radius_z.unwrap_or(rx);
                sel.set_radii(rx, rz);
            }
            let force_load = with_state(|state| state.config.force_load_existing_chunks);
            sel.force_load_existing_chunks = force_load;

            let mut task = Task::new(sel.clone());
            let now = now_millis();
            let world_dir = game.native_world_dir();
            let data_dir = game.native_data_dir(chunky_core::ID);
            let config_dir = Game::native_config_dir().ok();
            let reg_dir = world_dir.as_deref().map(|d| region_dir(d, &world));
            let sources = Sources {
                data_dir: Some(&data_dir),
                config_dir: config_dir.as_deref(),
                region_dir: reg_dir.as_deref(),
            };
            if let Err(e) = task.build_chunks(sources, |_, _| true) {
                sender.send(game, &format!("Failed to build generation task: {e:?}"));
                return;
            }
            task.begin(now);

            let rad_str = if sel.radius_x == sel.radius_z {
                chunky_core::geometry::format_number(sel.radius_x)
            } else {
                format!(
                    "{}, {}",
                    chunky_core::geometry::format_number(sel.radius_x),
                    chunky_core::geometry::format_number(sel.radius_z)
                )
            };
            let start_msg =
                messages::start(&world, sel.shape, sel.center_x, sel.center_z, &rad_str).line();

            let tdir = tasks_dir(world_dir.as_deref(), &data_dir);
            let _ = task::save(&task, &tdir);

            with_state(|state| {
                state.tasks.insert(task);
            });
            sender.send(game, &start_msg);
        }
        Action::Pause { world } => {
            let default_dim = game.dim.id().to_string();
            let target = world.unwrap_or_else(|| {
                with_state(|state| {
                    if state.active_world.is_empty() {
                        default_dim.clone()
                    } else {
                        state.active_world.clone()
                    }
                })
            });

            let world_dir = game.native_world_dir();
            let data_dir = game.native_data_dir(chunky_core::ID);
            let tdir = tasks_dir(world_dir.as_deref(), &data_dir);

            let paused = with_state(|state| {
                if let Some(task) = state.tasks.get_mut(&target) {
                    task.stop(false);
                    let held = task.held();
                    let _ = task::save(task, &tdir);
                    Some(held)
                } else {
                    None
                }
            });

            if let Some(held) = paused {
                let owner = format!("chunky:task/{target}");
                for chunk in held {
                    game.native_force_chunk_in(&owner, &target, [chunk.x, chunk.z], false);
                }
                sender.send(game, &messages::pause(&target).line());
            } else {
                sender.send(game, &messages::pause_no_tasks().line());
            }
        }
        Action::Continue { world } => {
            let default_dim = game.dim.id().to_string();
            let target = world.unwrap_or_else(|| {
                with_state(|state| {
                    if state.active_world.is_empty() {
                        default_dim.clone()
                    } else {
                        state.active_world.clone()
                    }
                })
            });

            let world_dir = game.native_world_dir();
            let data_dir = game.native_data_dir(chunky_core::ID);
            let tdir = tasks_dir(world_dir.as_deref(), &data_dir);

            let mut loaded_task = with_state(|state| state.tasks.remove(&target));
            if loaded_task.is_none() {
                let tfile = task::task_file(&tdir, &target);
                loaded_task = task::load(&tfile);
            }

            if let Some(mut task) = loaded_task {
                let now = now_millis();
                let config_dir = Game::native_config_dir().ok();
                let reg_dir = world_dir.as_deref().map(|d| region_dir(d, &target));
                let sources = Sources {
                    data_dir: Some(&data_dir),
                    config_dir: config_dir.as_deref(),
                    region_dir: reg_dir.as_deref(),
                };
                if let Err(e) = task.build_chunks(sources, |_, _| true) {
                    sender.send(game, &format!("Failed to build task: {e:?}"));
                    return;
                }
                task.begin(now);
                let _ = task::save(&task, &tdir);
                with_state(|state| state.tasks.insert(task));
                sender.send(game, &messages::continue_task(&target).line());
            } else {
                sender.send(game, &messages::continue_no_tasks().line());
            }
        }
        Action::Cancel { world } => {
            let default_dim = game.dim.id().to_string();
            let target = world.unwrap_or_else(|| {
                with_state(|state| {
                    if state.active_world.is_empty() {
                        default_dim.clone()
                    } else {
                        state.active_world.clone()
                    }
                })
            });

            let cancelled = with_state(|state| state.tasks.remove(&target));
            if let Some(mut task) = cancelled {
                let now = now_millis();
                task.stop(true);
                let owner = format!("chunky:task/{target}");
                for chunk in task.held() {
                    game.native_force_chunk_in(&owner, &target, [chunk.x, chunk.z], false);
                }
                task.finish(true, now);
                let world_dir = game.native_world_dir();
                let data_dir = game.native_data_dir(chunky_core::ID);
                let tdir = tasks_dir(world_dir.as_deref(), &data_dir);
                let _ = task::save(&task, &tdir);
                sender.send(game, &messages::cancel(&target).line());
            } else {
                sender.send(game, &messages::cancel_no_tasks().line());
            }
        }
        Action::Progress => {
            let running = with_state(|state| state.tasks.running());
            if running.is_empty() {
                sender.send(game, &messages::progress_no_tasks().line());
            } else {
                with_state(|state| {
                    for w in running {
                        if let Some(task) = state.tasks.get(&w) {
                            let prog = &task.progress;
                            sender.send(
                                game,
                                &messages::task_update(
                                    &w,
                                    prog.count,
                                    prog.percent(),
                                    prog.eta_parts(),
                                    prog.rate,
                                    prog.chunk_x,
                                    prog.chunk_z,
                                )
                                .line(),
                            );
                        }
                    }
                });
            }
        }
        Action::Trim(args) => {
            let default_dim = game.dim.id().to_string();
            let world = args.selection.world.clone().unwrap_or_else(|| {
                with_state(|state| {
                    if state.active_world.is_empty() {
                        default_dim.clone()
                    } else {
                        state.active_world.clone()
                    }
                })
            });

            let mut sel = with_state(|state| state.selection_for(&world).clone());
            sel.world = world.clone();
            if let Some(shape_str) = args.selection.shape {
                if let Some(s) = chunky_core::shape::SHAPES
                    .iter()
                    .find(|s| s.name().eq_ignore_ascii_case(&shape_str))
                {
                    sel.shape = *s;
                }
            }
            if let Some((cx, cz)) = args.selection.center {
                sel.center_x = cx;
                sel.center_z = cz;
            }
            if let Some(rx) = args.selection.radius_x {
                let rz = args.selection.radius_z.unwrap_or(rx);
                sel.set_radii(rx, rz);
            }
            let bounds = sel.bounding();

            for filter in &args.filters {
                sender.send(game, &filter.notice().line());
            }
            sender.send(
                game,
                &messages::trim_confirm(
                    &world,
                    sel.shape,
                    sel.center_x,
                    sel.center_z,
                    &sel.radius_text(),
                    args.inside,
                )
                .line(),
            );

            with_state(|state| {
                state.pending = Some((
                    PendingAction::Trim {
                        world,
                        shape: sel.shape,
                        bounds,
                        inside: args.inside,
                        filters: args.filters,
                    },
                    Instant::now(),
                ));
            });
        }
        Action::Confirm => {
            let pending = with_state(|state| state.pending.take());
            if let Some((action, time)) = pending {
                if time.elapsed() <= Duration::from_secs(60) {
                    match action {
                        PendingAction::Trim {
                            world,
                            shape,
                            bounds,
                            inside,
                            filters,
                        } => {
                            let Some(world_dir) = game.native_world_dir() else {
                                sender.send(game, "No world is currently open.");
                                return;
                            };
                            let reg_dir = region_dir(&world_dir, &world);
                            let root = reg_dir.parent().unwrap_or(&world_dir);
                            let start = Instant::now();
                            sender.send(game, "Trimming region files, please wait...");
                            match trim::trim_world(
                                root,
                                shape,
                                &bounds,
                                inside,
                                &filters,
                                |_, _, _| true,
                            ) {
                                Ok((deleted, _regions)) => {
                                    let elapsed = start.elapsed().as_secs_f64();
                                    sender.send(
                                        game,
                                        &messages::task_trim(deleted, &world, elapsed).line(),
                                    );
                                }
                                Err(e) => {
                                    sender.send(game, &format!("Trim failed: {e:?}"));
                                }
                            }
                        }
                    }
                    return;
                }
            }
            sender.send(game, &messages::confirm().line());
        }
        Action::Reload => {
            let running = with_state(|state| !state.tasks.running().is_empty());
            if running {
                sender.send(game, &messages::reload_tasks_running().line());
                return;
            }
            with_state(|state| {
                if let Ok(dir) = Game::native_config_dir() {
                    let path = config::Config::path_in(&dir);
                    state.config = Config::load(&path);
                }
            });
            sender.send(game, &messages::reload().line());
        }
        Action::DebugMap => {
            let default_dim = game.dim.id().to_string();
            let sel = with_state(|state| {
                let world = if state.active_world.is_empty() {
                    default_dim.clone()
                } else {
                    state.active_world.clone()
                };
                state.selection_for(&world).clone()
            });

            sender.send(game, &messages::debug_map_start(&sel.world).line());
            let Some(world_dir) = game.native_world_dir() else {
                sender.send(
                    game,
                    &messages::debug_map_failed(&sel.world, "No world is open").line(),
                );
                return;
            };

            let reg_dir = region_dir(&world_dir, &sel.world);
            let mut full = 0u64;
            let unlit = 0u64;
            let mut incomplete = 0u64;
            let mut invalid = 0u64;
            let mut missing = 0u64;

            let math = sel.math();
            let chunk_min_x = math.center_chunk_x - math.radius_chunks_x;
            let chunk_max_x = math.center_chunk_x + math.radius_chunks_x;
            let chunk_min_z = math.center_chunk_z - math.radius_chunks_z;
            let chunk_max_z = math.center_chunk_z + math.radius_chunks_z;

            for cx in chunk_min_x..=chunk_max_x {
                for cz in chunk_min_z..=chunk_max_z {
                    let pos = ChunkPos::new(cx, cz);
                    if !sel.keeps(pos) {
                        continue;
                    }
                    let rx = cx >> 5;
                    let rz = cz >> 5;
                    let rname = chunky_core::anvil::region_name(rx, rz);
                    let rpath = reg_dir.join(&rname);
                    if !rpath.exists() {
                        missing += 1;
                        continue;
                    }
                    match RegionFile::open(&rpath) {
                        Ok(mut region) => {
                            if let Some(status) = region.chunk_status(pos) {
                                if status.is_full() {
                                    full += 1;
                                } else if status.is_empty() {
                                    missing += 1;
                                } else {
                                    incomplete += 1;
                                }
                            } else {
                                missing += 1;
                            }
                        }
                        Err(_) => {
                            invalid += 1;
                        }
                    }
                }
            }

            let missing_str = chunky_core::geometry::format_number(missing as f64);
            sender.send(
                game,
                &messages::debug_map(
                    &sel.world,
                    &reg_dir.display().to_string(),
                    full,
                    unlit,
                    incomplete,
                    invalid,
                    &missing_str,
                )
                .line(),
            );
        }
    }
}

pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_on(chunky_core::ID) || !game.native_authoritative() {
        return;
    }
    let now = now_millis();
    let mut reports: Vec<String> = Vec::new();
    let mut dones: Vec<String> = Vec::new();
    let mut to_load: Vec<(String, ChunkPos)> = Vec::new();

    with_state(|state| {
        load_config_if_needed(game, state);
        if let Some(max_players) = state.config.player_pause_count {
            let count = game.native_server_players().len() as u32;
            if count >= max_players {
                return;
            }
        }

        let world_dir = game.native_world_dir();
        let data_dir = game.native_data_dir(chunky_core::ID);
        let tdir = tasks_dir(world_dir.as_deref(), &data_dir);
        let update_interval = state.config.update_interval;
        let silent = state.config.silent;

        let running_worlds = state.tasks.running();
        for world in running_worlds {
            let Some(task) = state.tasks.get_mut(&world) else {
                continue;
            };
            let cache = state.caches.entry(world.clone()).or_insert_with(|| {
                let rdir = world_dir.as_deref().map(|d| region_dir(d, &world));
                RegionCache::new(rdir.as_deref().and_then(|r| r.parent()))
            });

            let mut steps = 0;
            while steps < 16 {
                match task.next_to_load(now, cache) {
                    Some(Step::Load(chunk)) => {
                        to_load.push((world.clone(), chunk));
                        steps += 1;
                    }
                    Some(Step::Counted) => {
                        steps += 1;
                    }
                    None => break,
                }
            }

            task.measure(now);
            if !silent && task.report_due(now, update_interval) {
                let prog = &task.progress;
                reports.push(
                    messages::task_update(
                        &world,
                        prog.count,
                        prog.percent(),
                        prog.eta_parts(),
                        prog.rate,
                        prog.chunk_x,
                        prog.chunk_z,
                    )
                    .line(),
                );
                task.reported(now);
            }

            if task.walk_exhausted() && task.working_count() == 0 {
                task.finish(false, now);
                let prog = &task.progress;
                dones.push(
                    messages::task_done(&world, prog.count, prog.percent(), prog.time_parts())
                        .line(),
                );
                let _ = task::save(task, &tdir);
            } else if task.autosave_due(now) {
                task.saved_automatically(now);
                let _ = task::save(task, &tdir);
            }
        }
    });

    for (world, chunk) in to_load {
        let owner = format!("chunky:task/{world}");
        game.native_force_chunk_in(&owner, &world, [chunk.x, chunk.z], true);
    }

    for report in reports {
        game.native_broadcast(Text::literal(&report));
    }
    for done in dones {
        game.native_broadcast(Text::literal(&done));
    }
}

pub(super) fn on_chunk_load(game: &mut Game<'_>, dimension: &str, chunk: [i32; 2], _new: bool) {
    if !game.native_on(chunky_core::ID) || !game.native_authoritative() {
        return;
    }
    let now = now_millis();
    let pos = ChunkPos::new(chunk[0], chunk[1]);
    let mut release = false;

    with_state(|state| {
        if let Some(cache) = state.caches.get_mut(dimension) {
            cache.set_generated(pos);
        }
        if let Some(task) = state.tasks.get_mut(dimension) {
            if task.loaded(pos, now) {
                release = true;
            }
        }
    });

    if release {
        let owner = format!("chunky:task/{dimension}");
        game.native_force_chunk_in(&owner, dimension, chunk, false);
    }
}

pub(super) fn on_world_open(game: &mut Game<'_>) {
    with_state(|state| {
        load_config_if_needed(game, state);
        if !state.config.continue_on_restart {
            return;
        }
        let world_dir = game.native_world_dir();
        let data_dir = game.native_data_dir(chunky_core::ID);
        let tdir = tasks_dir(world_dir.as_deref(), &data_dir);
        let saved_tasks = task::load_all(&tdir);
        let now = now_millis();
        let config_dir = Game::native_config_dir().ok();
        for mut task in saved_tasks {
            if task.state.is_cancelled() {
                continue;
            }
            let world = task.world().to_string();
            let reg_dir = world_dir.as_deref().map(|d| region_dir(d, &world));
            let sources = Sources {
                data_dir: Some(&data_dir),
                config_dir: config_dir.as_deref(),
                region_dir: reg_dir.as_deref(),
            };
            if task.build_chunks(sources, |_, _| true).is_ok() {
                task.begin(now);
                state.tasks.insert(task);
            }
        }
    });
}

pub(super) fn on_world_close(game: &mut Game<'_>) {
    with_state(|state| {
        let world_dir = game.native_world_dir();
        let data_dir = game.native_data_dir(chunky_core::ID);
        let tdir = tasks_dir(world_dir.as_deref(), &data_dir);
        let running_worlds = state.tasks.running();
        for world in running_worlds {
            if let Some(task) = state.tasks.get_mut(&world) {
                task.stop(false);
                let owner = format!("chunky:task/{world}");
                for chunk in task.held() {
                    game.native_force_chunk_in(&owner, &world, [chunk.x, chunk.z], false);
                }
                let _ = task::save(task, &tdir);
            }
        }
        state.tasks.clear();
        state.caches.clear();
        state.pending = None;
    });
}

pub(super) fn settings_buttons(game: &mut Game<'_>, buttons: &mut Vec<String>) {
    with_state(|state| {
        load_config_if_needed(game, state);
        buttons.push(format!(
            "Silent: {}",
            if state.config.silent { "On" } else { "Off" }
        ));
        buttons.push(format!(
            "Continue on restart: {}",
            if state.config.continue_on_restart {
                "On"
            } else {
                "Off"
            }
        ));
        buttons.push(format!(
            "Force load existing: {}",
            if state.config.force_load_existing_chunks {
                "On"
            } else {
                "Off"
            }
        ));
        buttons.push(format!(
            "Update interval: {}s",
            state.config.update_interval
        ));
    });
}

pub(super) fn settings_click(game: &mut Game<'_>, button: usize) {
    with_state(|state| {
        load_config_if_needed(game, state);
        match button {
            0 => state.config.silent = !state.config.silent,
            1 => state.config.continue_on_restart = !state.config.continue_on_restart,
            2 => state.config.force_load_existing_chunks = !state.config.force_load_existing_chunks,
            3 => {
                state.config.update_interval = match state.config.update_interval {
                    1 => 5,
                    5 => 10,
                    10 => 30,
                    30 => 60,
                    _ => 1,
                };
            }
            _ => return,
        }
        save_config(game, &state.config);
    });
}
