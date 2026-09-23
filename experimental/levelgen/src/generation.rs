//! Seeded candidate construction, followed by measurement and hard acceptance gates.

use crate::{
    grid::Grid,
    recipe::{self, Random, Recipe},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Biome {
    Cave,
    Tunnels,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) seed: u64,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) biome: Biome,
    pub(crate) passage_width: usize,
    pub(crate) open_min: usize,
    pub(crate) open_max: usize,
    pub(crate) min_route: usize,
    pub(crate) attempts: usize,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            seed: 42,
            width: 72,
            height: 36,
            biome: Biome::Cave,
            passage_width: 2,
            open_min: 25,
            open_max: 65,
            min_route: 30,
            attempts: 32,
        }
    }
}

impl Request {
    // Shared admission boundary: direct callers cannot bypass CLI validation.
    fn validate(&self) -> Result<(), String> {
        if !(16..=160).contains(&self.width) || !(16..=100).contains(&self.height) {
            return Err("width must be 16..160 and height 16..100".into());
        }
        if !(1..=8).contains(&self.passage_width)
            || (self.width - 2) / self.passage_width < 6
            || (self.height - 2) / self.passage_width < 6
        {
            return Err(
                "passage width must be 1..8 and leave at least 6 blocks on each axis".into(),
            );
        }
        if self.open_min == 0 || self.open_min > self.open_max || self.open_max > 100 {
            return Err("open percentages must satisfy 1 <= open-min <= open-max <= 100".into());
        }
        if self.min_route == 0 || self.min_route >= self.width * self.height {
            return Err("min-route must be positive and smaller than the map's cell count".into());
        }
        if !(1..=128).contains(&self.attempts) {
            return Err("attempts must be 1..128".into());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Candidate {
    pub(crate) map: Grid,
    pub(crate) stages: Vec<(&'static str, Grid)>,
    pub(crate) endpoints: Option<(usize, usize)>,
    pub(crate) open: usize,
    pub(crate) reached: usize,
    pub(crate) route: usize,
    pub(crate) violations: Vec<String>,
    pub(crate) attempt: usize,
}

pub(crate) enum Outcome {
    Accepted(Candidate),
    /// Last candidate, retained for diagnosis; never exposed as accepted output.
    Rejected(Candidate),
}

pub(crate) fn generate(request: &Request, recipe: &Recipe) -> Result<Outcome, String> {
    request.validate()?;
    recipe.validate()?;
    let mut random = Random(request.seed);
    for attempt in 1..=request.attempts {
        let (coarse, stages) = recipe::construct(request, recipe, &mut random);
        let map = expand(&coarse, request);
        let candidate = measure(map, stages, attempt, request);
        if candidate.violations.is_empty() {
            return Ok(Outcome::Accepted(candidate));
        }
        if attempt == request.attempts {
            return Ok(Outcome::Rejected(candidate));
        }
    }
    unreachable!("validated requests have at least one attempt")
}

/// Each logical floor cell becomes a full square. Neighboring squares share a
/// full edge, guaranteeing an axis-aligned route of the requested width. This
/// deliberately avoids claiming clearance for arbitrary diagonal movement.
fn expand(coarse: &Grid, request: &Request) -> Grid {
    let mut map = Grid::solid(request.width, request.height);
    let scale = request.passage_width;
    for (i, open) in coarse.cells.iter().enumerate() {
        if !open {
            continue;
        }
        for dy in 0..scale {
            for dx in 0..scale {
                let x = 1 + (i % coarse.width) * scale + dx;
                let y = 1 + (i / coarse.width) * scale + dy;
                map.cells[y * map.width + x] = true;
            }
        }
    }
    map
}

fn farthest(distances: &[Option<usize>]) -> Option<(usize, usize)> {
    // Strict comparison preserves the first row-order cell on ties.
    let mut best = None;
    for (i, distance) in distances.iter().enumerate() {
        if let Some(d) = *distance
            && best.is_none_or(|(_, previous)| d > previous)
        {
            best = Some((i, d));
        }
    }
    best
}

fn measure(
    map: Grid,
    stages: Vec<(&'static str, Grid)>,
    attempt: usize,
    request: &Request,
) -> Candidate {
    let open = map.cells.iter().filter(|cell| **cell).count();
    let mut endpoints = None;
    let mut reached = 0;
    let mut route = 0;
    if let Some(first) = map.cells.iter().position(|cell| *cell) {
        let (start, _) = farthest(&map.distances(first)).expect("first is open");
        let distance = map.distances(start);
        reached = distance.iter().flatten().count();
        let (exit, length) = farthest(&distance).expect("start is open");
        if start != exit {
            endpoints = Some((start, exit));
        }
        route = length;
    }
    let mut violations = Vec::new();
    // Integer cross-products avoid rounding a failing percentage into acceptance.
    if open * 100 < request.open_min * map.cells.len()
        || open * 100 > request.open_max * map.cells.len()
    {
        violations.push(format!(
            "open area is outside {}..{}%",
            request.open_min, request.open_max
        ));
    }
    if open == 0 || reached != open {
        violations.push("floor is empty or disconnected".into());
    }
    if endpoints.is_none() {
        violations.push("cannot place distinct start and exit".into());
    }
    if route < request.min_route {
        violations.push(format!("route is shorter than {} cells", request.min_route));
    }
    Candidate {
        map,
        stages,
        endpoints,
        open,
        reached,
        route,
        violations,
        attempt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(request: &Request) -> Candidate {
        match generate(request, &Recipe::default()).unwrap() {
            Outcome::Accepted(candidate) => candidate,
            Outcome::Rejected(candidate) => panic!("{:?}", candidate.violations),
        }
    }

    #[test]
    fn both_biomes_are_repeatable_connected_and_meet_the_requested_budgets() {
        for biome in [Biome::Cave, Biome::Tunnels] {
            for seed in 0..16 {
                let request = Request {
                    biome,
                    seed,
                    ..Request::default()
                };
                let a = accepted(&request);
                let b = accepted(&request);
                assert_eq!(a.map, b.map);
                assert_eq!(a.endpoints, b.endpoints);
                assert_eq!(a.attempt, b.attempt);
                let (start, exit) = a.endpoints.unwrap();
                let distances = a.map.distances(start);
                assert_eq!(distances.iter().flatten().count(), a.open);
                assert!(distances[exit].unwrap() >= request.min_route);
                assert!(a.open * 100 >= request.open_min * a.map.cells.len());
                assert!(a.open * 100 <= request.open_max * a.map.cells.len());
            }
        }
    }

    #[test]
    fn width_expansion_preserves_full_square_passages_and_solid_border() {
        let request = Request {
            width: 16,
            height: 16,
            passage_width: 2,
            ..Request::default()
        };
        let mut coarse = Grid::solid(7, 7);
        for i in [0, 1, 8] {
            coarse.cells[i] = true;
        }
        let map = expand(&coarse, &request);
        assert_eq!(map.cells.iter().filter(|c| **c).count(), 12);
        for y in 0..16 {
            for x in 0..16 {
                let expected = (1..=4).contains(&x) && (1..=2).contains(&y)
                    || (3..=4).contains(&x) && (3..=4).contains(&y);
                assert_eq!(map.cells[y * 16 + x], expected, "{x},{y}");
            }
        }
    }

    #[test]
    fn invalid_inputs_are_refused_before_allocation() {
        for request in [
            Request {
                width: usize::MAX,
                ..Request::default()
            },
            Request {
                passage_width: 0,
                ..Request::default()
            },
            Request {
                open_min: 70,
                open_max: 30,
                ..Request::default()
            },
            Request {
                attempts: 0,
                ..Request::default()
            },
        ] {
            assert!(generate(&request, &Recipe::default()).is_err());
        }
    }

    #[test]
    fn impossible_requirements_return_a_rejected_map_after_the_attempt_budget() {
        let request = Request {
            open_min: 100,
            open_max: 100,
            attempts: 2,
            ..Request::default()
        };
        let Outcome::Rejected(candidate) = generate(&request, &Recipe::default()).unwrap() else {
            panic!("a solid border makes 100% floor impossible");
        };
        assert_eq!(candidate.attempt, 2);
        assert!(candidate.violations.iter().any(|v| v.contains("open area")));
    }

    #[test]
    fn acceptance_uses_exact_counts_not_rounded_percentages() {
        let mut map = Grid::solid(10, 10);
        map.cells[..24].fill(true);
        let request = Request {
            open_min: 25,
            min_route: 1,
            ..Request::default()
        };
        let candidate = measure(map, Vec::new(), 1, &request);
        assert!(candidate.violations.iter().any(|v| v.contains("open area")));
    }
}
