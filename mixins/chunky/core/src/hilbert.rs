//! The Hilbert curve a fully-read region file is walked in.
//!
//! Chunky embeds the offsets of a 5th-order Hilbert curve over a 32x32 region
//! as a 1024-entry table (`util/Hilbert.regionChunkCoordinateOffsets`). The
//! table is the standard Hilbert `d -> (x, y)` mapping of order 5 with its two
//! axes transposed, so this generates it instead of carrying a thousand
//! literals: the port's order is the same one, which is what matters for a
//! resumed task to walk a region the way it would have.
//!
//! The table's own first entries, `(0,0) (0,1) (1,1) (1,0) (2,0) (3,0) (3,1)
//! (2,1) (2,2) ...` and its last `(31,0)`, are the tests below: they are the
//! standard order-1 `U` repeated, with no swap of the two axes.

use crate::geometry::ChunkPos;

/// The curve's order: `2^5 = 32`, one region file's side.
const ORDER: u32 = 32;
/// How many offsets the curve visits.
pub const OFFSETS: usize = (ORDER * ORDER) as usize;

/// The offset inside a region of the `distance`-th chunk the curve reaches.
pub fn offset(distance: u32) -> ChunkPos {
    let (x, y) = d2xy(ORDER, distance & (OFFSETS as u32 - 1));
    ChunkPos::new(x as i32, y as i32)
}

/// Every offset, in curve order.
pub fn offsets() -> impl Iterator<Item = ChunkPos> {
    (0..OFFSETS as u32).map(offset)
}

/// The standard `d -> (x, y)` Hilbert mapping (Wikipedia's `d2xy`), with the
/// rotation the curve needs at each level.
fn d2xy(n: u32, d: u32) -> (u32, u32) {
    let (mut x, mut y) = (0u32, 0u32);
    let mut t = d;
    let mut s = 1u32;
    while s < n {
        let rx = 1 & (t / 2);
        let ry = 1 & (t ^ rx);
        if ry == 0 {
            if rx == 1 {
                x = s.wrapping_sub(1).wrapping_sub(x);
                y = s.wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += s * rx;
        y += s * ry;
        t /= 4;
        s *= 2;
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first sixteen entries and the last one, as the Java table has them.
    #[test]
    fn the_curve_matches_chunkys_table() {
        let first: Vec<(i32, i32)> = (0..16)
            .map(|d| {
                let p = offset(d);
                (p.x, p.z)
            })
            .collect();
        assert_eq!(
            first,
            vec![
                (0, 0),
                (0, 1),
                (1, 1),
                (1, 0),
                (2, 0),
                (3, 0),
                (3, 1),
                (2, 1),
                (2, 2),
                (3, 2),
                (3, 3),
                (2, 3),
                (1, 3),
                (1, 2),
                (0, 2),
                (0, 3),
            ]
        );
        assert_eq!(offset(16), ChunkPos::new(0, 4));
        let last = offset(OFFSETS as u32 - 1);
        assert_eq!((last.x, last.z), (31, 0));
    }

    /// Every cell is visited exactly once.
    #[test]
    fn the_curve_covers_the_region_once() {
        let mut seen = vec![false; OFFSETS];
        for offset in offsets() {
            assert!(
                offset.x < 32 && offset.z < 32,
                "{offset:?} is outside the region"
            );
            let index = (offset.x + offset.z * 32) as usize;
            assert!(!seen[index], "{offset:?} is visited twice");
            seen[index] = true;
        }
        assert!(seen.into_iter().all(|v| v));
    }
}
