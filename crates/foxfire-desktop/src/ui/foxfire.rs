use rouge::core::{
    Corners, Div, Fill, IntoProp, Keyframes, PaintRegion, Pose, Prop, Styled, Tween, View, div, each, switch,
    window_focused,
};
use rouge::reactive::{Signal, expect_context, memo, provide_context, signal, stored, use_context};
use std::f32::consts::TAU;

/// Activity, not progress: each state has a distinct choreography and direction of motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoxfireState {
    Connecting,
    Thinking,
    Compacting,
    Working,
    Writing,
    Waiting,
}

/// A shared, reactive preference for activity indicators. Reduced motion removes their clocks.
#[derive(Clone, Copy)]
pub struct FoxfireMotion(Signal<bool>);

/// A quiet activity mark: a 3×3 matrix of cells, lit in a different pattern for each state.
/// Cells move on compositor keyframes, so a running mark never relays out or repaints; each new state fades in.
/// Waiting is always still; reduced motion, loss of focus, and clipping also stop the clock.
/// Pair this decoration with a visible activity label rather than relying on its color or motion.
pub fn foxfire<T>(size: f32, tint: impl IntoProp<Fill, T>, state: impl Fn() -> FoxfireState + 'static) -> Div {
    let tint = stored(tint.into_prop());
    let fill = move || tint.with(Prop::get);
    let state = memo(state);
    let region = signal(PaintRegion::default());
    let focused = window_focused();
    let motion = use_context::<FoxfireMotion>();
    let animated = memo(move || {
        state.get() != FoxfireState::Waiting
            && focused.get()
            && !motion.is_some_and(FoxfireMotion::reduced)
            && region.with(|region| !region.bounds.is_empty())
    });
    div().flex_none().relative().size(size).pass_through().track_paint_region(region).child(switch(
        move || animated.get(),
        move |animated| {
            if animated {
                each(
                    move || vec![state.get()],
                    |state| *state,
                    move |state| {
                        let state = state.get_untracked();
                        matrix(size, fill, |cell, column, row| moving(cell, state, size, column, row))
                    },
                )
                .enter(Pose::new().opacity(0.0), Tween::ms(FADE_IN_MS))
                .into_any()
            } else {
                switch(
                    move || state.get(),
                    move |state| matrix(size, fill, |cell, column, row| still(cell, state, size, column, row)),
                )
                .into_any()
            }
        },
    ))
}

impl FoxfireState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Thinking => "Thinking",
            Self::Compacting => "Compacting the conversation",
            Self::Working => "Working",
            Self::Writing => "Writing",
            Self::Waiting => "Waiting for you",
        }
    }

    fn choreography(self) -> (f32, u8) {
        match self {
            Self::Connecting => (34.0 / 30.0, 34),
            Self::Thinking => (2.0, 60),
            Self::Working => (1.0, 30),
            Self::Writing => (f32::from(WRITING_STEPS) / 9.0, WRITING_STEPS),
            Self::Compacting => (1.5, 45),
            Self::Waiting => (1.0, 1),
        }
    }

    /// The moment of the choreography that best stands in for it when the clock is stopped.
    fn still_phase(self) -> f32 {
        match self {
            Self::Connecting => 1.0 / 8.0,
            Self::Thinking => 0.884,
            Self::Working | Self::Waiting => 0.0,
            Self::Writing => 5.5 / f32::from(WRITING_STEPS),
            Self::Compacting => 0.4,
        }
    }
}

impl FoxfireMotion {
    #[must_use]
    pub fn install(reduced: bool) -> Self {
        let motion = Self(signal(reduced));
        provide_context(motion);
        motion
    }

    #[must_use]
    pub fn current() -> Self {
        expect_context()
    }

    pub fn set_reduced(self, reduced: bool) {
        self.0.set_if_changed(reduced);
    }

    #[must_use]
    pub fn reduced(self) -> bool {
        self.0.get()
    }
}

/// How lit a cell is (0–1) and how far it has been pulled toward the center (0–1 of a cell's pitch).
struct Frame {
    lit: f32,
    pull: f32,
}

impl Frame {
    fn opacity(&self) -> f32 {
        UNLIT + (1.0 - UNLIT) * self.lit
    }

    fn pose(&self, size: f32, column: u8, row: u8) -> Pose {
        let (x, y) = toward_center(size, column, row, self.pull);
        Pose::new().opacity(self.opacity()).translate_x(x).translate_y(y)
    }
}

/// The grid is four cells wide with half-cell gaps, so 12 and 16 px marks land on whole pixels at 2×.
fn matrix(size: f32, fill: impl Fn() -> Fill + Copy + 'static, cell: impl Fn(Div, u8, u8) -> Div) -> Div {
    let edge = size / 4.0;
    div().absolute().inset(0.0).pass_through().children((0..9u8).map(|index| {
        let (column, row) = (index % 3, index / 3);
        let at = div()
            .absolute()
            .left(f32::from(column) * edge * PITCH)
            .top(f32::from(row) * edge * PITCH)
            .size(edge)
            .rounded(Corners::all(edge * ROUNDNESS))
            .bg(fill);
        cell(at, column, row)
    }))
}

fn still(cell: Div, state: FoxfireState, size: f32, column: u8, row: u8) -> Div {
    let frame = frame(state, column, row, state.still_phase());
    let (x, y) = toward_center(size, column, row, frame.pull);
    cell.opacity(frame.opacity()).translate_x(x).translate_y(y)
}

fn moving(cell: Div, state: FoxfireState, size: f32, column: u8, row: u8) -> Div {
    let (seconds, steps) = state.choreography();
    let pose = |phase| frame(state, column, row, phase).pose(size, column, row);
    let keyframes = (0..steps).fold(Keyframes::new(), |keyframes, step| {
        let (start, end) = (f32::from(step) / f32::from(steps), f32::from(step + 1) / f32::from(steps));
        keyframes.at(start, |_| pose(start)).at(end, |_| pose(start))
    });
    cell.animate(keyframes.duration(seconds).repeat())
}

fn frame(state: FoxfireState, column: u8, row: u8, phase: f32) -> Frame {
    let ring = (0..8u8).find(|&index| RING[usize::from(index)] == (column, row));
    let distance = (f32::from(column) - 1.0).hypot(f32::from(row) - 1.0);
    let lit = match state {
        FoxfireState::Connecting => ring.map_or(0.0, |index| chase(phase * 8.0, index, 2.2)),
        FoxfireState::Thinking => (0.5 + 0.5 * (TAU * phase + distance * 2.3).sin()).powi(2),
        FoxfireState::Working => ring.map_or(0.45 + 0.4 * (TAU * phase).sin(), |index| chase(phase * 8.0, index, 4.0)),
        FoxfireState::Writing => typed(step(phase), row * 3 + column),
        FoxfireState::Compacting if ring.is_some() => 1.0 - smoothstep(0.1, 0.6, phase),
        FoxfireState::Compacting => 0.5 + 0.5 * smoothstep(0.3, 0.7, phase),
        FoxfireState::Waiting => {
            if ring.is_some() {
                0.0
            } else {
                1.0
            }
        }
    };
    let pull = match state {
        FoxfireState::Compacting => 0.36 * smoothstep(0.1, 0.55, phase) * (1.0 - smoothstep(0.75, 1.0, phase)),
        _ => 0.0,
    };
    Frame { lit, pull }
}

/// A head running clockwise around the ring with a tail `length` cells long.
fn chase(head: f32, index: u8, length: f32) -> f32 {
    let behind = (head - f32::from(index)).rem_euclid(8.0);
    if behind < length { (1.0 - behind / length).powf(1.3) } else { 0.0 }
}

/// Fills the cells in reading order, then blinks a cursor in the first cell before starting over.
fn typed(step: u8, cell: u8) -> f32 {
    match step {
        TYPED.. if cell == 0 && step % 2 == 1 => 0.7,
        TYPED.. => 0.0,
        _ if cell < step => 1.0,
        _ if cell == step => 0.45,
        _ => 0.0,
    }
}

/// The typed step a phase falls in; the slack keeps sampled step boundaries from rounding down a step.
fn step(phase: f32) -> u8 {
    (0..WRITING_STEPS).rev().find(|&step| f32::from(step) <= phase * f32::from(WRITING_STEPS) + 1e-3).unwrap_or(0)
}

fn toward_center(size: f32, column: u8, row: u8, pull: f32) -> (f32, f32) {
    let pitch = size / 4.0 * PITCH;
    ((1.0 - f32::from(column)) * pitch * pull, (1.0 - f32::from(row)) * pitch * pull)
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The outer cells, clockwise from the top left.
const RING: [(u8, u8); 8] = [(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (0, 1)];
const PITCH: f32 = 1.5;
const ROUNDNESS: f32 = 0.3;
/// Unlit cells stay faintly visible so the grid reads as one mark rather than scattered specks.
const UNLIT: f32 = 0.1;
const FADE_IN_MS: f32 = 160.0;
const WRITING_STEPS: u8 = 14;
const TYPED: u8 = 10;
