use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use glam::Vec2;

use arpg_core::{Action, InstanceBuffer, Intent, MoveDir, Report};
use arpg_gfx::{OrthoCamera, QuadBuffer, Renderer};
use arpg_sim::{Accumulator, World};

use crate::harness::{self, Command, Request};
use crate::hud;
use crate::input::Controls;
use crate::time::Clock;

/// Owns everything and wires it together. Deliberately the only place that
/// knows about all the subsystems at once — `gfx` and `world` stay ignorant of
/// each other, and this is where they meet.
#[derive(Default)]
pub(crate) struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Option<OrthoCamera>,
    world: World,
    /// Numbers the screenshots, so repeated captures do not overwrite.
    captures: u32,
    /// Frames skipped because the surface had none to give — occluded,
    /// resized behind our back, or lost. Reported alongside `frames` so a
    /// suspiciously fast run is self-diagnosing rather than mysterious.
    skipped: u64,
    /// Frames presented since launch.
    ///
    /// Reported by the harness so throughput can be *counted* over a known
    /// interval rather than inferred from `Clock`'s smoothed average — which is
    /// an EMA, and so cannot distinguish a steady 60Hz from a mixture that
    /// averages to it. It is also the only way to notice the app being throttled
    /// while it sits in the background.
    frames: u64,
    /// Present only when `ARPG_HARNESS` asked for a control socket.
    harness: Option<std::sync::mpsc::Receiver<Request>>,
    /// Keys to release, and replies to send, once their deadline passes.
    scheduled: Vec<Deferred>,
    /// Replies owed to callers waiting on a frame to be captured.
    awaiting_frame: Vec<std::sync::mpsc::Sender<String>>,
    /// Frames skipped since the oldest pending screenshot was asked for.
    ///
    /// A capture is recorded between drawing a frame and presenting it, so a
    /// frame that is never drawn never captures, and an occluded window skips
    /// every frame. Counting them is what lets the reply say "your window is
    /// covered" rather than a silent `ok` — see [`App::capture_has_stalled`].
    capture_stall: u32,
    /// Reused every frame so a steady state allocates nothing.
    instances: InstanceBuffer,
    /// The overlay's own staging buffer, on exactly the same terms — and
    /// separate from `instances` because the two are drawn by different
    /// pipelines in different spaces. Merging them would mean a sort.
    quads: QuadBuffer,
    input: Controls,
    clock: Clock,
    /// Turns the frame's elapsed seconds into whole simulation ticks.
    ///
    /// Owned by `app` because the frame loop is here, but defined in `sim`,
    /// which is the only thing that may mint a `Dt`. That split is the point:
    /// this crate measures wall clock and is structurally unable to hand any of
    /// it to the simulation.
    accumulator: Accumulator,
}

/// A key release, a harness reply, or both, owed once `due` passes.
///
/// Named rather than a tuple because two of its three fields are optional and
/// which combination is in play is the whole meaning: `tap` is a release with
/// no reply, `wait` is a reply with no release, `hold` is both.
struct Deferred {
    due: std::time::Instant,
    /// The key to lift, if this deadline releases one.
    release: Option<KeyCode>,
    /// The caller to answer, if one is blocked on this deadline.
    reply: Option<std::sync::mpsc::Sender<String>>,
}

/// One notch of zoom per keypress.
const ZOOM_STEP: f32 = 1.2;
const _: () = assert!(ZOOM_STEP > 1.0, "a step of 1 or less makes the zoom keys inert or inverted");

/// Where `P` writes screenshots. The system temp directory unless told
/// otherwise, so a stray keypress never litters the working tree.
fn capture_dir() -> std::path::PathBuf {
    std::env::var_os("ARPG_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

impl App {
    /// Debug and meta commands, kept deliberately apart from the action layer.
    ///
    /// These are not things the *character* does — they are things done to the
    /// running program, and the difference is not cosmetic. Game actions are
    /// sampled as state once per tick, need rebinding, and will one day come
    /// from a gamepad or a replay. These are one-shot, fire straight from the
    /// event callback, and are meaningless to a simulation. Funnelling them
    /// through `Action` would put "toggle vsync" in the vocabulary the horde's
    /// AI speaks.
    ///
    /// The two sets must stay disjoint; `BINDINGS` is the list to check against.
    fn on_debug_key(&mut self, key: KeyCode) {
        match key {
            // Doubling rather than stepping: the interesting range spans three
            // orders of magnitude, and the knee is easier to find by bisection
            // than by walking. Both directions clamp inside `set_enemy_count`.
            KeyCode::BracketRight => {
                // `max(1)` because doubling zero is zero: the horde can now
                // be emptied, and without this `]` could not refill it.
                self.world.set_enemy_count((self.world.enemy_count() * 2).max(1));
                log::info!("N = {}", self.world.enemy_count());
            }
            KeyCode::BracketLeft => {
                self.world.set_enemy_count(self.world.enemy_count() / 2);
                log::info!("N = {}", self.world.enemy_count());
            }
            KeyCode::Equal => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(1.0 / ZOOM_STEP);
                }
            }
            KeyCode::Minus => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(ZOOM_STEP);
                }
            }
            _ => {}
        }
    }

    /// Applies whatever the control socket has sent since the last frame.
    ///
    /// Runs before input is sampled, so an injected key takes effect on the
    /// very frame it arrives rather than the one after.
    fn drain_harness(&mut self) -> bool {
        // Collected up front so the receiver borrow ends before the loop needs
        // the rest of `self`.
        let Some(rx) = &self.harness else { return false };
        let requests: Vec<Request> = rx.try_iter().collect();
        let mut quit = false;

        for Request { command, reply } in requests {
            let now = std::time::Instant::now();
            let answer = match command {
                Command::Press(key) => {
                    self.input.on_key(key, true, false, self.world.attack_status().recovery);
                    "ok".to_string()
                }
                Command::Release(key) => {
                    self.input.on_key(key, false, false, self.world.attack_status().recovery);
                    "ok".to_string()
                }
                Command::Tap(key) => {
                    self.input.on_key(key, true, false, self.world.attack_status().recovery);
                    self.scheduled.push(Deferred { due: now, release: Some(key), reply: None });
                    "ok".to_string()
                }
                Command::Hold(key, ms) => {
                    self.input.on_key(key, true, false, self.world.attack_status().recovery);
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push(Deferred { due, release: Some(key), reply: Some(reply) });
                    continue; // replies once the key comes back up
                }
                Command::Wait(ms) => {
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push(Deferred { due, release: None, reply: Some(reply) });
                    continue;
                }
                Command::Shot(path) => {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.request_capture(path);
                    }
                    self.awaiting_frame.push(reply);
                    continue; // replies once the file exists
                }
                Command::State => self.report_state(),
                Command::Impulse { target, value } => match self.world.body_named(&target) {
                    Some(id) if self.world.apply_impulse(id, value) => {
                        format!("impulse applied to {id}")
                    }
                    _ => format!("error: no physical body {target}"),
                },
                Command::TraceSince(tick) => self.report_trace(tick),
                Command::SetEnemies(n) => {
                    self.world.set_enemy_count(n);
                    // Respawning retires every name, so it revokes every
                    // behaviour with them. Said in the reply rather than left
                    // to be discovered, because "I set the horde and the
                    // chasing stopped" is otherwise a puzzle.
                    format!(
                        "enemies {} seekers {}",
                        self.world.enemy_count(),
                        self.world.seeker_count()
                    )
                }
                Command::Spawn { x, z, what } => {
                    // The reply says *queued*, not spawned, because that is what
                    // happened: the body appears when the next tick runs. A
                    // reply claiming otherwise would make a `state` taken
                    // immediately afterwards look like a bug.
                    if self.world.request_spawn(Vec2::new(x, z), what) {
                        format!("queued at ({x}, {z}) seeks={}", what.seeks())
                    } else {
                        "error: spawn queue full".to_string()
                    }
                }
                Command::Source(spec) => {
                    // The conversion is where `Placement::around` and the
                    // cadence clamp are applied, so the socket cannot reach a
                    // source that skipped either.
                    let id = self.world.add_source(spec.into());
                    format!("source {id}")
                }
                Command::RemoveSource(id) => match self.world.remove_source(id) {
                    true => format!("removed {id}"),
                    false => format!("error: no live source {id}"),
                },
                Command::SetSeekers(n) => {
                    self.world.set_seeker_count(n);
                    format!("seekers {}", self.world.seeker_count())
                }
                Command::SetVsync(on) => match self.renderer.as_mut() {
                    Some(renderer) => {
                        if renderer.vsync() != on {
                            renderer.toggle_vsync();
                        }
                        format!("vsync {}", renderer.vsync())
                    }
                    None => "error: no renderer yet".to_string(),
                },
                Command::Quit => {
                    quit = true;
                    "ok".to_string()
                }
            };
            let _ = reply.send(answer);
        }
        quit
    }

    /// Everything worth knowing about the running program, as JSON.
    ///
    /// **Derived, not hand-written.** The simulation's half comes from
    /// `World::report`, which destructures `World` exhaustively — so a field
    /// added to the world fails to compile until it is observable. What is left
    /// here is the part `sim` genuinely cannot know: how the frame went, what
    /// the camera is doing, whether the renderer is presenting.
    ///
    /// JSON rather than a positional line, because the consumer is usually a
    /// program: `jq -r .sim.tick` does not care what order the fields are in or
    /// how many were added since it was written.
    fn report_state(&self) -> String {
        let mut out = Report::default();

        out.object("sim", |sim| self.world.report(sim));
        out.object("ui", |ui| self.input.menu().report(ui));

        out.object("render", |r| {
            let target = self.camera.as_ref().map(OrthoCamera::target).unwrap_or_default();
            r.vec3("camera_target", target);
            r.int("instances", self.instances.as_slice().len() as u64);
            r.int("frames", self.frames);
            r.int("skipped", self.skipped);
            r.num("frame_ms", self.clock.frame_ms());
            r.bool("vsync", self.renderer.as_ref().is_some_and(Renderer::vsync));
        });

        out.finish()
    }

    /// Every trace event from `tick` onward, one per line.
    ///
    /// Same rendering as a scenario's golden trace file, because it is the same
    /// function — a second formatter would be a second thing to keep in step,
    /// and the whole point is that what you read here is what a scenario can
    /// assert on.
    fn report_trace(&self, tick: u64) -> String {
        let trace = self.world.trace();
        let mut out = String::new();

        // Said out loud rather than left to be inferred from a suspiciously
        // short reply: the buffer is bounded, and a caller asking for a tick
        // that has already scrolled away should be told so.
        if trace.dropped() > 0 {
            out.push_str(&format!("# {} event(s) dropped; the buffer wrapped\n", trace.dropped()));
        }
        for (t, event) in trace.since(tick) {
            out.push_str(&format!("{t} {event}\n"));
        }
        out.push_str(&format!("# {} event(s)", trace.since(tick).count()));
        out
    }

    /// Counts one skipped frame against any pending screenshot, and reports
    /// whether the wait should be abandoned.
    ///
    /// Waiting forever would be honest and useless: the caller blocks on a
    /// socket read with no idea why. Giving up after a bounded run of skipped
    /// frames keeps the harness's promise — every command replies, and the
    /// reply is true — while making the one condition that breaks screenshots
    /// name itself.
    ///
    /// Split from the abandoning itself so the rule is a plain function over a
    /// counter. The condition it fires on is one this environment cannot
    /// produce on demand, and an untested error path is one that has never run.
    fn capture_has_stalled(waiting: bool, stall: &mut u32) -> bool {
        /// Frames of nothing before a screenshot is declared impossible. Only
        /// has to exceed what a healthy run produces, and a healthy run
        /// captures on the very next frame.
        const STALLED_FRAMES: u32 = 240;

        if !waiting {
            *stall = 0;
            return false;
        }

        *stall += 1;
        if *stall < STALLED_FRAMES {
            return false;
        }

        *stall = 0;
        true
    }

    /// Fires the releases and replies whose deadline has passed.
    fn service_schedule(&mut self) {
        let now = std::time::Instant::now();
        let mut still_pending = Vec::new();

        for deferred in std::mem::take(&mut self.scheduled) {
            if now < deferred.due {
                still_pending.push(deferred);
                continue;
            }
            if let Some(key) = deferred.release {
                self.input.on_key(key, false, false, self.world.attack_status().recovery);
            }
            if let Some(reply) = deferred.reply {
                let _ = reply.send("ok".to_string());
            }
        }
        self.scheduled = still_pending;
    }

    /// One frame: measure it, run whatever ticks it bought, then draw.
    ///
    /// Everything below the `alpha` line is presentation. Nothing there may
    /// write simulation state, and nothing there may consume an input edge.
    fn redraw(&mut self) {
        let (Some(renderer), Some(camera)) = (self.renderer.as_mut(), self.camera.as_mut()) else {
            return;
        };

        let frame = self.clock.tick();

        // Screen space becomes world space here, and only here. The camera owns
        // the mapping because it owns the angle; `sim` is handed a direction it
        // can integrate without knowing a screen exists.
        let (right, up) = camera.ground_basis();
        let to_world = |axis: Vec2| MoveDir::new(right * axis.x + up * axis.y);

        // Zero, one or several — a frame buys whole ticks and the remainder
        // waits.
        //
        // **Intent is sampled per tick, not per frame.** `sample` clears the
        // latched edges, so sampling once per frame means a frame that runs no
        // ticks consumes a keypress and discards it — and uncapped, most frames
        // run no ticks. Sampling here makes a press wait for a tick and gives it
        // to exactly one. The symptom otherwise is "the attack sometimes does
        // not come out", which points nowhere near the frame loop.
        for dt in self.accumulator.pending(frame) {
            // Requests wait through zero-tick frames and apply before this
            // tick's attack input. Drawing below cannot consume or apply one.
            if let Some(recovery) = self.input.take_recovery() {
                self.world.set_attack_recovery(recovery);
            }
            let intent = self.input.sample();
            // `just_pressed`, not `held`: a swing is an edge. Holding the key
            // must not swing every tick, and a tap shorter than a frame must
            // still swing exactly once.
            self.world.step(
                dt,
                Intent::new(to_world(intent.move_axis()), intent.just_pressed(Action::Attack)),
            );
        }

        // How far this frame falls between the tick just run and the next one.
        // Everything below draws; nothing below simulates.
        let alpha = self.accumulator.alpha();

        // Presentation, in three ways at once. **After** the step, or it would
        // add a frame of lag on top of the smoothing that is there on purpose.
        // The **drawn** position and the **frame's** delta, not the tick's,
        // because a rig whose whole job is smoothness must not be given a
        // stair-step to follow. And `held`, never `sample` — a camera that
        // consumed a keypress would be the same bug wearing a different hat.
        let facing = to_world(self.input.held().move_axis());
        camera.follow(self.world.player_pos_at(alpha), facing, frame);

        self.world.extract(alpha, self.instances.sink());

        // The overlay is built here rather than inside `render` for the same
        // reason `extract` is: what a readout says is a decision this crate
        // makes, and the renderer's job stops at drawing the rectangles it is
        // handed. Scoped so the sink's borrow ends before the draw.
        {
            let mut sink = self.quads.sink();
            hud::draw(renderer.glyphs(), self.input.menu(), self.world.attack_status(), &mut sink);
        }

        // Counted only when a frame actually reached the screen. An occluded
        // window skips the draw entirely, and counting those would report
        // thousands of frames a second for drawing nothing.
        if renderer.render(camera, self.instances.as_slice(), self.quads.as_slice()) {
            self.frames += 1;

            // The capture is written inside `render`, and only on the path that
            // presents — so this is the first moment the file is known to exist,
            // and the only place `ok` is honest.
            self.capture_stall = 0;
            for reply in std::mem::take(&mut self.awaiting_frame) {
                let _ = reply.send("ok".to_string());
            }
        } else {
            self.skipped += 1;

            let waiting = !self.awaiting_frame.is_empty();
            if Self::capture_has_stalled(waiting, &mut self.capture_stall) {
                // Drop the request too, so it cannot fire minutes later and
                // write a file after the caller was told it failed.
                renderer.cancel_capture();
                for reply in std::mem::take(&mut self.awaiting_frame) {
                    let _ = reply.send(
                        "error: no frame was presented, so nothing could be \
                         captured — the window is occluded or minimised"
                            .to_string(),
                    );
                }
            }
        }

        if self.clock.hud_due()
            && let Some(window) = &self.window
        {
            window.set_title(&format!(
                "arpg — {:.2}ms  {:.0}fps  N={}  ({} instances){}",
                self.clock.frame_ms(),
                self.clock.fps(),
                self.world.enemy_count(),
                self.instances.as_slice().len(),
                if renderer.vsync() { "  [vsync]" } else { "  [uncapped]" },
            ));
        }
    }

    pub(crate) fn run() {
        let event_loop = EventLoop::new().expect("create event loop");

        // Poll rather than Wait: never block waiting for input, just keep
        // looping. Vsync in the surface config is what actually paces us.
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.run_app(&mut App::default()).expect("run app");
    }
}

impl ApplicationHandler for App {
    /// Window and GPU creation belongs here rather than in `main`, because on
    /// mobile platforms the surface is destroyed and rebuilt as the app moves
    /// in and out of the foreground. winit models that as suspend/resume, and
    /// guarantees `resumed` fires before any window event on every platform.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return; // Can fire more than once; only build GPU state the first time.
        }

        let attrs = Window::default_attributes()
            .with_title("arpg")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let display_handle = event_loop.owned_display_handle();

        // Native-only, so we can simply block until the GPU is ready. The
        // cross-platform examples route this back through the event loop
        // because the browser forbids blocking the main thread.
        self.renderer = Some(pollster::block_on(Renderer::new(window.clone(), display_handle)));
        let size = window.inner_size();

        // Start framed on the character rather than easing in from the origin.
        // Harmless today, since both begin there — but the moment anything
        // spawns the player elsewhere, the first thing the player would see is
        // the camera flying across the world to catch up.
        let mut camera = OrthoCamera::new(size.width, size.height);
        camera.snap_to(self.world.player_pos());

        self.camera = Some(camera);
        self.window = Some(window);
        self.harness = harness::start();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(renderer), Some(camera)) = (self.renderer.as_mut(), self.camera.as_mut()) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            // A key-up goes to whoever has focus. Alt-tab while running and the
            // release never arrives, so without this the character keeps going.
            WindowEvent::Focused(focused) => {
                log::debug!("focus: {focused}");
                if !focused {
                    self.input.release_all();
                }
            }

            WindowEvent::KeyboardInput { event: key_event, .. } => {
                log::debug!(
                    "key: physical={:?} state={:?} repeat={}",
                    key_event.physical_key,
                    key_event.state,
                    key_event.repeat
                );

                let PhysicalKey::Code(key) = key_event.physical_key else {
                    return;
                };
                let pressed = key_event.state == ElementState::Pressed;

                // The harness uses this same route. A modal UI event must not
                // also fire a debug command (especially Escape -> quit).
                let consumed = self.input.on_key(
                    key,
                    pressed,
                    key_event.repeat,
                    self.world.attack_status().recovery,
                );

                if !consumed && pressed && !key_event.repeat {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyV => {
                            renderer.toggle_vsync();
                        }
                        KeyCode::KeyP => {
                            let path = capture_dir().join(format!("arpg-{:04}.png", self.captures));
                            self.captures += 1;
                            renderer.request_capture(path);
                        }
                        _ => self.on_debug_key(key),
                    }
                }
            }

            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                camera.set_viewport(size.width, size.height);
            }

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    /// winit is event-driven by default: with nothing happening, it sleeps. A
    /// game is the opposite — it must produce a frame whether or not anyone
    /// touched the keyboard. Requesting a redraw every time the event queue
    /// drains is what turns this into a continuous loop.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The control socket is drained here rather than inside the redraw,
        // because this is the one point in the loop where nothing else is
        // borrowed out of `self` — and it runs immediately before the frame,
        // so an injected key takes effect on the very next one.
        //
        // Expiries are serviced *first*, so a key pressed by this pass survives
        // until the next one and is therefore held for exactly one frame.
        self.service_schedule();
        if self.drain_harness() {
            event_loop.exit();
            return;
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::App;

    /// A screenshot that cannot be taken has to *say so*. A reply sent whether
    /// or not the frame was drawn turns "your window is covered" into "your
    /// change did nothing" — the worst failure available to a harness whose
    /// whole contract is that a reply means the effect landed.
    #[test]
    fn a_screenshot_gives_up_only_after_a_long_run_of_skipped_frames() {
        let mut stall = 0;

        // A hiccup is not a stall; the wait has to survive one.
        assert!(!App::capture_has_stalled(true, &mut stall));
        assert!(!App::capture_has_stalled(true, &mut stall));

        let gave_up = (0..10_000).any(|_| App::capture_has_stalled(true, &mut stall));
        assert!(gave_up, "waited forever instead of reporting the failure");
    }

    /// With nothing pending there is nothing to give up on, and the count must
    /// not carry over — otherwise a long occluded stretch would fail the next
    /// screenshot the instant it was asked for.
    #[test]
    fn skipped_frames_with_no_screenshot_pending_do_not_accumulate() {
        let mut stall = 0;

        for _ in 0..10_000 {
            assert!(!App::capture_has_stalled(false, &mut stall));
        }
        assert_eq!(stall, 0);

        assert!(!App::capture_has_stalled(true, &mut stall), "a fresh request must not fail");
    }
}
