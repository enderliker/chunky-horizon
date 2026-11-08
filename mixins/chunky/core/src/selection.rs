//! What is about to be generated: a world, a centre, two radii, a shape and a
//! pattern.
//!
//! The port of Chunky's `Selection`. A selection is what the commands edit and
//! what a task is started from; it is small, it is copied into the saved task
//! file, and every derived value a task needs — the chunk the centre is in,
//! the shape snapped to whole chunks — is worked out from it.

use crate::geometry::SelectionMath;
use crate::pattern::{Pattern, PatternKind};
use crate::shape::{Bounding, ShapeKind, is_bounding};

/// The centre a fresh selection starts at.
pub const DEFAULT_CENTER: f64 = 0.0;
/// The radius a fresh selection starts at.
pub const DEFAULT_RADIUS: f64 = 500.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// The world's name, `minecraft:overworld` or `world_nether`.
    pub world: String,
    pub center_x: f64,
    pub center_z: f64,
    pub radius_x: f64,
    pub radius_z: f64,
    pub shape: ShapeKind,
    pub pattern: Pattern,
    /// Chunks a second the task may start, 0 for no limit.
    pub rate_limit: u64,
    /// Generate every chunk in the shape, even the ones already on disk.
    pub force_load_existing_chunks: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            world: String::new(),
            center_x: DEFAULT_CENTER,
            center_z: DEFAULT_CENTER,
            radius_x: DEFAULT_RADIUS,
            radius_z: DEFAULT_RADIUS,
            shape: ShapeKind::Square,
            pattern: Pattern::default_pattern(),
            rate_limit: 0,
            force_load_existing_chunks: false,
        }
    }
}

impl Selection {
    pub fn new(world: impl Into<String>) -> Self {
        Self {
            world: world.into(),
            ..Self::default()
        }
    }

    /// The chunk and region geometry the selection covers.
    pub fn math(&self) -> SelectionMath {
        SelectionMath::new(self.center_x, self.center_z, self.radius_x, self.radius_z)
    }

    /// The shape snapped to whole chunks, which is the one a task filters
    /// with (`ShapeFactory.getShape(selection)`).
    pub fn bounding(&self) -> Bounding {
        Bounding::chunk_aligned(&self.math())
    }

    /// Whether the shape keeps a chunk: the test is made on the chunk's
    /// centre, as every call site in Chunky does.
    pub fn keeps(&self, chunk: crate::geometry::ChunkPos) -> bool {
        let (x, z) = chunk.center();
        is_bounding(self.shape, &self.bounding(), x, z)
    }

    /// Sets the radii and moves the shape with them: a square becomes a
    /// rectangle when the two differ, a circle an ellipse.
    pub fn set_radii(&mut self, radius_x: f64, radius_z: f64) {
        self.radius_x = radius_x;
        self.radius_z = radius_z;
        self.shape = self.shape.with_radii(radius_x, radius_z);
    }

    /// Sets the centre from a pair of corners and picks the shape that fits.
    pub fn set_corners(&mut self, x1: f64, z1: f64, x2: f64, z2: f64) {
        self.center_x = (x1 + x2) / 2.0;
        self.center_z = (z1 + z2) / 2.0;
        let radius_x = (x1 - x2).abs() / 2.0;
        let radius_z = (z1 - z2).abs() / 2.0;
        self.radius_x = radius_x;
        self.radius_z = radius_z;
        self.shape = if radius_x == radius_z {
            ShapeKind::Square
        } else {
            ShapeKind::Rectangle
        };
    }

    /// The radii as the messages print them.
    pub fn radius_text(&self) -> String {
        crate::geometry::format_radius(self.radius_x, self.radius_z)
    }

    /// The pattern with its value, as the task's log names it.
    pub fn pattern_name(&self) -> String {
        self.pattern.describe()
    }

    /// The pattern a task walks this selection in, when it was left unset.
    pub fn with_pattern(mut self, kind: PatternKind, value: Option<String>) -> Self {
        self.pattern = Pattern::with_value(kind, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ChunkPos;

    #[test]
    fn a_fresh_selection_is_chunkys_own() {
        let selection = Selection::new("minecraft:overworld");
        assert_eq!(selection.center_x, 0.0);
        assert_eq!(selection.radius_x, 500.0);
        assert_eq!(selection.shape, ShapeKind::Square);
        assert_eq!(selection.pattern.kind, PatternKind::Ring);
        assert_eq!(selection.rate_limit, 0);
    }

    #[test]
    fn corners_become_a_centre_two_radii_and_a_shape() {
        let mut selection = Selection::default();
        selection.set_corners(-100.0, 0.0, 100.0, 50.0);
        assert_eq!(selection.center_x, 0.0);
        assert_eq!(selection.center_z, 25.0);
        assert_eq!(selection.radius_x, 100.0);
        assert_eq!(selection.radius_z, 25.0);
        assert_eq!(selection.shape, ShapeKind::Rectangle);

        selection.set_corners(-100.0, -100.0, 100.0, 100.0);
        assert_eq!(selection.shape, ShapeKind::Square);
    }

    #[test]
    fn a_square_keeps_the_chunks_inside_its_bounding_box() {
        let mut selection = Selection::default();
        selection.set_corners(-16.0, -16.0, 16.0, 16.0);
        // radius 16 blocks -> 1 chunk of radius, so a 3x3 of chunks.
        assert!(selection.keeps(ChunkPos::new(0, 0)));
        assert!(selection.keeps(ChunkPos::new(1, 1)));
        assert!(!selection.keeps(ChunkPos::new(2, 2)));
    }
}
