//! The shapes a selection can take, and the test that says whether a chunk is
//! inside one.
//!
//! The port of Chunky's `shape` package. The test is always given a chunk's
//! centre in block coordinates, and the shape is *chunk aligned*: its centre
//! and its half-width are whole chunks, so a shape covers chunks as a whole
//! and never cuts one in half (`AbstractShape` with `chunkAligned = true`,
//! which is what both the generation task and the trim command build).
//!
//! Two of Chunky's quirks carry over on purpose, because they decide which
//! chunks are generated: a chunk-aligned circle, square, diamond, triangle,
//! hexagon, pentagon and star read only the X radius, so a selection whose
//! radii differ still uses the X one for them; and the polygon test is a
//! *strict* inequality, so a chunk exactly on an edge is outside.

use crate::geometry::SelectionMath;
use std::f64::consts::PI;

/// A shape by name. `Oval` is Chunky's alias of `Ellipse` and is not offered
/// as its own choice (`ShapeType.all()` leaves it out).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Circle,
    Diamond,
    Ellipse,
    Hexagon,
    Pentagon,
    Rectangle,
    Square,
    Star,
    Triangle,
}

/// The shapes a player can choose, in the order the messages list them.
pub const SHAPES: [ShapeKind; 9] = [
    ShapeKind::Circle,
    ShapeKind::Diamond,
    ShapeKind::Ellipse,
    ShapeKind::Hexagon,
    ShapeKind::Pentagon,
    ShapeKind::Rectangle,
    ShapeKind::Square,
    ShapeKind::Star,
    ShapeKind::Triangle,
];

impl ShapeKind {
    /// The name the commands take, and the key the messages translate.
    pub fn name(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Diamond => "diamond",
            Self::Ellipse => "ellipse",
            Self::Hexagon => "hexagon",
            Self::Pentagon => "pentagon",
            Self::Rectangle => "rectangle",
            Self::Square => "square",
            Self::Star => "star",
            Self::Triangle => "triangle",
        }
    }

    /// A shape by name, lowercased, an exact match only (`Input.tryShape`,
    /// which tests against `ShapeType.all()`). `oval` is deliberately not one
    /// of them: Chunky only reads it out of a saved task file, never from a
    /// command.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "circle" => Some(Self::Circle),
            "diamond" => Some(Self::Diamond),
            "ellipse" => Some(Self::Ellipse),
            "hexagon" => Some(Self::Hexagon),
            "pentagon" => Some(Self::Pentagon),
            "rectangle" => Some(Self::Rectangle),
            "square" => Some(Self::Square),
            "star" => Some(Self::Star),
            "triangle" => Some(Self::Triangle),
            _ => None,
        }
    }

    /// A shape as a saved task file names it: `oval` is Chunky's older name
    /// for an ellipse, and `ShapeFactory` still reads it that way.
    pub fn parse_saved(text: &str) -> Option<Self> {
        if text.trim().eq_ignore_ascii_case("oval") {
            return Some(Self::Ellipse);
        }
        Self::parse(text)
    }

    /// The shape the radius command switches to when the radii change: a
    /// square becomes a rectangle when the two radii differ, and a circle an
    /// ellipse; every other shape is left alone (`RadiusCommand`).
    pub fn with_radii(self, radius_x: f64, radius_z: f64) -> Self {
        let equal = radius_x == radius_z;
        match self {
            Self::Square | Self::Rectangle => {
                if equal {
                    Self::Square
                } else {
                    Self::Rectangle
                }
            }
            Self::Circle | Self::Ellipse => {
                if equal {
                    Self::Circle
                } else {
                    Self::Ellipse
                }
            }
            other => other,
        }
    }
}

/// A shape placed on a selection and snapped to whole chunks: its centre in
/// block coordinates and its half-width on each axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounding {
    pub center_x: f64,
    pub center_z: f64,
    pub radius_x: f64,
    pub radius_z: f64,
}

impl Bounding {
    /// The shape as the task and the trim command build it: the centre on a
    /// chunk's centre and the radius a whole number of chunks, so both come
    /// from the chunk counts and never from the raw block radius.
    pub fn chunk_aligned(math: &SelectionMath) -> Self {
        let radius_x = (math.diameter_chunks_x as f64) * 8.0;
        let radius_z = (math.diameter_chunks_z as f64) * 8.0;
        Self {
            center_x: (math.center_chunk_x as f64) * 16.0 + 8.0,
            center_z: (math.center_chunk_z as f64) * 16.0 + 8.0,
            radius_x,
            radius_z,
        }
    }

    /// The same shape on the raw block coordinates, which the debug map and
    /// the trims of an already-built selection do not use but a preview does.
    pub fn block_aligned(center_x: f64, center_z: f64, radius_x: f64, radius_z: f64) -> Self {
        Self {
            center_x,
            center_z,
            radius_x,
            radius_z,
        }
    }
}

/// Whether a point is inside the shape, as `Shape.isBounding`.
pub fn is_bounding(kind: ShapeKind, bounding: &Bounding, x: f64, z: f64) -> bool {
    let (c_x, c_z) = (bounding.center_x, bounding.center_z);
    let (r_x, r_z) = (bounding.radius_x, bounding.radius_z);
    match kind {
        ShapeKind::Circle => ((c_x - x).powi(2) + (c_z - z).powi(2)).sqrt() <= r_x,
        ShapeKind::Square => (x - c_x).abs() <= r_x && (z - c_z).abs() <= r_x,
        ShapeKind::Rectangle => (x - c_x).abs() <= r_x && (z - c_z).abs() <= r_z,
        ShapeKind::Ellipse => {
            if r_x == 0.0 || r_z == 0.0 {
                false
            } else {
                (x - c_x).powi(2) / r_x.powi(2) + (z - c_z).powi(2) / r_z.powi(2) <= 1.0
            }
        }
        ShapeKind::Diamond => {
            let points = [
                (c_x, c_z + r_x),
                (c_x - r_x, c_z),
                (c_x, c_z - r_x),
                (c_x + r_x, c_z),
            ];
            inside_polygon(&points, x, z)
        }
        ShapeKind::Triangle => {
            let points = [
                (c_x + r_x, c_z + r_x),
                (c_x - r_x, c_z + r_x),
                (c_x, c_z - r_x),
            ];
            inside_polygon(&points, x, z)
        }
        ShapeKind::Hexagon => inside_polygon(
            &ellipse_points(c_x, c_z, r_x, &[60, 120, 180, 240, 300, 360]),
            x,
            z,
        ),
        ShapeKind::Pentagon => inside_polygon(
            &ellipse_points(c_x, c_z, r_x, &[54, 126, 198, 270, 342]),
            x,
            z,
        ),
        ShapeKind::Star => {
            let outer = ellipse_points(c_x, c_z, r_x, &[54, 126, 198, 270, 342]);
            let p = |i: usize| outer[i - 1];
            let inner = |a: (usize, usize), b: (usize, usize), fallback: usize| {
                intersection(p(a.0), p(a.1), p(b.0), p(b.1)).unwrap_or(p(fallback))
            };
            let inner_points = [
                inner((1, 3), (2, 5), 1),
                inner((1, 3), (2, 4), 2),
                inner((2, 4), (3, 5), 3),
                inner((3, 5), (1, 4), 4),
                inner((1, 4), (2, 5), 5),
            ];
            // The five edges of the star, each read as `insideLine(pX, pY)`.
            let edges = [
                inside_line(p(1), p(3), (x, z)),
                inside_line(p(2), p(4), (x, z)),
                inside_line(p(3), p(5), (x, z)),
                inside_line(p(4), p(1), (x, z)),
                inside_line(p(5), p(2), (x, z)),
            ];
            let _ = inner_points;
            // A point is inside when four of the five edges agree, whichever
            // one is the star's notch (`Star.isBounding`).
            [
                [0, 1, 2, 3],
                [1, 2, 3, 4],
                [2, 3, 4, 0],
                [3, 4, 0, 1],
                [4, 0, 1, 2],
            ]
            .iter()
            .any(|set| set.iter().all(|&i| edges[i]))
        }
    }
}

/// The outline of a regular polygon on the ellipse of the given radii: the
/// vertices at the given angles in degrees (`ShapeUtil.pointOnEllipse`).
pub fn ellipse_points(c_x: f64, c_z: f64, r_x: f64, angles: &[i32]) -> Vec<(f64, f64)> {
    angles
        .iter()
        .map(|&angle| {
            let radians = (angle as f64).to_radians();
            (c_x + r_x * radians.cos(), c_z + r_x * radians.sin())
        })
        .collect()
}

/// The star's ten outline points, in the order it draws them.
pub fn star_points(bounding: &Bounding) -> Vec<(f64, f64)> {
    let outer = ellipse_points(
        bounding.center_x,
        bounding.center_z,
        bounding.radius_x,
        &[54, 126, 198, 270, 342],
    );
    let p = |i: usize| outer[i - 1];
    let inner = |a: (usize, usize), b: (usize, usize), fallback: usize| {
        intersection(p(a.0), p(a.1), p(b.0), p(b.1)).unwrap_or(p(fallback))
    };
    vec![
        p(1),
        inner((1, 3), (2, 5), 1),
        p(2),
        inner((1, 3), (2, 4), 2),
        p(3),
        inner((2, 4), (3, 5), 3),
        p(4),
        inner((3, 5), (1, 4), 4),
        p(5),
        inner((1, 4), (2, 5), 5),
    ]
}

/// Whether every edge of the polygon keeps the point on the same side: the
/// polygon's vertices run counterclockwise, so a point inside is left of each
/// edge. The test is strict, as `ShapeUtil.insideLine` is.
fn inside_polygon(points: &[(f64, f64)], x: f64, z: f64) -> bool {
    (0..points.len()).all(|i| inside_line(points[i], points[(i + 1) % points.len()], (x, z)))
}

/// `ShapeUtil.insideLine`: `(bx - ax) * (cz - az) > (bz - az) * (cx - ax)`.
pub fn inside_line(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    (b.0 - a.0) * (c.1 - a.1) > (b.1 - a.1) * (c.0 - a.0)
}

/// Where two lines meet, `None` when they are parallel (`ShapeUtil.intersection`).
pub fn intersection(
    l1: (f64, f64),
    l1b: (f64, f64),
    l2: (f64, f64),
    l2b: (f64, f64),
) -> Option<(f64, f64)> {
    let a1 = l1b.1 - l1.1;
    let a2 = l2b.1 - l2.1;
    let b1 = l1.0 - l1b.0;
    let b2 = l2.0 - l2b.0;
    let det = a1 * b2 - a2 * b1;
    if det == 0.0 {
        return None;
    }
    let c1 = a1 * l1.0 + b1 * l1.1;
    let c2 = a2 * l2.0 + b2 * l2.1;
    Some(((b2 * c1 - b1 * c2) / det, (a1 * c2 - a2 * c1) / det))
}

/// The angles a shape's outline turns through, for the debug map's drawing.
pub const FULL_TURN: f64 = 2.0 * PI;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::SelectionMath;

    fn math(center: f64, radius: f64) -> SelectionMath {
        SelectionMath::new(center, center, radius, radius)
    }

    #[test]
    fn a_chunk_aligned_shape_snaps_to_whole_chunks() {
        // radius 500 blocks -> 32 chunks of radius (ceil(500/16) = 32), so the
        // diameter is 65 chunks and the half-width 65 * 8 = 520.
        let bounding = Bounding::chunk_aligned(&math(0.0, 500.0));
        assert_eq!(bounding.radius_x, 520.0);
        assert_eq!(bounding.center_x, 8.0);
    }

    #[test]
    fn a_square_reads_the_x_radius_for_both_axes() {
        let bounding = Bounding {
            center_x: 0.0,
            center_z: 0.0,
            radius_x: 16.0,
            radius_z: 64.0,
        };
        assert!(is_bounding(ShapeKind::Square, &bounding, 8.0, 8.0));
        assert!(!is_bounding(ShapeKind::Square, &bounding, 8.0, 40.0));
        // The rectangle is the one that honours both.
        assert!(is_bounding(ShapeKind::Rectangle, &bounding, 8.0, 40.0));
        assert!(!is_bounding(ShapeKind::Rectangle, &bounding, 40.0, 8.0));
    }

    #[test]
    fn a_circle_covers_its_radius_and_no_more() {
        let bounding = Bounding {
            center_x: 8.0,
            center_z: 8.0,
            radius_x: 24.0,
            radius_z: 24.0,
        };
        assert!(is_bounding(ShapeKind::Circle, &bounding, 8.0, 8.0));
        assert!(is_bounding(ShapeKind::Circle, &bounding, 24.0, 8.0));
        assert!(!is_bounding(ShapeKind::Circle, &bounding, 40.0, 8.0));
    }

    #[test]
    fn polygon_edges_are_strict() {
        // Exactly on a diamond's edge counts as outside.
        let bounding = Bounding {
            center_x: 0.0,
            center_z: 0.0,
            radius_x: 32.0,
            radius_z: 32.0,
        };
        assert!(is_bounding(ShapeKind::Diamond, &bounding, 0.0, 0.0));
        assert!(is_bounding(ShapeKind::Diamond, &bounding, 8.0, 8.0));
        assert!(!is_bounding(ShapeKind::Diamond, &bounding, 32.0, 0.0));
    }

    #[test]
    fn a_radius_change_picks_the_matching_shape() {
        assert_eq!(
            ShapeKind::Square.with_radii(10.0, 20.0),
            ShapeKind::Rectangle
        );
        assert_eq!(
            ShapeKind::Rectangle.with_radii(10.0, 10.0),
            ShapeKind::Square
        );
        assert_eq!(ShapeKind::Circle.with_radii(10.0, 20.0), ShapeKind::Ellipse);
        assert_eq!(
            ShapeKind::Triangle.with_radii(10.0, 20.0),
            ShapeKind::Triangle
        );
    }

    #[test]
    fn shape_names_round_trip() {
        for shape in SHAPES {
            assert_eq!(ShapeKind::parse(shape.name()), Some(shape));
        }
        assert_eq!(ShapeKind::parse("oval"), None, "not one of ShapeType.all()");
        assert_eq!(ShapeKind::parse_saved("oval"), Some(ShapeKind::Ellipse));
        assert_eq!(ShapeKind::parse("nope"), None);
    }
}
