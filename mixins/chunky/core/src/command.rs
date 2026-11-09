//! `/chunky ...`: reading the line, and nothing else.
//!
//! Horizon hands a mod the command the player typed as one string, so this
//! module is the port of Chunky's brigadier tree as a plain parser: it turns a
//! line into an [`Action`], or into the usage line to answer with. What an
//! action then does to the task list is `task.rs`'s business, and the strings it
//! prints are `messages.rs`'s.
//!
//! The positional arguments of `start` and `trim` are read the way Chunky reads
//! them — world, then shape, then a centre, then two radii, then `trim`'s side
//! and filters — which means a shape cannot be given without a world before it.
//! That is Chunky's own order and the port keeps it, so the same line means the
//! same thing in both.

use crate::geometry::{is_past_world_limit, parse_sign, parse_suffixed};
use crate::pattern::{PATTERNS, PatternKind};
use crate::shape::{SHAPES, ShapeKind};
use crate::trim::{self, TrimFilter};

/// The command's label, without its slash.
pub const LABEL: &str = "chunky";

/// Determines whether a client-side command hook should claim the command or
/// forward it to the authoritative server.
///
/// In integrated single-player and remote/LAN multiplayer, commands must NOT be claimed
/// on the client, so that Horizon forwards them across the network / thread boundary to
/// the authoritative server thread where chunk generation actually ticks.
pub fn should_claim_on_client(is_remote: bool, command: &str) -> bool {
    if is_remote {
        return false;
    }
    let line = command.strip_prefix('/').unwrap_or(command);
    if let Some(args) = line.strip_prefix(LABEL) {
        args.is_empty() || args.starts_with(' ')
    } else {
        false
    }
}

/// What a line asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Show the help, optionally a page of it.
    Help {
        page: Option<i64>,
    },
    /// `confirm`: run whatever was left waiting.
    Confirm,
    Center {
        /// Both are given or neither is; neither means the sender's position.
        x: Option<f64>,
        z: Option<f64>,
    },
    Corners {
        x1: f64,
        z1: f64,
        x2: f64,
        z2: f64,
    },
    Radius {
        /// A value on its own sets the radius; one with a sign is added to it.
        radius_x: Option<(f64, Option<i32>)>,
        radius_z: Option<(f64, Option<i32>)>,
    },
    Limit {
        rate: i64,
    },
    Quiet {
        seconds: i64,
    },
    Pattern {
        name: String,
        value: Option<String>,
    },
    Shape {
        name: String,
    },
    World {
        name: Option<String>,
    },
    Spawn,
    WorldBorder,
    Selection,
    Progress,
    Silent,
    Reload,
    Start(SelectionArgs),
    Trim(TrimArgs),
    Pause {
        world: Option<String>,
    },
    Continue {
        world: Option<String>,
    },
    Cancel {
        world: Option<String>,
    },
    /// Print the generation map of the selection.
    DebugMap,
}

/// The world, shape and geometry `start` and `trim` take with them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionArgs {
    pub world: Option<String>,
    pub shape: Option<String>,
    pub center: Option<(f64, f64)>,
    pub radius_x: Option<f64>,
    pub radius_z: Option<f64>,
}

/// What `trim` adds to that.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimArgs {
    pub selection: SelectionArgs,
    /// Whether the chunks *inside* the shape are the ones to delete.
    pub inside: bool,
    pub filters: Vec<TrimFilter>,
}

/// Why a line could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// `/chunky` on its own, or a sub-command that wants arguments and got
    /// none: answer with this usage key.
    Usage(&'static str),
    /// A label that is not one of Chunky's.
    Unknown,
    /// A shape or pattern name that is not one of Chunky's.
    NotFound(NotFound),
    /// The filter tokens at the end of a `trim`.
    TrimFilter(String),
    /// A `trim` whose filters were followed by no usable side or geometry.
    TrimUsage,
}

/// A name that was not recognised, with what could have been meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotFound {
    Shape(String),
    Pattern(String),
}

/// The help pages, in Chunky's `HelpCommand` order.
pub const HELP_ORDER: [&str; 20] = [
    "start",
    "pause",
    "continue",
    "cancel",
    "world",
    "worldborder",
    "center",
    "spawn",
    "radius",
    "corners",
    "shape",
    "pattern",
    "silent",
    "quiet",
    "limit",
    "trim",
    "selection",
    "progress",
    "border",
    "reload",
];
pub const HELP_COMMANDS: [&str; 20] = HELP_ORDER;

/// The shape names in the order the error message lists them.
pub fn shape_names() -> String {
    sorted_shape_names().join(", ")
}

/// All shape names sorted alphabetically.
pub fn sorted_shape_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = SHAPES.iter().map(|shape| shape.name()).collect();
    names.sort_unstable();
    names
}

/// The pattern names in the order the error message lists them.
pub fn pattern_names() -> String {
    sorted_pattern_names().join(", ")
}

/// All pattern names sorted alphabetically.
pub fn sorted_pattern_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = PATTERNS.iter().map(|pattern| pattern.name()).collect();
    names.sort_unstable();
    names
}

/// Reads a whole line, with or without the leading slash and label.
pub fn parse(line: &str) -> Result<Action, ParseError> {
    let line = line.trim();
    let line = line.strip_prefix('/').unwrap_or(line);
    let mut tokens: Vec<&str> = line.split_whitespace().collect();
    // A bare `chunky` is the command; anything else is the sub-command alone.
    if tokens
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case(LABEL))
    {
        tokens.remove(0);
    }
    parse_tokens(&tokens)
}

/// Reads the tokens after the label.
pub fn parse_tokens(tokens: &[&str]) -> Result<Action, ParseError> {
    let Some((label, rest)) = tokens.split_first() else {
        return Ok(Action::Help { page: None });
    };
    let label = label.to_ascii_lowercase();
    match label.as_str() {
        "help" => Ok(Action::Help {
            page: rest.first().and_then(|page| page.parse::<i64>().ok()),
        }),
        "confirm" => Ok(Action::Confirm),
        "center" => {
            let x = rest.first().and_then(|value| parse_suffixed(value));
            let z = rest.get(1).and_then(|value| parse_suffixed(value));
            match (x, z) {
                (Some(x), Some(z)) => {
                    if is_past_world_limit(x) || is_past_world_limit(z) {
                        return Err(ParseError::Usage("center"));
                    }
                    Ok(Action::Center {
                        x: Some(x),
                        z: Some(z),
                    })
                }
                // No arguments at all: the sender's own position.
                (None, None) if rest.is_empty() => Ok(Action::Center { x: None, z: None }),
                _ => Err(ParseError::Usage("center")),
            }
        }
        "corners" => {
            let values: Option<Vec<f64>> = rest
                .iter()
                .take(4)
                .map(|value| parse_suffixed(value))
                .collect();
            let Some(values) = values.filter(|values| values.len() == 4) else {
                return Err(ParseError::Usage("corners"));
            };
            if values.iter().any(|value| is_past_world_limit(*value)) {
                return Err(ParseError::Usage("corners"));
            }
            Ok(Action::Corners {
                x1: values[0],
                z1: values[1],
                x2: values[2],
                z2: values[3],
            })
        }
        "radius" => {
            let radius_x = parse_radius(rest.first().copied())?;
            let Some(radius_x) = radius_x else {
                return Err(ParseError::Usage("radius"));
            };
            let radius_z = match rest.get(1) {
                Some(value) => Some(parse_radius(Some(value))?.ok_or(ParseError::Usage("radius"))?),
                None => None,
            };
            Ok(Action::Radius {
                radius_x: Some(radius_x),
                radius_z,
            })
        }
        "limit" => {
            let Some(rate) = rest.first().and_then(|value| value.parse::<i64>().ok()) else {
                return Err(ParseError::Usage("limit"));
            };
            Ok(Action::Limit { rate: rate.max(0) })
        }
        "quiet" => {
            let Some(seconds) = rest.first().and_then(|value| value.parse::<i64>().ok()) else {
                return Err(ParseError::Usage("quiet"));
            };
            Ok(Action::Quiet {
                seconds: seconds.max(0),
            })
        }
        "pattern" => {
            let Some(name) = rest.first() else {
                return Err(ParseError::Usage("pattern"));
            };
            if PatternKind::parse(name).is_none() {
                return Err(ParseError::NotFound(NotFound::Pattern((*name).to_string())));
            }
            let value = rest.get(1).map(|value| (*value).to_string());
            // `csv` reads a file, so it cannot be given without one.
            if name.eq_ignore_ascii_case("csv") && value.is_none() {
                return Err(ParseError::Usage("pattern"));
            }
            Ok(Action::Pattern {
                name: (*name).to_string(),
                value,
            })
        }
        "shape" => {
            let Some(name) = rest.first() else {
                return Err(ParseError::Usage("shape"));
            };
            if ShapeKind::parse(name).is_none() {
                return Err(ParseError::NotFound(NotFound::Shape((*name).to_string())));
            }
            Ok(Action::Shape {
                name: (*name).to_string(),
            })
        }
        "world" => Ok(Action::World {
            name: rest.first().map(|name| (*name).to_string()),
        }),
        "spawn" => Ok(Action::Spawn),
        "worldborder" => Ok(Action::WorldBorder),
        "selection" => Ok(Action::Selection),
        "progress" => Ok(Action::Progress),
        "silent" => Ok(Action::Silent),
        "reload" => Ok(Action::Reload),
        "start" => Ok(Action::Start(parse_selection_args(rest, "start")?)),
        "trim" => parse_trim(rest),
        "pause" => Ok(Action::Pause {
            world: rest.first().map(|name| (*name).to_string()),
        }),
        "continue" => Ok(Action::Continue {
            world: rest.first().map(|name| (*name).to_string()),
        }),
        "cancel" => Ok(Action::Cancel {
            world: rest.first().map(|name| (*name).to_string()),
        }),
        "debug" => match rest.first().map(|value| value.to_ascii_lowercase()) {
            Some(option) if option == "map" => Ok(Action::DebugMap),
            _ => Err(ParseError::Usage("debug")),
        },
        _ => Err(ParseError::Unknown),
    }
}

/// A radius argument: `500`, `+100`, `-100`, each optionally suffixed.
fn parse_radius(value: Option<&str>) -> Result<Option<(f64, Option<i32>)>, ParseError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let sign = parse_sign(value);
    let digits = if sign.is_some() { &value[1..] } else { value };
    let Some(radius) = parse_suffixed(digits) else {
        return Ok(None);
    };
    if radius < 0.0 || is_past_world_limit(radius) {
        return Ok(None);
    }
    Ok(Some((radius, sign)))
}

/// The positional arguments of `start`: world, shape, centre, radii.
fn parse_selection_args(tokens: &[&str], usage: &'static str) -> Result<SelectionArgs, ParseError> {
    let mut args = SelectionArgs::default();
    let mut rest = tokens.iter().copied();

    args.world = rest.next().map(str::to_string);
    let Some(shape) = rest.next() else {
        return Ok(args);
    };
    if ShapeKind::parse(shape).is_none() {
        return Err(ParseError::NotFound(NotFound::Shape(shape.to_string())));
    }
    args.shape = Some(shape.to_string());

    let Some(center_x) = rest.next() else {
        return Ok(args);
    };
    let center_x = parse_suffixed(center_x).filter(|value| !is_past_world_limit(*value));
    let center_z = rest
        .next()
        .and_then(parse_suffixed)
        .filter(|value| !is_past_world_limit(*value));
    let (Some(center_x), Some(center_z)) = (center_x, center_z) else {
        return Err(ParseError::Usage(usage));
    };
    args.center = Some((center_x, center_z));

    let Some(radius_x) = rest.next() else {
        return Ok(args);
    };
    let Some(radius_x) = parse_radius(Some(radius_x))?.map(|(radius, _)| radius) else {
        return Err(ParseError::Usage(usage));
    };
    args.radius_x = Some(radius_x);

    if let Some(radius_z) = rest.next() {
        let Some(radius_z) = parse_radius(Some(radius_z))?.map(|(radius, _)| radius) else {
            return Err(ParseError::Usage(usage));
        };
        args.radius_z = Some(radius_z);
    }
    Ok(args)
}

/// `trim`'s positional arguments: the selection's, then a side, then filters.
fn parse_trim(tokens: &[&str]) -> Result<Action, ParseError> {
    let selection = parse_selection_args(tokens, "trim")?;
    // `parse_selection_args` stops at the first token it did not take, so the
    // side and the filters are what is left. Six positionals at most.
    let consumed = 1
        + usize::from(selection.shape.is_some())
        + if selection.shape.is_some() { 2 } else { 0 }
        + usize::from(selection.radius_x.is_some())
        + usize::from(selection.radius_z.is_some());
    let rest = &tokens[consumed.min(tokens.len())..];

    let (inside, filter_tokens) = match rest.first() {
        None => (false, &rest[0..0]),
        Some(side) => match side.to_ascii_lowercase().as_str() {
            "inside" => (true, &rest[1..]),
            "outside" => (false, &rest[1..]),
            _ => return Err(ParseError::TrimUsage),
        },
    };
    let filters = trim::parse_filters(
        &filter_tokens
            .iter()
            .map(|token| (*token).to_string())
            .collect::<Vec<_>>(),
        now_seconds(),
    )
    .map_err(ParseError::TrimFilter)?;
    Ok(Action::Trim(TrimArgs {
        selection,
        inside,
        filters,
    }))
}

/// The current time in Unix seconds, for the `unmodified` filter's cutoff.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trim::Unit;

    #[test]
    fn a_bare_label_asks_for_the_help() {
        assert_eq!(parse("chunky"), Ok(Action::Help { page: None }));
        assert_eq!(parse("/chunky"), Ok(Action::Help { page: None }));
        assert_eq!(parse("chunky help 3"), Ok(Action::Help { page: Some(3) }));
        assert_eq!(parse("chunky help"), Ok(Action::Help { page: None }));
    }

    #[test]
    fn the_shape_and_pattern_names_are_checked() {
        assert_eq!(
            parse("chunky shape circle"),
            Ok(Action::Shape {
                name: "circle".to_string()
            })
        );
        assert_eq!(
            parse("chunky shape oval"),
            Err(ParseError::NotFound(NotFound::Shape("oval".to_string())))
        );
        assert_eq!(
            parse("chunky pattern csv"),
            Err(ParseError::Usage("pattern")),
            "csv needs a file"
        );
        assert_eq!(
            parse("chunky pattern csv chunks.csv"),
            Ok(Action::Pattern {
                name: "csv".to_string(),
                value: Some("chunks.csv".to_string())
            })
        );
        assert_eq!(
            parse("chunky pattern nonsense"),
            Err(ParseError::NotFound(NotFound::Pattern(
                "nonsense".to_string()
            )))
        );
    }

    #[test]
    fn a_centre_is_two_numbers_or_none_at_all() {
        assert_eq!(
            parse("chunky center 100 -200"),
            Ok(Action::Center {
                x: Some(100.0),
                z: Some(-200.0)
            })
        );
        assert_eq!(
            parse("chunky center"),
            Ok(Action::Center { x: None, z: None })
        );
        assert_eq!(parse("chunky center 100"), Err(ParseError::Usage("center")));
        assert_eq!(
            parse("chunky center 1e9 0"),
            Err(ParseError::Usage("center")),
            "past the world limit"
        );
    }

    #[test]
    fn a_radius_takes_a_sign() {
        assert_eq!(
            parse("chunky radius 500"),
            Ok(Action::Radius {
                radius_x: Some((500.0, None)),
                radius_z: None
            })
        );
        assert_eq!(
            parse("chunky radius +100"),
            Ok(Action::Radius {
                radius_x: Some((100.0, Some(1))),
                radius_z: None
            })
        );
        assert_eq!(
            parse("chunky radius -100 -50"),
            Ok(Action::Radius {
                radius_x: Some((100.0, Some(-1))),
                radius_z: Some((50.0, Some(-1)))
            })
        );
        assert_eq!(parse("chunky radius"), Err(ParseError::Usage("radius")));
        assert_eq!(
            parse("chunky radius nope"),
            Err(ParseError::Usage("radius"))
        );
    }

    #[test]
    fn corners_need_all_four() {
        assert_eq!(
            parse("chunky corners -100 0 100 50"),
            Ok(Action::Corners {
                x1: -100.0,
                z1: 0.0,
                x2: 100.0,
                z2: 50.0
            })
        );
        assert_eq!(
            parse("chunky corners 1 2 3"),
            Err(ParseError::Usage("corners"))
        );
    }

    #[test]
    fn start_reads_its_positionals_in_chunkys_order() {
        assert_eq!(
            parse("chunky start"),
            Ok(Action::Start(SelectionArgs::default()))
        );
        assert_eq!(
            parse("chunky start minecraft:overworld"),
            Ok(Action::Start(SelectionArgs {
                world: Some("minecraft:overworld".to_string()),
                ..Default::default()
            }))
        );
        assert_eq!(
            parse("chunky start minecraft:overworld square 0 0 100 200"),
            Ok(Action::Start(SelectionArgs {
                world: Some("minecraft:overworld".to_string()),
                shape: Some("square".to_string()),
                center: Some((0.0, 0.0)),
                radius_x: Some(100.0),
                radius_z: Some(200.0),
            }))
        );
        // A centre needs both of its numbers.
        assert_eq!(
            parse("chunky start minecraft:overworld square 0"),
            Err(ParseError::Usage("start"))
        );
        // A radius that is not a number is a usage error too.
        assert_eq!(
            parse("chunky start minecraft:overworld square 0 0 nope"),
            Err(ParseError::Usage("start"))
        );
    }

    #[test]
    fn the_world_slot_takes_anything_and_comes_first() {
        // The world slot is read before the shape, so `square` here is a world
        // name and `circle` the shape. Chunky's order, which the port keeps.
        assert_eq!(
            parse("chunky start square circle"),
            Ok(Action::Start(SelectionArgs {
                world: Some("square".to_string()),
                shape: Some("circle".to_string()),
                ..Default::default()
            }))
        );
        // With one token there is no shape to check, so any name is a world.
        assert_eq!(
            parse("chunky start square"),
            Ok(Action::Start(SelectionArgs {
                world: Some("square".to_string()),
                ..Default::default()
            }))
        );
    }

    #[test]
    fn trim_reads_a_side_and_then_filters() {
        let Action::Trim(args) = parse("chunky trim").unwrap() else {
            panic!("not a trim");
        };
        assert!(!args.inside);
        assert!(args.filters.is_empty());

        let Action::Trim(args) =
            parse("chunky trim minecraft:overworld square 0 0 100 100 inside").unwrap()
        else {
            panic!("not a trim");
        };
        assert!(args.inside);
        assert_eq!(args.selection.radius_x, Some(100.0));

        let Action::Trim(args) =
            parse("chunky trim minecraft:overworld square 0 0 100 100 outside inhabited=600 unmodified=7d")
                .unwrap()
        else {
            panic!("not a trim");
        };
        assert!(!args.inside);
        assert_eq!(args.filters.len(), 2);
        assert_eq!(
            args.filters[0],
            TrimFilter::Inhabited {
                min_ticks: 0,
                max_ticks: 600
            }
        );
        assert!(matches!(
            args.filters[1],
            TrimFilter::Unmodified {
                amount: 7,
                unit: Unit::Days,
                ..
            }
        ));

        // A bare number right after `trim` is the world, not a filter.
        assert!(matches!(parse("chunky trim 500"), Ok(Action::Trim(_))));
        assert!(matches!(
            parse("chunky trim minecraft:overworld square 0 0 100 100 sideways"),
            Err(ParseError::TrimUsage)
        ));
        assert!(matches!(
            parse("chunky trim minecraft:overworld square 0 0 100 100 outside nonsense=1"),
            Err(ParseError::TrimFilter(_))
        ));
    }

    #[test]
    fn unknown_labels_and_options_are_refused() {
        assert_eq!(parse("chunky nope"), Err(ParseError::Unknown));
        assert_eq!(parse("chunky debug"), Err(ParseError::Usage("debug")));
        assert_eq!(parse("chunky debug nope"), Err(ParseError::Usage("debug")));
        assert_eq!(parse("chunky debug map"), Ok(Action::DebugMap));
    }

    #[test]
    fn the_simple_commands_come_through() {
        assert_eq!(parse("chunky silent"), Ok(Action::Silent));
        assert_eq!(parse("chunky reload"), Ok(Action::Reload));
        assert_eq!(parse("chunky selection"), Ok(Action::Selection));
        assert_eq!(parse("chunky progress"), Ok(Action::Progress));
        assert_eq!(parse("chunky spawn"), Ok(Action::Spawn));
        assert_eq!(parse("chunky worldborder"), Ok(Action::WorldBorder));
        assert_eq!(parse("chunky confirm"), Ok(Action::Confirm));
        assert_eq!(parse("chunky quiet 30"), Ok(Action::Quiet { seconds: 30 }));
        assert_eq!(parse("chunky limit 64"), Ok(Action::Limit { rate: 64 }));
        assert_eq!(parse("chunky limit -5"), Ok(Action::Limit { rate: 0 }));
        assert_eq!(parse("chunky world"), Ok(Action::World { name: None }));
        assert_eq!(
            parse("chunky pause minecraft:the_nether"),
            Ok(Action::Pause {
                world: Some("minecraft:the_nether".to_string())
            })
        );
    }

    #[test]
    fn the_label_is_optional_and_case_does_not_matter() {
        assert_eq!(parse("CHUNKY silent"), Ok(Action::Silent));
        assert_eq!(parse("silent"), Ok(Action::Silent));
        assert_eq!(
            parse("/chunky SHAPE Square"),
            Ok(Action::Shape {
                name: "Square".to_string()
            })
        );
    }

    #[test]
    fn the_names_in_the_error_messages_are_sorted() {
        assert_eq!(
            shape_names(),
            "circle, diamond, ellipse, hexagon, pentagon, rectangle, square, star, triangle"
        );
        assert_eq!(
            pattern_names(),
            "concentric, csv, loop, region, ring, spiral, strip, world"
        );
    }

    #[test]
    fn client_command_claiming_skips_when_remote_or_integrated() {
        assert!(!should_claim_on_client(true, "chunky start"));
        assert!(!should_claim_on_client(true, "/chunky start"));
        assert!(!should_claim_on_client(true, "/chunky radius 500"));

        assert!(should_claim_on_client(false, "chunky start"));
        assert!(should_claim_on_client(false, "/chunky start"));

        assert!(!should_claim_on_client(false, "help"));
        assert!(!should_claim_on_client(false, "/gamemode creative"));
        assert!(!should_claim_on_client(false, "chunkynot"));
    }
}
