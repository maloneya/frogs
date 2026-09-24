# Crowd collision

Crowd separation uses a hand-written uniform 2D grid to reject distant pairs,
then applies the existing disc contact test and immediate mass-weighted
separation/momentum response. The player still has a separate linear pass.
Cell width derives from the shared non-player collision diameter in
[the grid owner](../crates/sim/src/grid.rs); it is not an authored tuning copy.

## Gaits and pushing

Seeking enemies choose Walk, Run or Sprint from stable identity and simulation
tick. Choices last three seconds with staggered boundaries; the initial partial
interval varies by identity. Restart repeats the choices, and removing another
body cannot reroll a survivor. [The movement owner](../crates/sim/src/pass/seek.rs)
defines speeds and selection. `Game::gait` and the `gaits` state report expose
individual choices; traces aggregate switches by gait each tick.

A gait requests velocity through the shared [physics motor](../crates/sim/src/pass/motion.rs).
The motor applies drag and bounded acceleration before integration. Contacts
then exchange that same velocity: a sprinter gives up momentum to a walker,
and a stationary bystander can be shoved without an attack. Incoming knockback
also survives and is opposed gradually. Passive bodies apply drag after contacts;
a private tick marker prevents driven bodies from damping twice. Player controls
remain immediate, with knockback added through physics.

Approaching the player reduces requested speed using the remaining stopping
distance. Isolated arrivals settle without repeatedly colliding. In a crowd,
other bodies can still push an arrived enemy. Walls and fixed props use the
existing shared contact response. Bodies remain on the floor; jumping and
climbing need a separate vertical-motion change.

Horde animation retains eight shared poses: two phases each for idle, walking,
running and sprinting cadence. Moving cadences reuse the authored Run clip,
selected from actual displacement and retimed against the sim-owned gait speeds.
This adds visible pace variation without adding skeleton evaluations per frame;
it does not provide distinct authored walk/sprint animations.

## Preserve the physical sweep

The previous solver visited every `(i, j)` in ascending dense-row order, with
`j > i`. Each correction landed immediately. That order determines how a shove
travels through the crowd, and remains unchanged for all potentially contacting
pairs. The optimization removes only tests that cannot succeed at the moment
the original sweep would visit them.

For each outer row, gather bodies from its own cell and the eight neighbours,
filter to higher rows and mark them in a bitset. Reading set bits in ascending
order visits each pair once without sorting candidate lists. After each pair callback,
the grid checks both positions and repairs any changed cell links. If the outer
body crosses a cell, re-query its new neighbourhood, retaining only rows after
the last visited row. Previously marked higher rows remain safe extra candidates
for the exact disc test; they do not need to be removed or re-sorted. A correction can create a new later contact; it must not
cause an already-passed pair to run twice. If the outer body stays in its cell,
the original neighbourhood still conservatively covers its possible contacts.

Each cell is scanned at most once for an outer row. During that sweep, only
the outer body and already-visited partners can move, so an already-scanned
cell cannot acquire a new unvisited partner. Remembering those scans avoids
repeated linked-list walks when contact corrections push the outer body back
and forth across a boundary. The scan markers reset before each pass.

Indexing and pair mutation share one private owner operation. There is no public
query that callers could accidentally use after moving a body without updating
its membership. The reference all-pairs traversal exists only in engine tests.
Those tests drive the ordinary World schedule and compare full hashes and
transitions after each tick, including contact-induced movement, attacks and
structural changes. The cell-crossing and momentum scenarios additionally run
through Game with predicted intermediate positions or velocities.

## Bounds and lifecycle

Storage is reserved at construction for the physical admission ceiling; solving
does not allocate. Each cell holds a chain of small contiguous index pages,
which reduces pointer chasing compared with one linked node per body. Removal
fills its hole from the head page and updates the moved row's location. Empty
pages return to a bounded free list. This costs more reserved scratch memory
than individual links but keeps both cell searches and membership updates cheap.
The fixed arena bounds the cell table. Since movement and
scene placement can put bodies outside the arena before containment, outlying
coordinates map conservatively into edge cells. They retain their physical
positions until the normal containment pass. This can create extra candidates,
but cannot hide a pair. Index arithmetic uses f64 to avoid rounding a near-boundary
f32 position incorrectly when adding the arena offset; physical arithmetic stays
unchanged.

The grid is scratch rebuilt from all current rows at the beginning of every
crowd pass, after spawn, steering and carried motion. No cached row or identity
is consulted across passes. Removal, defeat and scene eviction therefore need
no persistent grid-membership repair; the next pass rebuilds before any query.
Fresh World/Game construction gets fresh scratch. Scratch is explicitly omitted
from hashes and world reports because it is derived, unobservable work storage.
Diagnostic phase counts are exposed through StepObserver, never read by gameplay.

## Measuring and limits

Use F2's **Crowd stress — 10,000 seekers** scene, wait for the crowd to compress,
then follow [performance telemetry](performance.md). Compare crowd time per tick,
candidate/possible-pair counts and frame percentiles. Repeat with movement and
attacks; retain the actual population because defeats reduce workload.

A grid does not make genuine dense interactions disappear. Large coincident
piles, overcrowded edge cells and repeated cell crossings can still be costly.
There is no candidate cap that silently drops contacts and no change to the
catch-up policy, collision tuning, or physical response. Improvements to those
mechanisms need their own evidence and behavioural verification.
