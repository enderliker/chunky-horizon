//! The lines Chunky prints, in Chunky's own English.
//!
//! Every string here is the `en.json` entry of that name with its arguments
//! filled in, so a player who knows the plugin reads the same sentences. One
//! thing is dropped: Chunky's `&c`/`&a` legacy colour codes. Horizon's chat and
//! console take plain text, and a stray `&c` in the middle of a sentence reads
//! worse than no colour at all, so the codes are removed and the wording is
//! left alone.

use crate::geometry::{format_number, format_percent, format_radius, format_rate};
use crate::shape::ShapeKind;

/// What every line Chunky sends starts with.
pub const PREFIX: &str = "[Chunky] ";

/// Fills a template's `%s` holes left to right, as Java's `String.format` does,
/// and turns the `%%` of a template into one `%`. A template with more holes
/// than arguments keeps the holes that were not filled.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(at) = rest.find('%') {
        out.push_str(&rest[..at]);
        match rest[at + 1..].chars().next() {
            Some('s') => {
                match args.next() {
                    Some(arg) => out.push_str(arg),
                    None => out.push_str("%s"),
                }
                rest = &rest[at + 2..];
            }
            // `%%` is one per cent, as Java's format spells it.
            Some('%') => {
                out.push('%');
                rest = &rest[at + 2..];
            }
            _ => {
                out.push('%');
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drops legacy formatting codes: `&` or `§` followed by one character.
///
/// `&` is only treated as a code before a character that is one — a hexadecimal
/// digit or one of `klmnor` — so a sentence that says "&" on its own keeps it.
pub fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '&' || c == '§')
            && let Some(&next) = chars.peek()
            && is_code(next)
        {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

fn is_code(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), '0'..='9' | 'a'..='f' | 'k' | 'l' | 'm' | 'n' | 'o' | 'r')
}

/// The body of a message, before the prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message(pub String);

impl Message {
    /// The line as it goes out, prefix and all.
    pub fn line(&self) -> String {
        format!("{PREFIX}{}", self.0)
    }

    /// The body of the message without the prefix.
    pub fn raw(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.line())
    }
}

/// `help_...`: the usage line for a command.
pub fn help(key: &str) -> Message {
    let text = match key {
        "start" => "&2chunky start&r - Start a new chunk generation task",
        "pause" => "&2chunky pause&r - Pause current tasks and save progress",
        "continue" => "&2chunky continue&r - Continue current or saved tasks",
        "cancel" => "&2chunky cancel&r - Stop and delete current or saved tasks",
        "world" => "&2chunky world <world>&r - Set the world target",
        "worldborder" => "&2chunky worldborder&r - Use the world border center & radius",
        "center" => "&2chunky center <x> <z>&r - Set the center block location",
        "spawn" => "&2chunky spawn&r - Set the center to the spawn point",
        "radius" => "&2chunky radius <radius>&r - Set the radius",
        "corners" => "&2chunky corners <x1> <z1> <x2> <z2>&r - Set by corners",
        "shape" => "&2chunky shape <shape>&r - Set the shape to generate",
        "pattern" => "&2chunky pattern <pattern>&r - Set the generation pattern",
        "silent" => "&2chunky silent&r - Toggle displaying update messages",
        "quiet" => "&2chunky quiet <interval>&r - Set the quiet interval",
        "limit" => "&2chunky limit <rate>&r - Set the max chunks per second",
        "trim" => "&2chunky trim&r - Delete chunks outside selection",
        "selection" => "&2chunky selection&r - Display the current selection",
        "progress" => "&2chunky progress&r - Displays current task progress",
        "reload" => "&2chunky reload&r - Reload configuration",
        "border" => {
            "&2chunky border <add|remove|wrap|list|load|bypass>&r - Manage custom world borders"
        }
        "debug" => "&2chunky debug&r - Debug tools",
        other => return Message(format!("chunky {other}")).stripped(),
    };
    Message(text.to_string()).stripped()
}

impl Message {
    /// The same message with its legacy colour codes taken out.
    pub fn stripped(self) -> Self {
        Message(strip_codes(&self.0))
    }
}

/// `command_no_permission`.
pub fn command_no_permission() -> Message {
    Message("You do not have permission for this command.".to_string())
}

/// `format_center`.
pub fn center(x: f64, z: f64) -> Message {
    Message(fill(
        "Center changed to %s, %s.",
        &[&format_number(x), &format_number(z)],
    ))
}

/// `format_radius`.
pub fn radius(radius: f64) -> Message {
    Message(fill("Radius changed to %s.", &[&format_number(radius)]))
}

/// `format_radii`.
pub fn radii(radius_x: f64, radius_z: f64) -> Message {
    Message(fill(
        "Radius changed for x to %s, and for z to %s.",
        &[&format_number(radius_x), &format_number(radius_z)],
    ))
}

/// `format_shape`.
pub fn shape(shape: ShapeKind) -> Message {
    Message(fill("Shape changed to %s.", &[shape.name()]))
}

/// `format_shape_not_found`.
pub fn shape_not_found(name: &str, available: &str) -> Message {
    Message(fill(
        "Shape %s not found. Available shapes: %s",
        &[name, available],
    ))
}

/// `format_pattern`.
pub fn pattern(kind: &str) -> Message {
    Message(fill("Pattern changed to %s.", &[kind]))
}

/// `format_pattern_not_found`.
pub fn pattern_not_found(name: &str, available: &str) -> Message {
    Message(fill(
        "Pattern %s not found. Available patterns: %s",
        &[name, available],
    ))
}

/// `format_world`.
pub fn world(name: &str) -> Message {
    Message(fill("World changed to %s.", &[name]))
}

/// `format_world_not_found`.
pub fn world_not_found(name: &str, available: &str) -> Message {
    Message(fill(
        "World %s not found. Available worlds: %s",
        &[name, available],
    ))
}

/// `format_limit`.
pub fn limit(world: &str, rate: u64) -> Message {
    Message(fill(
        "Rate limit for %s set to %s chunks per second.",
        &[world, &rate.to_string()],
    ))
}

/// `format_limit_disabled`.
pub fn limit_disabled(world: &str) -> Message {
    Message(fill("Rate limit disabled for %s.", &[world]))
}

/// `format_quiet`.
pub fn quiet(seconds: u64) -> Message {
    Message(fill(
        "Quiet interval set to %s seconds.",
        &[&seconds.to_string()],
    ))
}

/// `format_silent`.
pub fn silent(enabled: bool) -> Message {
    Message(fill(
        "Silent mode %s.",
        &[if enabled { "enabled" } else { "disabled" }],
    ))
}

/// `format_reload`.
pub fn reload() -> Message {
    Message("Successfully reloaded configuration.".to_string())
}

/// `format_reload_tasks_running`.
pub fn reload_tasks_running() -> Message {
    Message("The configuration cannot be reloaded while tasks are running.".to_string())
}

/// `format_start`.
pub fn start(world: &str, shape: ShapeKind, center_x: f64, center_z: f64, radius: &str) -> Message {
    Message(fill(
        "Task started in %s for the %s region centered at %s, %s with radius %s.",
        &[
            world,
            shape.name(),
            &format_number(center_x),
            &format_number(center_z),
            radius,
        ],
    ))
}

/// `format_start_confirm`.
pub fn start_confirm() -> Message {
    Message(fill(
        "A task was already started for this world. To continue running it, type '%s'. To start a new task, type '%s'.",
        &["/chunky continue", "/chunky confirm"],
    ))
}

/// `format_started_already`.
pub fn started_already(world: &str) -> Message {
    Message(fill("Task already started for %s!", &[world]))
}

/// `format_start_limit`.
pub fn start_limit(limit: f64) -> Message {
    Message(fill(
        "Your host has limited the maximum pre-generation radius to %s to avoid excessive disk space usage. Reduce the radius or contact your host if you wish to have this limit removed.",
        &[&format_number(limit)],
    ))
}

/// `format_start_disk`.
pub fn start_disk(remaining: &str, required: &str) -> Message {
    Message(fill(
        "The server only has %s disk space remaining, but the estimated disk space required for the task is %s. If you wish to start anyway, type '%s'.",
        &[remaining, required, "/chunky confirm"],
    ))
}

/// `format_worldborder_too_large`.
pub fn worldborder_too_large(maximum: f64) -> Message {
    Message(
        fill(
            "&eNotice: Your world border is currently set larger than the maximum of %s configured in server.properties. This is normally an error and can cause issues.",
            &[&format_number(maximum)],
        ),
    )
    .stripped()
}

/// `format_pause`.
pub fn pause(world: &str) -> Message {
    Message(fill("Task paused for %s.", &[world]))
}

/// `format_pause_no_tasks`.
pub fn pause_no_tasks() -> Message {
    Message("No tasks to pause.".to_string())
}

/// `format_continue`.
pub fn continue_task(world: &str) -> Message {
    Message(fill("Task continuing for %s.", &[world]))
}

/// `format_continue_no_tasks`.
pub fn continue_no_tasks() -> Message {
    Message("No tasks to continue.".to_string())
}

/// `format_cancel_all`.
pub fn cancel_all() -> Message {
    Message("Cancelling all tasks.".to_string())
}

/// `format_cancel`.
pub fn cancel(world: &str) -> Message {
    Message(fill("Task cancelled for %s.", &[world]))
}

/// `format_cancel_confirm`.
pub fn cancel_confirm() -> Message {
    Message(fill(
        "Cancelled tasks cannot be continued. If you are sure you want to cancel, type '%s'.",
        &["/chunky confirm"],
    ))
}

/// `format_cancel_no_tasks`.
pub fn cancel_no_tasks() -> Message {
    Message("No tasks to cancel.".to_string())
}

/// `format_confirm`.
pub fn confirm() -> Message {
    Message("Nothing to confirm!".to_string())
}

/// `format_progress_no_tasks`.
pub fn progress_no_tasks() -> Message {
    Message("No tasks running.".to_string())
}

/// `format_selection`, the header of the selection readout.
pub fn selection() -> Message {
    Message("Current selection".to_string())
}

/// `format_selection_world`.
pub fn selection_world(world: &str) -> Message {
    Message(fill("World: %s", &[world]))
}

/// `format_selection_center`.
pub fn selection_center(x: f64, z: f64) -> Message {
    Message(fill(
        "Center: %s, %s",
        &[&format_number(x), &format_number(z)],
    ))
}

/// `format_selection_shape`.
pub fn selection_shape(shape: ShapeKind) -> Message {
    Message(fill("Shape: %s", &[shape.name()]))
}

/// `format_selection_radius`.
pub fn selection_radius(radius: &str) -> Message {
    Message(fill("Radius: %s", &[radius]))
}

/// `format_selection_radii`.
pub fn selection_radii(radius_x: f64, radius_z: f64) -> Message {
    Message(fill(
        "Radii: %s, %s",
        &[&format_number(radius_x), &format_number(radius_z)],
    ))
}

/// `task_update`: one progress report.
///
/// The fit is Chunky's exactly: the chunk count is the raw number, the
/// percentage takes two decimals, the ETA is three arguments and the rate one
/// decimal.
pub fn task_update(
    world: &str,
    chunks: u64,
    percent: f64,
    eta: (String, String, String),
    rate: f64,
    x: i32,
    z: i32,
) -> Message {
    Message(fill(
        "Task running for %s. Processed: %s chunks (%s%%), ETA: %s:%s:%s, Rate: %s cps, Current: %s, %s",
        &[
            world,
            &chunks.to_string(),
            &format_percent(percent),
            &eta.0,
            &eta.1,
            &eta.2,
            &format_rate(rate),
            &x.to_string(),
            &z.to_string(),
        ],
    ))
}

/// `task_done`.
pub fn task_done(
    world: &str,
    chunks: u64,
    percent: f64,
    time: (String, String, String),
) -> Message {
    Message(fill(
        "Task finished for %s. Processed: %s chunks (%s%%), Total time: %s:%s:%s",
        &[
            world,
            &chunks.to_string(),
            &format_percent(percent),
            &time.0,
            &time.1,
            &time.2,
        ],
    ))
}

/// `task_stopped`.
pub fn task_stopped(world: &str) -> Message {
    Message(fill("Task stopped for %s.", &[world]))
}

/// `task_trim_update`.
pub fn task_trim_update(world: &str, regions: u64, percent: f64) -> Message {
    Message(fill(
        "Task running for %s. Processed: %s regions (%s%%)",
        &[world, &regions.to_string(), &format_percent(percent)],
    ))
}

/// `task_trim`.
pub fn task_trim(chunks: u64, world: &str, seconds: f64) -> Message {
    Message(fill(
        "Deleted %s chunks from %s in %s seconds.",
        &[&chunks.to_string(), world, &format!("{seconds:.3}")],
    ))
}

/// `task_trim_unsupported_compression`.
pub fn task_trim_unsupported_compression(world: &str, region: &str, reason: &str) -> Message {
    Message(
        fill(
            "&cWarning: Task stopped for %s. Region %s could not be read (%s). Chunks in this region and any remaining regions were not trimmed.",
            &[world, region, reason],
        ),
    )
    .stripped()
}

/// `format_trim_confirm`.
pub fn trim_confirm(
    world: &str,
    shape: ShapeKind,
    x: f64,
    z: f64,
    radius: &str,
    inside: bool,
) -> Message {
    let template = if inside {
        "&cWarning: Chunks in %s inside of the %s region centered at %s, %s with radius %s will be permanently deleted. Please make sure you have taken a backup of your server before continuing to avoid potential damage. You will be required to restart your server fully after the task has finished. If you understand and wish to proceed, type '%s'."
    } else {
        "&cWarning: Chunks in %s outside of the %s region centered at %s, %s with radius %s will be permanently deleted. Please make sure you have taken a backup of your server before continuing to avoid potential damage. You will be required to restart your server fully after the task has finished. If you understand and wish to proceed, type '%s'."
    };
    Message(fill(
        template,
        &[
            world,
            shape.name(),
            &format_number(x),
            &format_number(z),
            radius,
            "/chunky confirm",
        ],
    ))
    .stripped()
}

/// `format_trim_confirm_inhabited`.
pub fn trim_confirm_inhabited(max_ticks: i64) -> Message {
    Message(
        fill(
            "&eNotice: Filtering chunks by a maximum inhabited time of %s ticks. Trim may take longer than usual to complete.",
            &[&max_ticks.to_string()],
        ),
    )
    .stripped()
}

/// `format_trim_confirm_inhabited_minimum`.
pub fn trim_confirm_inhabited_minimum(min_ticks: i64) -> Message {
    Message(
        fill(
            "&eNotice: Filtering chunks by a minimum inhabited time of %s ticks. Trim may take longer than usual to complete.",
            &[&min_ticks.to_string()],
        ),
    )
    .stripped()
}

/// `format_trim_confirm_inhabited_range`.
pub fn trim_confirm_inhabited_range(min_ticks: i64, max_ticks: i64) -> Message {
    Message(
        fill(
            "&eNotice: Filtering chunks by an inhabited time between %s and %s ticks. Trim may take longer than usual to complete.",
            &[&min_ticks.to_string(), &max_ticks.to_string()],
        ),
    )
    .stripped()
}

/// `format_trim_confirm_unmodified`.
pub fn trim_confirm_unmodified(amount: f64, unit: &str) -> Message {
    Message(
        fill(
            "&eNotice: Filtering chunks that have not been modified in the last %s. Chunks without a known modification time will not be deleted.",
            &[&unit_label(amount, unit)],
        ),
    )
    .stripped()
}

/// `format_trim_filter_invalid`.
pub fn trim_filter_invalid(token: &str, available: &str) -> Message {
    Message(fill(
        "Trim filter %s is invalid. Available filters: %s",
        &[token, available],
    ))
}

/// `format_border_load`: the selection took the world border's centre and radii.
pub fn border_load(world: &str) -> Message {
    Message(fill("Selection loaded from world border for %s.", &[world]))
}

/// `format_border_no_border`.
pub fn border_no_border(world: &str) -> Message {
    Message(fill("No world border exists for %s.", &[world]))
}

/// `format_debug_map_start`.
pub fn debug_map_start(world: &str) -> Message {
    Message(fill("Creating generation map for %s...", &[world]))
}

/// `format_debug_map`.
pub fn debug_map(
    world: &str,
    path: &str,
    full: u64,
    unlit: u64,
    incomplete: u64,
    invalid: u64,
    missing: &str,
) -> Message {
    Message(fill(
        "Generation map for %s saved to %s. Full: %s, Unlit or unsaved: %s, Incomplete: %s, Invalid: %s, Not generated: %s",
        &[
            world,
            path,
            &format_number(full as f64),
            &format_number(unlit as f64),
            &format_number(incomplete as f64),
            &format_number(invalid as f64),
            missing,
        ],
    ))
}

/// `format_debug_map_failed`.
pub fn debug_map_failed(world: &str, error: &str) -> Message {
    Message(fill(
        "&cUnable to save generation map for %s (%s).",
        &[world, error],
    ))
    .stripped()
}

/// `help_more`.
pub fn help_more(command: &str) -> Message {
    Message(fill("Type &a%s&r to see more.", &[command])).stripped()
}

/// Formats a page of Chunky help commands, matching official Chunky's `HelpCommand`.
pub fn help_page(page: usize, is_player: bool) -> Vec<Message> {
    let mut out = Vec::new();
    out.push(Message("&aChunky Commands&r".to_string()).stripped());
    let total = crate::command::HELP_COMMANDS.len();
    let per_page = 8;
    let page_count = total.div_ceil(per_page);
    let page_index = if page == 0 {
        0
    } else {
        (page - 1) % page_count
    };
    let (first, last) = if is_player {
        (
            page_index * per_page,
            (page_index * per_page + per_page).min(total),
        )
    } else {
        (0, total)
    };
    for &cmd in &crate::command::HELP_COMMANDS[first..last] {
        out.push(help(cmd));
    }
    if is_player && page_index + 1 < page_count {
        out.push(help_more(&format!("/chunky help {}", page_index + 2)));
    }
    out
}

/// The `time_...` entries: "3 minutes", "1 minute".
pub fn unit_label(amount: f64, unit: &str) -> String {
    let number = format_number(amount);
    let singular = amount == 1.0;
    let unit = match unit {
        "minute" | "minutes" => {
            if singular {
                "minute"
            } else {
                "minutes"
            }
        }
        "hour" | "hours" => {
            if singular {
                "hour"
            } else {
                "hours"
            }
        }
        "day" | "days" => {
            if singular {
                "day"
            } else {
                "days"
            }
        }
        _ => {
            if singular {
                "second"
            } else {
                "seconds"
            }
        }
    };
    format!("{number} {unit}")
}

/// A selection's radii as the messages print them.
pub fn radii_text(radius_x: f64, radius_z: f64) -> String {
    format_radius(radius_x, radius_z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_fills_its_holes_in_order() {
        assert_eq!(fill("%s and %s", &["a", "b"]), "a and b");
        assert_eq!(fill("no holes", &[]), "no holes");
        assert_eq!(fill("%s %s %s", &["a"]), "a %s %s");
        assert_eq!(fill("100%% of %s", &["it"]), "100% of it");
        assert_eq!(fill("%50", &[]), "%50");
    }

    #[test]
    fn legacy_codes_come_out_and_plain_ampersands_stay() {
        assert_eq!(strip_codes("&cCareful!"), "Careful!");
        assert_eq!(strip_codes("&aChunky Commands&r"), "Chunky Commands");
        assert_eq!(strip_codes("rock & roll"), "rock & roll");
        assert_eq!(
            strip_codes("&2chunky start&r - Start"),
            "chunky start - Start"
        );
        // `&m` is strikethrough, so this one really is a code...
        assert_eq!(strip_codes("M&Ms"), "Ms");
        // ...but a `&` before a letter that is not a code is left alone.
        assert_eq!(strip_codes("AT&T"), "AT&T");
    }

    #[test]
    fn the_lines_read_as_chunkys_do() {
        assert_eq!(center(0.0, 0.0).line(), "[Chunky] Center changed to 0, 0.");
        assert_eq!(
            shape(ShapeKind::Square).line(),
            "[Chunky] Shape changed to square."
        );
        assert_eq!(
            start("minecraft:overworld", ShapeKind::Square, 0.0, 0.0, "500").line(),
            "[Chunky] Task started in minecraft:overworld for the square region centered at 0, 0 with radius 500."
        );
        assert_eq!(
            help("start").line(),
            "[Chunky] chunky start - Start a new chunk generation task"
        );
    }

    #[test]
    fn a_progress_report_looks_like_chunkys() {
        let line = task_update(
            "minecraft:overworld",
            1500,
            25.5,
            ("0".into(), "01".into(), "30".into()),
            64.0,
            4,
            8,
        )
        .line();
        assert_eq!(
            line,
            "[Chunky] Task running for minecraft:overworld. Processed: 1500 chunks (25.50%), ETA: 0:01:30, Rate: 64.0 cps, Current: 4, 8"
        );
    }

    #[test]
    fn a_finished_task_names_its_total_time() {
        let line = task_done(
            "minecraft:overworld",
            1024,
            100.0,
            ("0".into(), "00".into(), "12".into()),
        )
        .line();
        assert_eq!(
            line,
            "[Chunky] Task finished for minecraft:overworld. Processed: 1024 chunks (100.00%), Total time: 0:00:12"
        );
    }

    #[test]
    fn times_are_singular_only_at_one() {
        assert_eq!(unit_label(1.0, "minutes"), "1 minute");
        assert_eq!(unit_label(2.0, "minutes"), "2 minutes");
        assert_eq!(unit_label(1.0, "hours"), "1 hour");
        assert_eq!(unit_label(30.0, "seconds"), "30 seconds");
    }

    #[test]
    fn a_trim_warning_is_plain_text() {
        let line = trim_confirm(
            "minecraft:overworld",
            ShapeKind::Square,
            0.0,
            0.0,
            "500",
            false,
        )
        .line();
        assert!(
            line.starts_with("[Chunky] Warning: Chunks in minecraft:overworld outside"),
            "{line}"
        );
        assert!(!line.contains('&'), "{line}");
    }

    #[test]
    fn a_help_page_matches_official_chunkys_pages() {
        let p1 = help_page(1, true);
        assert_eq!(p1.len(), 10);
        assert_eq!(p1[0].raw(), "Chunky Commands");
        assert_eq!(
            p1[1].raw(),
            "chunky start - Start a new chunk generation task"
        );
        assert_eq!(
            p1[8].raw(),
            "chunky spawn - Set the center to the spawn point"
        );
        assert_eq!(p1[9].raw(), "Type /chunky help 2 to see more.");

        let p2 = help_page(2, true);
        assert_eq!(p2.len(), 10);
        assert_eq!(p2[0].raw(), "Chunky Commands");
        assert_eq!(p2[1].raw(), "chunky radius <radius> - Set the radius");
        assert_eq!(p2[9].raw(), "Type /chunky help 3 to see more.");

        let p3 = help_page(3, true);
        assert_eq!(p3.len(), 5);
        assert_eq!(p3[0].raw(), "Chunky Commands");
        assert_eq!(
            p3[1].raw(),
            "chunky selection - Display the current selection"
        );
        assert_eq!(p3[4].raw(), "chunky reload - Reload configuration");

        let console = help_page(1, false);
        assert_eq!(console.len(), 21);
    }
}
