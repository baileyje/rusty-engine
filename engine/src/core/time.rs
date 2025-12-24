use std::time::{Duration, Instant};

pub const SIXTY_FPS: u64 = 16_666_666;
pub const ONE_FPS: u64 = 1_000_000_000;

/// A TimeFrame represents a specific amount of time elapsed within the engine for a single
/// simulation frame. Each frame captures total elapsed time as well as the delta time since
/// the last frame. New frames are intended to be generated from a previous frame using the
/// `next()` method. Generally this can be invoked on each iteration of a game loop.
#[derive(Debug, Copy, Clone)]
pub struct Time {
    // The current instant when this frame was created
    instant: Instant,
    pub fixed_time_step: u64,
    /// The time delta since the last frame
    pub delta: Duration,
    /// The total elapsed time since the first frame
    pub time: Duration,
    /// The total elapsed time since the first frame but incremented by the fixed time step
    pub fixed_time: Duration,
    /// An accumulator for fixed time step calculations
    accumulator: u64,
}

impl Time {
    /// Construct a new `Frame` with delta and time set to `0`. Caller must provide a fixed time
    /// step in nano seconds.
    pub fn new(fixed_time_step: u64) -> Self {
        Self {
            fixed_time_step,
            instant: Instant::now(),
            delta: Duration::ZERO,
            time: Duration::ZERO,
            fixed_time: Duration::ZERO,
            accumulator: 0,
        }
    }

    /// Increment the fixed frame time accumulation
    pub fn increment_fixed(&mut self) {
        self.fixed_time += Duration::from_nanos(self.fixed_time_step);
        self.accumulator -= self.fixed_time_step;
    }

    /// Create the next frame from an existing frame. This will capture the delta from the last
    /// frame and update the cumulative time.
    pub fn next(self) -> Self {
        let delta = self.instant.elapsed();
        Self {
            fixed_time_step: self.fixed_time_step,
            instant: Instant::now(),
            delta,
            time: self.time + delta,
            fixed_time: self.fixed_time,
            accumulator: self.accumulator + delta.as_nanos() as u64,
        }
    }

    /// Determine whether this frame has accumulated enough delta for a fixed frame.
    pub fn has_fixed(&self) -> bool {
        self.accumulator >= self.fixed_time_step
    }

    /// Reset the time frame to now with zeroed accumulator. This is useful for situations where
    /// the engine is paused and resumed.
    pub fn reset_now(&mut self) {
        self.instant = Instant::now();
        self.accumulator = 0;
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new(SIXTY_FPS)
    }
}
