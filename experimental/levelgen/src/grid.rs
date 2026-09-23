//! Boolean cell storage and four-neighbor traversal, independent of generation.

use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Grid {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) cells: Vec<bool>,
}

impl Grid {
    pub(crate) fn solid(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    pub(crate) fn neighbors(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let x = i % self.width;
        let y = i / self.width;
        [
            (x > 0).then(|| i - 1),
            (x + 1 < self.width).then_some(i + 1),
            (y > 0).then(|| i - self.width),
            (y + 1 < self.height).then_some(i + self.width),
        ]
        .into_iter()
        .flatten()
    }

    pub(crate) fn distances(&self, start: usize) -> Vec<Option<usize>> {
        let mut distance = vec![None; self.cells.len()];
        if !self.cells[start] {
            return distance;
        }
        distance[start] = Some(0);
        let mut pending = VecDeque::from([start]);
        while let Some(i) = pending.pop_front() {
            let next = distance[i].expect("queued cells have a distance") + 1;
            for n in self.neighbors(i) {
                if self.cells[n] && distance[n].is_none() {
                    distance[n] = Some(next);
                    pending.push_back(n);
                }
            }
        }
        distance
    }

    /// Keep the largest component, breaking ties by first cell in row order.
    pub(crate) fn keep_largest(&mut self) {
        let mut seen = vec![false; self.cells.len()];
        let mut largest = Vec::new();
        for start in 0..self.cells.len() {
            if !self.cells[start] || seen[start] {
                continue;
            }
            let mut region = vec![start];
            seen[start] = true;
            let mut cursor = 0;
            while cursor < region.len() {
                let i = region[cursor];
                cursor += 1;
                for n in self.neighbors(i) {
                    if self.cells[n] && !seen[n] {
                        seen[n] = true;
                        region.push(n);
                    }
                }
            }
            if region.len() > largest.len() {
                largest = region;
            }
        }
        self.cells.fill(false);
        for i in largest {
            self.cells[i] = true;
        }
    }

    pub(crate) fn ascii(&self, endpoints: Option<(usize, usize)>) -> String {
        let mut text = String::with_capacity(self.cells.len() + self.height);
        for (i, open) in self.cells.iter().enumerate() {
            text.push(match endpoints {
                Some((start, _)) if i == start => 'S',
                Some((_, exit)) if i == exit => 'E',
                _ if *open => '.',
                _ => '#',
            });
            if i % self.width == self.width - 1 {
                text.push('\n');
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_does_not_wrap_rows_or_cross_diagonals() {
        let mut grid = Grid::solid(3, 3);
        for i in [2, 3, 7, 8] {
            grid.cells[i] = true;
        }
        assert_eq!(grid.distances(2).iter().flatten().count(), 1);
        grid.keep_largest();
        assert_eq!(
            grid.cells,
            vec![false, false, false, false, false, false, false, true, true]
        );
    }

    #[test]
    fn distance_follows_the_route_instead_of_straight_line() {
        let mut grid = Grid::solid(3, 3);
        grid.cells = vec![true, false, true, true, false, true, true, true, true];
        assert_eq!(grid.distances(0)[2], Some(6));
    }
}
