//! Uniform 2D broad phase for the crowd's equal-radius discs.
//!
//! Scratch only: every solve rebuilds from the current dense rows after motion.
//! No entity identity or membership survives into the next solve. The only API
//! that visits pairs also owns position mutation and repairs both cell links
//! before another query, so callers cannot query a stale spatial index.

use glam::Vec2;
use crate::{ARENA_HALF, ENEMY_RADIUS, MAX_BODIES};

const CELL_SIZE: f32 = 2.0 * ENEMY_RADIUS;
const SIDE: usize = (2.0 * ARENA_HALF / CELL_SIZE) as usize + 1;
const CELLS: usize = SIDE * SIDE;
const NONE: u32 = u32::MAX;
const _: () = assert!(MAX_BODIES < NONE as usize);

// Small contiguous pages avoid one cache-dependent pointer chase per body in
// crowded cells. Only the head page may be partially full; removal fills a hole
// from that head, so updating membership remains constant-time.
const PAGE_SIZE: usize = 8;
const _: () = assert!(MAX_BODIES * PAGE_SIZE < NONE as usize);
const _: () = assert!(CELLS < NONE as usize);

#[derive(Clone, Copy)]
struct Page { rows: [u32; PAGE_SIZE], len: u32, next: u32 }
const EMPTY_PAGE: Page = Page { rows: [0; PAGE_SIZE], len: 0, next: NONE };

pub(crate) struct Grid {
    heads: Vec<u32>,
    pages: Vec<Page>,
    allocated: usize,
    free: Vec<u32>,
    cells: Vec<u32>,
    locations: Vec<u32>,
    candidates: Vec<u64>,
    /// During one outer row, only i and already-visited j rows can move.
    /// A cell already scanned cannot acquire a new unvisited row in that sweep.
    seen: Vec<u32>,
    /// Test-only oracle selection; production has exactly one implementation.
    #[cfg(test)]
    pub(crate) brute_force: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            heads: vec![NONE; CELLS],
            pages: vec![EMPTY_PAGE; MAX_BODIES],
            allocated: 0,
            free: Vec::with_capacity(MAX_BODIES),
            cells: vec![0; MAX_BODIES],
            locations: vec![0; MAX_BODIES],
            candidates: vec![0; MAX_BODIES.div_ceil(64)],
            seen: vec![NONE; CELLS],
            #[cfg(test)]
            brute_force: false,
        }
    }
}

impl Grid {
    /// Visit potentially touching pairs in the original lexicographic (i,j)
    /// order. `visit` may change only these two positions. Re-query after i
    /// crosses a cell, keeping j strictly increasing: newly reachable higher
    /// rows must be found, but already passed pairs must never be revisited.
    pub(crate) fn solve(&mut self, positions: &mut [Vec2], mut visit: impl FnMut(usize, usize, &mut Vec2, &mut Vec2)) -> u64 {
        assert!(positions.len() <= MAX_BODIES, "grid capacity follows physical admission");
        // No pair can exist. A later nontrivial solve rebuilds all scratch.
        if positions.len() < 2 { return 0; }
        self.heads.fill(NONE);
        self.allocated = 0;
        self.free.clear();
        self.seen.fill(NONE);
        for (row, &position) in positions.iter().enumerate() {
            self.insert(row, cell(position));
        }
        let words = positions.len().div_ceil(64);
        let mut tested = 0;
        for i in 0..positions.len() - 1 {
            self.candidates[..words].fill(0);
            self.collect(self.cells[i] as usize, i + 1, i as u32);
            let mut minimum = i + 1;
            while let Some(j) = self.next_candidate(minimum, words) {
                let old_a = positions[i];
                let old_b = positions[j];
                let (head, tail) = positions.split_at_mut(j);
                visit(i, j, &mut head[i], &mut tail[0]);
                tested += 1;
                // Most broad-phase candidates are not contacts. Avoid index
                // arithmetic unless the callback actually changed a position.
                let moved = positions[i] != old_a && self.update(i, positions[i]);
                if positions[j] != old_b { self.update(j, positions[j]); }
                minimum = j + 1;
                if moved { self.collect(self.cells[i] as usize, minimum, i as u32); }
            }
        }
        tested
    }

    fn insert(&mut self, row: usize, cell: usize) {
        let mut head = self.heads[cell];
        if head == NONE || self.pages[head as usize].len as usize == PAGE_SIZE {
            let page = self.free.pop().unwrap_or_else(|| {
                assert!(self.allocated < self.pages.len(), "one page per live body suffices");
                let page = self.allocated as u32;
                self.allocated += 1;
                page
            });
            self.pages[page as usize] = Page { next: head, ..EMPTY_PAGE };
            self.heads[cell] = page;
            head = page;
        }
        let page = &mut self.pages[head as usize];
        self.locations[row] = head * PAGE_SIZE as u32 + page.len;
        self.cells[row] = cell as u32;
        page.rows[page.len as usize] = row as u32;
        page.len += 1;
    }

    fn update(&mut self, row: usize, position: Vec2) -> bool {
        let new_cell = cell(position);
        let old_cell = self.cells[row] as usize;
        if old_cell == new_cell { return false; }
        let head = self.heads[old_cell] as usize;
        let last = self.pages[head].len as usize - 1;
        let replacement = self.pages[head].rows[last];
        let location = self.locations[row] as usize;
        self.pages[location / PAGE_SIZE].rows[location % PAGE_SIZE] = replacement;
        self.locations[replacement as usize] = location as u32;
        self.pages[head].len -= 1;
        if last == 0 {
            self.heads[old_cell] = self.pages[head].next;
            self.free.push(head as u32);
        }
        self.insert(row, new_cell);
        true
    }

    fn collect(&mut self, cell: usize, minimum: usize, outer: u32) {
        let (x, z) = (cell % SIDE, cell / SIDE);
        for z in z.saturating_sub(1)..=(z + 1).min(SIDE - 1) {
            for x in x.saturating_sub(1)..=(x + 1).min(SIDE - 1) {
                let cell = z * SIDE + x;
                if self.seen[cell] == outer { continue; }
                self.seen[cell] = outer;
                let mut page = self.heads[cell];
                while page != NONE {
                    let entries = &self.pages[page as usize];
                    for &row in &entries.rows[..entries.len as usize] {
                        if row as usize >= minimum {
                            self.candidates[row as usize / 64] |= 1 << (row % 64);
                        }
                    }
                    page = entries.next;
                }
            }
        }
        // Union in newly reachable rows instead of sorting/replacing a list.
        // Old candidates are safe false positives: the exact disc test rejects
        // them if i has moved away. The increasing cursor prevents revisits.
    }

    fn next_candidate(&self, minimum: usize, words: usize) -> Option<usize> {
        let mut word = minimum / 64;
        if word >= words { return None; }
        let mut bits = self.candidates[word] & (u64::MAX << (minimum % 64));
        loop {
            if bits != 0 { return Some(word * 64 + bits.trailing_zeros() as usize); }
            word += 1;
            if word == words { return None; }
            bits = self.candidates[word];
        }
    }

}

fn cell(position: Vec2) -> usize {
    // The pass precedes containment, and authored movable bodies may start
    // anywhere finite. Collapse out-of-arena coordinates into edge cells rather
    // than clamping their physical positions or refusing previously valid input.
    // This is conservative (possibly slower), including huge finite coordinates.
    // f64 avoids shifting a near-boundary f32 value into the wrong cell when
    // adding the arena offset; no f64 value writes back to simulation.
    let axis = |value: f32| {
        (((f64::from(value) + f64::from(ARENA_HALF)) / f64::from(CELL_SIZE)).floor() as usize).min(SIDE - 1)
    };
    axis(position.y) * SIDE + axis(position.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_across_cells_discovers_later_pairs_without_revisiting_earlier_ones() {
        let mut grid = Grid::default();
        let mut positions = [Vec2::new(0.49, 0.0), Vec2::new(0.48, 0.0), Vec2::new(1.01, 0.0)];
        let mut visited = Vec::new();
        grid.solve(&mut positions, |i, j, a, _| {
            visited.push((i, j));
            if (i, j) == (0, 1) { a.x = 0.74; }
        });
        assert_eq!(visited, [(0, 1), (0, 2)]);
    }

    #[test]
    fn updating_the_second_body_is_visible_to_later_outer_rows() {
        let mut grid = Grid::default();
        let mut positions = [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.1, 0.0)];
        let mut visited = Vec::new();
        grid.solve(&mut positions, |i, j, _, b| {
            visited.push((i, j));
            if (i, j) == (0, 2) { b.x = 2.1; }
        });
        assert_eq!(visited, [(0, 2), (1, 2)]);
    }

    #[test]
    fn returning_to_a_previous_cell_retains_its_unvisited_candidates() {
        let mut grid = Grid::default();
        let mut positions = [Vec2::new(0.49, 0.0), Vec2::new(0.48, 0.0),
            Vec2::new(1.01, 0.0), Vec2::new(0.01, 0.0)];
        let mut visited = Vec::new();
        grid.solve(&mut positions, |i, j, a, _| {
            if i == 0 {
                visited.push(j);
                if j == 1 { a.x = 0.74; }
                if j == 2 { a.x = -0.01; }
            }
        });
        assert_eq!(visited, [1, 2, 3]);
    }

    #[test]
    fn boundaries_and_far_outside_positions_remain_conservative() {
        let mut grid = Grid::default();
        let positions = [
            Vec2::new(-0.000_000_1, 0.0), Vec2::new(0.499, 0.0),
            Vec2::new(96.1, -96.5), Vec2::new(96.2, -96.5),
            Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MAX, f32::MAX),
        ];
        let mut visited = Vec::new();
        let mut live = positions;
        grid.solve(&mut live, |i, j, _, _| visited.push((i, j)));
        for i in 0..positions.len() {
            for j in i + 1..positions.len() {
                if positions[i].distance_squared(positions[j]) < CELL_SIZE * CELL_SIZE {
                    assert!(visited.contains(&(i, j)), "missing ({i},{j})");
                }
            }
        }
        assert!(visited.windows(2).all(|p| p[0] < p[1]));
    }
}
