//! Bounded generator recipe: the only values a research proposer may change.
use crate::{
    generation::{Biome, Request},
    grid::Grid,
};

// Name, minimum, maximum, search step. Validation and proposer schema share these bounds.
const FIELDS: [(&str, usize, usize, usize); 4] = [
    ("rock_percent", 30, 60, 3),
    ("smoothing_passes", 0, 8, 1),
    ("carve_percent", 20, 70, 5),
    ("straight_percent", 0, 90, 15),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recipe {
    pub(crate) rock_percent: usize,
    pub(crate) smoothing_passes: usize,
    pub(crate) carve_percent: usize,
    pub(crate) straight_percent: usize,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            rock_percent: 45,
            smoothing_passes: 5,
            carve_percent: 45,
            straight_percent: 0,
        }
    }
}

impl Recipe {
    fn values(&self) -> [usize; 4] {
        [
            self.rock_percent,
            self.smoothing_passes,
            self.carve_percent,
            self.straight_percent,
        ]
    }
    pub(crate) fn schema() -> String {
        let defaults = Self::default().values();
        let fields = FIELDS.iter().zip(defaults).map(|((name,min,max,step),default)|
            format!("{{\"name\":\"{name}\",\"min\":{min},\"max\":{max},\"step\":{step},\"default\":{default}}}"))
            .collect::<Vec<_>>().join(",");
        format!("{{\"fields\":[{fields}]}}\n")
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        for ((name, min, max, _), value) in FIELDS.iter().zip(self.values()) {
            if !(*min..=*max).contains(&value) {
                return Err(format!("{name} must be {min}..{max}"));
            }
        }
        Ok(())
    }

    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut values = [None; 4];
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or("recipe lines must be key=value")?;
            let index = FIELDS
                .iter()
                .position(|(name, _, _, _)| *name == key.trim())
                .ok_or_else(|| format!("unknown recipe key {key}"))?;
            if values[index].is_some() {
                return Err(format!("duplicate recipe key {key}"));
            }
            values[index] = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "recipe values must be integers")?,
            );
        }
        let [
            Some(rock_percent),
            Some(smoothing_passes),
            Some(carve_percent),
            Some(straight_percent),
        ] = values
        else {
            return Err("recipe requires all four keys".into());
        };
        let recipe = Self {
            rock_percent,
            smoothing_passes,
            carve_percent,
            straight_percent,
        };
        recipe.validate()?;
        Ok(recipe)
    }

    pub(crate) fn text(&self) -> String {
        FIELDS
            .iter()
            .zip(self.values())
            .map(|((name, _, _, _), value)| format!("{name}={value}\n"))
            .collect()
    }
}
/// SplitMix64: explicit wrapping operations make the sequence build-mode independent.
pub(crate) struct Random(pub(crate) u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

pub(crate) fn construct(
    request: &Request,
    recipe: &Recipe,
    random: &mut Random,
) -> (Grid, Vec<(&'static str, Grid)>) {
    let mut grid = Grid::solid(
        (request.width - 2) / request.passage_width,
        (request.height - 2) / request.passage_width,
    );
    let mut stages = Vec::new();
    match request.biome {
        Biome::Cave => {
            for cell in &mut grid.cells {
                *cell = random.below(100) >= recipe.rock_percent;
            }
            stages.push(("random cells", grid.clone()));
            for _ in 0..recipe.smoothing_passes {
                grid = smooth(&grid);
            }
            stages.push(("after smoothing", grid.clone()));
        }
        Biome::Tunnels => {
            let mut x = grid.width / 2;
            let mut y = grid.height / 2;
            // A bounded walk; the recipe guides carving but acceptance uses the
            // expanded map's actual area, including its solid border.
            let target = grid.cells.len() * recipe.carve_percent / 100;
            let mut direction = random.below(4);
            let mut carved = 0;
            for _ in 0..grid.cells.len() * 20 {
                let i = y * grid.width + x;
                if !grid.cells[i] {
                    grid.cells[i] = true;
                    carved += 1;
                }
                if carved >= target {
                    break;
                }
                if random.below(100) >= recipe.straight_percent {
                    direction = random.below(4);
                }
                match direction {
                    0 => x = x.saturating_sub(1),
                    1 => x = (x + 1).min(grid.width - 1),
                    2 => y = y.saturating_sub(1),
                    _ => y = (y + 1).min(grid.height - 1),
                }
            }
            stages.push(("carved walk", grid.clone()));
        }
    }
    grid.keep_largest();
    stages.push(("largest connected region", grid.clone()));
    (grid, stages)
}

/// Synchronous update: all decisions read the old grid, including unchanged ties.
fn smooth(grid: &Grid) -> Grid {
    let mut next = grid.clone();
    for y in 0..grid.height {
        for x in 0..grid.width {
            let mut rock = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x.checked_add_signed(dx);
                    let ny = y.checked_add_signed(dy);
                    let open = nx.zip(ny).is_some_and(|(nx, ny)| {
                        nx < grid.width && ny < grid.height && grid.cells[ny * grid.width + nx]
                    });
                    rock += usize::from(!open);
                }
            }
            next.cells[y * grid.width + x] = match rock {
                0..=3 => true,
                5..=8 => false,
                _ => grid.cells[y * grid.width + x],
            };
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_floor_disappears_and_isolated_rock_fills_in() {
        let mut grid = Grid::solid(5, 5);
        grid.cells[12] = true;
        assert!(!smooth(&grid).cells[12]);
        grid.cells.fill(true);
        grid.cells[12] = false;
        assert!(smooth(&grid).cells[12]);
    }

    #[test]
    fn recipe_input_rejects_missing_duplicate_and_out_of_range_fields() {
        let valid = Recipe::default().text();
        assert_eq!(Recipe::parse(&valid).unwrap(), Recipe::default());
        for invalid in [
            String::new(),
            format!("{valid}rock_percent=40"),
            valid.replace("45", "999"),
            valid.replace("rock_percent", "other"),
        ] {
            assert!(Recipe::parse(&invalid).is_err());
        }
    }
}
