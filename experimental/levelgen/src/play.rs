//! Deterministic navigation sessions. Policies receive only remembered observations.
use crate::grid::Grid;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    Seeker,
    Explorer,
}
impl Profile {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Seeker => "seeker",
            Self::Explorer => "explorer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    North,
    South,
    West,
    East,
}
impl Action {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        match text {
            "north" => Ok(Self::North),
            "south" => Ok(Self::South),
            "west" => Ok(Self::West),
            "east" => Ok(Self::East),
            _ => Err("action must be north, south, west or east".into()),
        }
    }
}

/// No reference to hidden terrain or undiscovered exit coordinates is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Observation {
    width: usize,
    height: usize,
    position: usize,
    known: Vec<Option<bool>>,
    exit: Option<usize>,
}
impl Observation {
    pub(crate) fn ascii(&self) -> String {
        let mut text = String::new();
        for (i, cell) in self.known.iter().enumerate() {
            text.push(if i == self.position {
                '@'
            } else if self.exit == Some(i) {
                'E'
            } else {
                match cell {
                    None => '?',
                    Some(true) => '.',
                    Some(false) => '#',
                }
            });
            if i % self.width == self.width - 1 {
                text.push('\n');
            }
        }
        text
    }
    fn neighbors(&self, i: usize) -> impl Iterator<Item = usize> {
        let (x, y) = (i % self.width, i / self.width);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < self.width).then_some(i + 1),
            (y > 0).then(|| i - self.width),
            (y + 1 < self.height).then_some(i + self.width),
        ]
        .into_iter()
        .flatten()
    }
    fn frontier(&self, i: usize) -> bool {
        self.known[i] == Some(true) && self.neighbors(i).any(|n| self.known[n].is_none())
    }
}

pub(crate) struct Session {
    map: Grid,
    start: usize,
    exit: usize,
    profile: Profile,
    observation: Observation,
    visits: Vec<usize>,
    limit: usize,
    steps: usize,
    discovered_exit: Option<usize>,
    trace: Vec<Event>,
}
#[derive(Debug)]
struct Event {
    action: Option<Action>,
    position: usize,
    revealed: usize,
    revisit: bool,
    blocked: bool,
}

impl Session {
    pub(crate) fn new(
        map: Grid,
        start: usize,
        exit: usize,
        profile: Profile,
        limit: usize,
    ) -> Result<Self, String> {
        if map.width == 0
            || map.height == 0
            || map.width > 160
            || map.height > 100
            || map.width * map.height != map.cells.len()
            || start == exit
            || !map.cells.get(start).copied().unwrap_or(false)
            || !map.cells.get(exit).copied().unwrap_or(false)
            || limit == 0
            || limit > 100_000
        {
            return Err("invalid navigation session".into());
        }
        let cells = map.cells.len();
        let mut game = Self {
            observation: Observation {
                width: map.width,
                height: map.height,
                position: start,
                known: vec![None; cells],
                exit: None,
            },
            map,
            start,
            exit,
            profile,
            visits: vec![0; cells],
            limit,
            steps: 0,
            discovered_exit: None,
            trace: Vec::new(),
        };
        game.visits[start] = 1;
        let revealed = game.reveal();
        game.trace.push(Event {
            action: None,
            position: start,
            revealed,
            revisit: false,
            blocked: false,
        });
        Ok(game)
    }
    pub(crate) fn observe(&self) -> &Observation {
        &self.observation
    }
    pub(crate) fn completed(&self) -> bool {
        self.observation.position == self.exit
            && (self.profile == Profile::Seeker
                || self
                    .map
                    .cells
                    .iter()
                    .zip(&self.observation.known)
                    .all(|(open, known)| !open || known.is_some()))
    }
    pub(crate) fn finished(&self) -> bool {
        self.completed() || self.steps >= self.limit
    }
    pub(crate) fn steps(&self) -> usize {
        self.steps
    }

    /// A move consumes one turn, even when blocked. Finished sessions refuse actions.
    pub(crate) fn step(&mut self, action: Action) -> Result<(), String> {
        if self.finished() {
            return Err("session has finished".into());
        }
        let old = self.observation.position;
        let next = match action {
            Action::North => (old >= self.map.width).then(|| old - self.map.width),
            Action::South => {
                (old + self.map.width < self.map.cells.len()).then_some(old + self.map.width)
            }
            Action::West => (!old.is_multiple_of(self.map.width)).then(|| old - 1),
            Action::East => (old % self.map.width + 1 < self.map.width).then_some(old + 1),
        };
        let blocked = !next.is_some_and(|i| self.map.cells[i]);
        if let Some(i) = next.filter(|i| self.map.cells[*i]) {
            self.observation.position = i;
        }
        self.steps += 1;
        let position = self.observation.position;
        let revisit = self.visits[position] > 0;
        self.visits[position] += 1;
        let revealed = self.reveal();
        self.trace.push(Event {
            action: Some(action),
            position,
            revealed,
            revisit,
            blocked,
        });
        Ok(())
    }

    // Local sensing follows up to three floor edges and reveals adjacent walls.
    // This is deliberately a topological sensor, not optical line of sight.
    fn reveal(&mut self) -> usize {
        let mut revealed = 0;
        let mut seen = vec![false; self.map.cells.len()];
        let mut pending = VecDeque::from([(self.observation.position, 0)]);
        seen[self.observation.position] = true;
        while let Some((i, d)) = pending.pop_front() {
            if self.observation.known[i].is_none() && self.map.cells[i] {
                revealed += 1;
            }
            self.observation.known[i] = Some(self.map.cells[i]);
            if d < 3 && self.map.cells[i] {
                for n in self.map.neighbors(i) {
                    if !seen[n] {
                        seen[n] = true;
                        pending.push_back((n, d + 1));
                    }
                }
            }
        }
        if self.observation.known[self.exit].is_some() {
            self.observation.exit = Some(self.exit);
            if self.discovered_exit.is_none() {
                self.discovered_exit = Some(self.steps);
            }
        }
        revealed
    }

    pub(crate) fn result(&self) -> PlayResult {
        PlayResult {
            profile: self.profile,
            completed: self.completed(),
            steps: self.steps,
            revisits: self.trace.iter().skip(1).filter(|e| e.revisit).count(),
            discovered_exit: self.discovered_exit,
            revealed: self
                .observation
                .known
                .iter()
                .filter(|c| **c == Some(true))
                .count(),
            floor: self.map.cells.iter().filter(|c| **c).count(),
            trace: self.trace_text(),
            overlay: self.overlay(),
        }
    }
    fn trace_text(&self) -> String {
        let mut text = "tick\taction\tx\ty\tnew_floor\trevisit\tblocked\n".to_owned();
        for (tick, event) in self.trace.iter().enumerate() {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                tick,
                event.action.map_or("reset", Action::name),
                event.position % self.map.width,
                event.position / self.map.width,
                event.revealed,
                event.revisit,
                event.blocked
            ));
        }
        text
    }
    fn overlay(&self) -> String {
        let mut text = String::new();
        for (i, open) in self.map.cells.iter().enumerate() {
            text.push(if i == self.start {
                'S'
            } else if i == self.exit {
                'E'
            } else if !open {
                '#'
            } else if self.visits[i] > 1 {
                '+'
            } else if self.visits[i] == 1 {
                '*'
            } else {
                '.'
            });
            if i % self.map.width == self.map.width - 1 {
                text.push('\n');
            }
        }
        text
    }
}

/// Breadth-first search through remembered floor only. Stable neighbor order
/// makes each policy repeatable; no level state or diagnostics are passed in.
fn choose(observation: &Observation, profile: Profile) -> Option<Action> {
    let mut previous = vec![None; observation.known.len()];
    let mut pending = VecDeque::from([observation.position]);
    previous[observation.position] = Some(observation.position);
    let mut nearest_frontier = None;
    while let Some(i) = pending.pop_front() {
        if nearest_frontier.is_none() && observation.frontier(i) {
            nearest_frontier = Some(i);
        }
        for n in observation.neighbors(i) {
            if observation.known[n] == Some(true) && previous[n].is_none() {
                previous[n] = Some(i);
                pending.push_back(n);
            }
        }
    }
    let visible_exit = observation.exit.filter(|i| previous[*i].is_some());
    let mut target = match profile {
        Profile::Seeker => visible_exit.or(nearest_frontier),
        Profile::Explorer => nearest_frontier.or(visible_exit),
    }?;
    if target == observation.position {
        return None;
    }
    while previous[target]? != observation.position {
        target = previous[target]?;
    }
    Some(if target + 1 == observation.position {
        Action::West
    } else if target == observation.position + 1 {
        Action::East
    } else if target < observation.position {
        Action::North
    } else {
        Action::South
    })
}

pub(crate) struct PlayResult {
    pub(crate) profile: Profile,
    pub(crate) completed: bool,
    pub(crate) steps: usize,
    pub(crate) revisits: usize,
    pub(crate) discovered_exit: Option<usize>,
    pub(crate) revealed: usize,
    pub(crate) floor: usize,
    pub(crate) trace: String,
    pub(crate) overlay: String,
}
impl PlayResult {
    pub(crate) fn json(&self) -> String {
        format!(
            "{{\"profile\":\"{}\",\"completed\":{},\"steps\":{},\"revisits\":{},\"discovered_exit\":{},\"revealed\":{},\"floor\":{}}}",
            self.profile.name(),
            self.completed,
            self.steps,
            self.revisits,
            self.discovered_exit
                .map_or("null".into(), |n| n.to_string()),
            self.revealed,
            self.floor
        )
    }
}

pub(crate) fn autoplay(map: &Grid, endpoints: (usize, usize), profile: Profile) -> PlayResult {
    let mut session = Session::new(
        map.clone(),
        endpoints.0,
        endpoints.1,
        profile,
        map.cells.len() * 6,
    )
    .expect("accepted map has valid endpoints and bounded size");
    while !session.finished() {
        let Some(action) = choose(session.observe(), profile) else {
            break;
        };
        session.step(action).expect("session is active");
    }
    session.result()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn corridor() -> Grid {
        let mut map = Grid::solid(12, 3);
        for x in 1..11 {
            map.cells[12 + x] = true;
        }
        map
    }
    #[test]
    fn fog_hides_the_exit_and_discovery_occurs_at_the_predicted_step() {
        let mut game = Session::new(corridor(), 13, 22, Profile::Seeker, 30).unwrap();
        assert_eq!(game.observe().exit, None);
        assert_eq!(game.observe().known[22], None);
        for _ in 0..5 {
            game.step(Action::East).unwrap();
        }
        assert_eq!(game.observe().exit, None);
        game.step(Action::East).unwrap();
        assert_eq!(game.observe().exit, Some(22));
        assert_eq!(game.result().discovered_exit, Some(6));
        for _ in 0..3 {
            game.step(Action::East).unwrap();
        }
        assert!(game.completed());
        let before = game.observe().clone();
        assert!(game.step(Action::West).is_err());
        assert_eq!(*game.observe(), before);
    }
    #[test]
    fn changing_hidden_terrain_does_not_change_observation_or_first_action() {
        let a = corridor();
        let mut b = a.clone();
        b.cells[21] = false;
        let a = Session::new(a, 13, 22, Profile::Seeker, 30).unwrap();
        let b = Session::new(b, 13, 22, Profile::Seeker, 30).unwrap();
        assert_eq!(a.observe(), b.observe());
        assert_eq!(choose(a.observe(), Profile::Seeker), Some(Action::East));
        assert_eq!(
            choose(a.observe(), Profile::Seeker),
            choose(b.observe(), Profile::Seeker)
        );
    }
    #[test]
    fn blocked_actions_consume_budget_and_timeout_is_not_completion() {
        let mut game = Session::new(corridor(), 13, 22, Profile::Seeker, 1).unwrap();
        game.step(Action::North).unwrap();
        assert!(game.finished());
        assert!(!game.completed());
        assert_eq!(game.result().steps, 1);
        assert!(game.result().trace.ends_with("0\ttrue\ttrue\n"));
    }
    #[test]
    fn explorer_reveals_branch_before_finishing_while_seeker_takes_visible_exit() {
        let map = corridor();
        let seeker = autoplay(&map, (13, 15), Profile::Seeker);
        let explorer = autoplay(&map, (13, 15), Profile::Explorer);
        assert_eq!(seeker.steps, 2);
        assert!(explorer.completed);
        assert!(explorer.steps > seeker.steps);
        assert_eq!(explorer.revealed, 10);
    }
}
