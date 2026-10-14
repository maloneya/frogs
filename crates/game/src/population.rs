//! Authored actor births and their gameplay payloads. Simulation owns placement,
//! cadence and tickets; gameplay binds definitions from authoritative receipts.
use crate::behavior::{ActorDefinition, Actors};
use arpg_core::Report;
use arpg_sim::{Condition, EntityId, Fnv, SceneId, SourceId, SpawnOutcome, SpawnTicket};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

/// One complete actor placement.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    /// Ground-plane position in metres.
    pub pos: (f32, f32),
    /// Shared physical capabilities and gameplay definition.
    #[serde(default)]
    pub what: ActorDefinition,
}

/// A compact rectangular formation; each actor starts independent execution.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BodyGrid {
    /// First actor's position.
    pub origin: (f32, f32),
    /// Number of columns along X.
    pub columns: usize,
    /// Number of rows along Z.
    pub rows: usize,
    /// Spacing in metres; at least a body diameter when the grid holds more than one body.
    pub spacing: f32,
    /// Definition shared by every actor in this formation.
    #[serde(default)]
    pub what: ActorDefinition,
}

/// Complete game content, translated to physical content only at admission.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// Human-readable scene label.
    pub name: String,
    /// Explicit placements in identity order.
    #[serde(default)]
    pub bodies: Vec<Placed>,
    /// Formations expanded after explicit placements.
    #[serde(default)]
    pub grids: Vec<BodyGrid>,
    /// Independent timed spawn producers.
    #[serde(default)]
    pub sources: Vec<SourceSpec>,
    /// Shared physical wall geometry.
    #[serde(default)]
    pub walls: Vec<arpg_sim::Wall>,
}

impl Scene {
    pub(crate) fn physical(&self) -> Result<arpg_sim::Scene, &'static str> {
        for definition in self
            .bodies
            .iter()
            .map(|p| &p.what)
            .chain(self.grids.iter().map(|g| &g.what))
            .chain(self.sources.iter().map(|s| &s.what))
        {
            definition.validate()?;
        }
        Ok(arpg_sim::Scene {
            name: self.name.clone(),
            walls: self.walls.clone(),
            joints: Vec::new(),
            bodies: self
                .bodies
                .iter()
                .map(|p| arpg_sim::Placed {
                    pos: p.pos,
                    what: p.what.physical,
                })
                .collect(),
            grids: self
                .grids
                .iter()
                .map(|g| arpg_sim::BodyGrid {
                    origin: g.origin,
                    columns: g.columns,
                    rows: g.rows,
                    spacing: g.spacing,
                    what: g.what.physical,
                })
                .collect(),
            sources: self.sources.iter().map(SourceSpec::physical).collect(),
        })
    }
}

/// A source carries the same actor definition as every other birth door.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// Initially enabled; disabling preserves cadence.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Point or ring centre in metres.
    pub pos: (f32, f32),
    /// Ring radius; zero emits at the centre.
    #[serde(default)]
    pub radius: f32,
    /// Ticks between emissions; zero normalizes to one.
    #[serde(default = "one")]
    pub every: u32,
    /// Source population counts hostile actors, independently of their health.
    #[serde(default)]
    pub when: Condition,
    /// Definition for every emitted actor.
    #[serde(default)]
    pub what: ActorDefinition,
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
impl SourceSpec {
    fn physical(&self) -> arpg_sim::SourceSpec {
        arpg_sim::SourceSpec {
            enabled: self.enabled,
            pos: self.pos,
            radius: self.radius,
            every: self.every,
            when: self.when,
            what: self.what.physical,
        }
    }
}

/// Validated source admission, with no externally mutable runtime state.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "SourceSpec")]
pub struct Source {
    pub(crate) physical: arpg_sim::Source,
    pub(crate) definition: Arc<ActorDefinition>,
}
impl TryFrom<SourceSpec> for Source {
    type Error = &'static str;
    fn try_from(spec: SourceSpec) -> Result<Self, Self::Error> {
        spec.what.validate()?;
        let physical = arpg_sim::Source::try_from(spec.physical())?;
        Ok(Self {
            physical,
            definition: Arc::new(spec.what),
        })
    }
}

struct Producer {
    id: SourceId,
    owner: Option<SceneId>,
    definition: Arc<ActorDefinition>,
}
struct Pending {
    ticket: SpawnTicket,
    definition: Arc<ActorDefinition>,
}
/// Definitions survive accepted work, including producer removal before drain.
#[derive(Default)]
pub(crate) struct Births {
    sources: Vec<Producer>,
    pending: Vec<Pending>,
    // A derived weak cache shares equal definitions across all birth doors.
    // It owns no actor lifetime and cannot keep evicted content alive.
    definitions: BTreeMap<u64, Vec<Weak<ActorDefinition>>>,
}
impl Births {
    pub(crate) fn definition(&mut self, definition: ActorDefinition) -> Arc<ActorDefinition> {
        self.share(Arc::new(definition))
    }
    fn share(&mut self, definition: Arc<ActorDefinition>) -> Arc<ActorDefinition> {
        let mut hash = Fnv::default();
        definition.hash(&mut hash);
        let bucket = self.definitions.entry(hash.finish()).or_default();
        bucket.retain(|entry| entry.strong_count() != 0);
        for entry in bucket.iter().filter_map(Weak::upgrade) {
            if *entry == *definition {
                return entry;
            }
        }
        bucket.push(Arc::downgrade(&definition));
        definition
    }
    pub(crate) fn prune(&mut self) {
        self.definitions.retain(|_, bucket| {
            bucket.retain(|entry| entry.strong_count() != 0);
            !bucket.is_empty()
        });
    }
    pub(crate) fn source(
        &mut self,
        id: SourceId,
        owner: Option<SceneId>,
        definition: Arc<ActorDefinition>,
    ) {
        let definition = self.share(definition);
        self.sources.push(Producer {
            id,
            owner,
            definition,
        });
    }
    pub(crate) fn request(&mut self, ticket: SpawnTicket, definition: Arc<ActorDefinition>) {
        let definition = self.share(definition);
        self.pending.push(Pending { ticket, definition });
    }
    pub(crate) fn admit(&mut self, outcomes: &[SpawnOutcome], actors: &mut Actors) {
        for outcome in outcomes {
            let definition = if let Some(source) = outcome.source {
                self.sources
                    .iter()
                    .find(|p| p.id == source)
                    .map(|p| Arc::clone(&p.definition))
            } else {
                self.pending
                    .iter()
                    .position(|p| p.ticket == outcome.ticket)
                    .map(|row| self.pending.remove(row).definition)
            };
            if let Some(body) = outcome.entity {
                actors.install(
                    body,
                    outcome.owner,
                    definition.expect("accepted spawn retains its actor definition"),
                );
            }
        }
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.sources.retain(|p| p.owner != Some(owner));
        self.prune();
    }
    pub(crate) fn remove_source(&mut self, source: SourceId) {
        self.sources.retain(|p| p.id != source);
        self.prune();
    }
    pub(crate) fn install(
        &mut self,
        scene: &Scene,
        owner: SceneId,
        (bodies, formations): (&[EntityId], &[EntityId]),
        sources: &[SourceId],
        actors: &mut Actors,
    ) {
        let mut ids = bodies.iter().chain(formations).copied();
        for body in &scene.bodies {
            actors.install(
                ids.next().expect("admitted body"),
                Some(owner),
                self.definition(body.what.clone()),
            );
        }
        for grid in &scene.grids {
            let definition = self.definition(grid.what.clone());
            for _ in 0..grid.columns * grid.rows {
                actors.install(
                    ids.next().expect("admitted grid body"),
                    Some(owner),
                    Arc::clone(&definition),
                );
            }
        }
        for (id, source) in sources.iter().copied().zip(&scene.sources) {
            self.source(id, Some(owner), Arc::new(source.what.clone()));
        }
    }
    pub(crate) fn hash(&self, hash: &mut Fnv) {
        let Self {
            sources,
            pending,
            definitions: _,
        } = self;
        hash.usize(sources.len());
        for source in sources {
            let Producer {
                id,
                owner,
                definition,
            } = source;
            id.hash_into(hash);
            hash.usize(usize::from(owner.is_some()));
            if let Some(owner) = owner {
                owner.hash_into(hash);
            }
            definition.hash(hash);
        }
        hash.usize(pending.len());
        for pending in pending {
            pending.ticket.hash_into(hash);
            pending.definition.hash(hash);
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.object("actor_births", |out| {
            out.int("sources", self.sources.len() as u64);
            out.int("pending", self.pending.len() as u64);
            out.object("producers", |out| {
                for source in &self.sources {
                    out.object(&source.id.to_string(), |out| {
                        if let Some(owner) = source.owner {
                            out.text("scene", &owner.to_string());
                        }
                        source.definition.report(out);
                    });
                }
            });
            out.object("requests", |out| {
                for pending in &self.pending {
                    out.object(&pending.ticket.to_string(), |out| {
                        pending.definition.report(out)
                    });
                }
            });
        });
    }
}
