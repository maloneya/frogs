//! Uniform 2D broad phase, rebuilt once per collision pass.
//!
//! A counting sort places body rows into cells; pairs come from each cell and
//! four forward neighbours, so every nearby pair is visited exactly once in a
//! stable order that depends only on positions and row order. Cells are wide
//! enough that any contact the solver can admit lies in adjacent cells.

use crate::{ARENA_HALF, MAX_BODIES};
use glam::Vec2;

/// Largest centre distance a pair query must find. Owners size their reach to fit.
pub(crate) const CELL: f32 = 1.0;
pub(crate) const SIDE: usize = (2.0 * ARENA_HALF / CELL) as usize + 1;
pub(crate) const CELLS: usize = SIDE * SIDE;
const _: () = assert!(MAX_BODIES < u32::MAX as usize);
const _: () = assert!(CELLS < u32::MAX as usize);

/// Cell coordinates, collapsing positions outside the arena into edge cells.
/// f64 avoids shifting a near-boundary f32 value into the wrong cell when
/// adding the arena offset; no f64 value writes back to simulation.
pub(crate) fn cell_xz(position: Vec2) -> (usize, usize) {
    let axis = |value: f32| {
        let scaled = (f64::from(value) + f64::from(ARENA_HALF)) / f64::from(CELL);
        // NaN never reaches here; negative and huge values clamp to the edges.
        (scaled.floor().max(0.0) as usize).min(SIDE - 1)
    };
    (axis(position.x), axis(position.y))
}

/// Inclusive cell bounds of an axis-aligned box.
pub(crate) fn cell_box(low: Vec2, high: Vec2) -> ((usize, usize), (usize, usize)) {
    (cell_xz(low), cell_xz(high))
}

pub(crate) struct Grid {
    /// Prefix offsets into `rows`, one per cell plus a terminator.
    start: Vec<u32>,
    cursor: Vec<u32>,
    rows: Vec<u32>,
    cells: Vec<u32>,
    occupied: Vec<u32>,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            start: vec![0; CELLS + 1],
            cursor: vec![0; CELLS],
            // Sized for admission's population bound so rebuilding never allocates.
            rows: Vec::with_capacity(MAX_BODIES),
            cells: Vec::with_capacity(MAX_BODIES),
            occupied: Vec::with_capacity(MAX_BODIES),
        }
    }
}

impl Grid {
    /// Index the rows for which `include` holds. Rows keep ascending order within a cell.
    pub(crate) fn build(&mut self, positions: &[Vec2], include: impl Fn(usize) -> bool) {
        assert!(
            positions.len() <= MAX_BODIES,
            "grid capacity follows physical admission"
        );
        self.start.fill(0);
        self.cells.clear();
        self.occupied.clear();
        for (row, &position) in positions.iter().enumerate() {
            let cell = if include(row) {
                let (x, z) = cell_xz(position);
                let cell = z * SIDE + x;
                self.start[cell + 1] += 1;
                cell as u32
            } else {
                u32::MAX
            };
            self.cells.push(cell);
        }
        // Collecting in this ascending scan keeps visits in cell order, so
        // neighbouring rows stay close in memory without a sort.
        for cell in 0..CELLS {
            if self.start[cell + 1] != 0 {
                self.occupied.push(cell as u32);
            }
            self.start[cell + 1] += self.start[cell];
        }
        self.cursor.copy_from_slice(&self.start[..CELLS]);
        self.rows.clear();
        self.rows.resize(self.start[CELLS] as usize, 0);
        for (row, &cell) in self.cells.iter().enumerate() {
            if cell != u32::MAX {
                let slot = &mut self.cursor[cell as usize];
                self.rows[*slot as usize] = row as u32;
                *slot += 1;
            }
        }
    }

    fn cell(&self, cell: usize) -> &[u32] {
        &self.rows[self.start[cell] as usize..self.start[cell + 1] as usize]
    }

    /// Visit each unordered pair in the same or adjacent cells once. Returns
    /// the number of candidates offered.
    pub(crate) fn pairs(&self, mut visit: impl FnMut(usize, usize)) -> u64 {
        let mut tested = 0;
        for &cell in &self.occupied {
            let cell = cell as usize;
            let (x, z) = (cell % SIDE, cell / SIDE);
            let here = self.cell(cell);
            for (k, &a) in here.iter().enumerate() {
                for &b in &here[k + 1..] {
                    visit(a as usize, b as usize);
                    tested += 1;
                }
            }
            // Forward half of the neighbourhood: east, and the three cells
            // of the next row. The other four see this cell as their forward.
            let mut forward = [None; 4];
            if x + 1 < SIDE {
                forward[0] = Some(cell + 1);
            }
            if z + 1 < SIDE {
                if x > 0 {
                    forward[1] = Some(cell + SIDE - 1);
                }
                forward[2] = Some(cell + SIDE);
                if x + 1 < SIDE {
                    forward[3] = Some(cell + SIDE + 1);
                }
            }
            for other in forward.into_iter().flatten() {
                let there = self.cell(other);
                for &a in here {
                    for &b in there {
                        visit(a as usize, b as usize);
                        tested += 1;
                    }
                }
            }
        }
        tested
    }

    /// Visit rows in cells overlapping the box, in cell then row order.
    pub(crate) fn query(&self, low: Vec2, high: Vec2, mut visit: impl FnMut(usize)) {
        let ((x0, z0), (x1, z1)) = cell_box(low, high);
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &row in self.cell(z * SIDE + x) {
                    visit(row as usize);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(positions: &[Vec2]) -> Vec<(usize, usize)> {
        let mut grid = Grid::default();
        grid.build(positions, |_| true);
        let mut out = Vec::new();
        grid.pairs(|a, b| out.push((a.min(b), a.max(b))));
        out
    }

    #[test]
    fn every_pair_within_one_cell_is_visited_exactly_once() {
        // Deterministic scatter including cell edges and a dense cluster.
        let mut positions = Vec::new();
        let mut seed = 0x9e37_79b9_u32;
        for _ in 0..600 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let x = (seed % 4000) as f32 / 400.0 - 5.0;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let z = (seed % 4000) as f32 / 400.0 - 5.0;
            positions.push(Vec2::new(x, z));
        }
        positions.extend([
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(-0.000_000_1, 0.0),
        ]);
        let mut found = pairs(&positions);
        let visited = found.len();
        found.sort_unstable();
        found.dedup();
        assert_eq!(found.len(), visited, "a pair was visited twice");
        for i in 0..positions.len() {
            for j in i + 1..positions.len() {
                if positions[i].distance(positions[j]) <= CELL {
                    assert!(found.binary_search(&(i, j)).is_ok(), "missing ({i},{j})");
                }
            }
        }
    }

    #[test]
    fn outside_and_edge_positions_collapse_into_edge_cells() {
        let positions = [
            Vec2::new(96.1, -96.5),
            Vec2::new(96.2, -96.5),
            Vec2::new(f32::MAX, f32::MAX),
            Vec2::new(f32::MAX, f32::MAX),
            Vec2::new(-f32::MAX, 0.0),
        ];
        let found = pairs(&positions);
        assert!(found.contains(&(0, 1)) && found.contains(&(2, 3)));
    }

    #[test]
    fn excluded_rows_and_queries_follow_cell_order() {
        let positions = [
            Vec2::new(0.1, 0.1),
            Vec2::new(0.2, 0.1),
            Vec2::new(3.0, 3.0),
        ];
        let mut grid = Grid::default();
        grid.build(&positions, |row| row != 1);
        let mut seen = Vec::new();
        grid.pairs(|a, b| seen.push((a, b)));
        assert!(seen.is_empty());
        let mut hits = Vec::new();
        grid.query(Vec2::splat(-0.5), Vec2::splat(3.5), |row| hits.push(row));
        assert_eq!(hits, [0, 2]);
    }
}
