use std::time::{Duration, Instant};

/// A specific frame in the simulation loop. Each frame will contain the timing information (total and delta) as well as the current
/// simulation data state.
/// 
/// 
pub struct Frame<'a, Data> {
  /// The current frame time info
  pub time: TimeFrame,
  /// The current frame's data
  pub data: &'a mut Data,
} 

impl<'a, Data> Frame<'a, Data> {
  /// Construct a new instance with a timeframe and data reference.
  pub fn new(time: TimeFrame, data: &'a mut Data) -> Self {
    return Self { time, data };
  }
}

/// A TimeFrame represents a specific amount of time elapsed within the engines for a single simulation frame. Each frame captures total
/// elapsed time as well as the delta time since the last frame. New frames are intended to be generated from a pervious frame using the
/// `next()` method. Generally this can be invoked on each iteration of a game loop.
#[derive(Debug, Copy, Clone)]
pub struct TimeFrame {
  instant: Instant,
  pub fixed_time_step: u64,
  /// The time delta since the last frame
  pub delta: Duration,
  /// The total elapsed time since the first frame
  pub time: Duration,
  /// The total elapsed time since the first frame but incremented by the fixed time step
  pub fixed_time: Duration,
  
  pub accumulator: u64
}

impl TimeFrame {
  /// Construct a new `Frame` with delta and time set to `0`. Caller must provide a fixed time step in nano seconds.
  pub fn new(fixed_time_step: u64) -> Self {
    return Self {
      fixed_time_step,
      instant: Instant::now(),
      delta: Duration::ZERO,
      time: Duration::ZERO,
      fixed_time: Duration::ZERO,
      accumulator: 0
    };
  }

  /// Increment the fixed frame time accumulation
  pub fn increment_fixed(&mut self) {
    self.fixed_time += Duration::from_nanos(self.fixed_time_step);
    self.accumulator -= self.fixed_time_step;
  }

  /// Create the next frame from an existing frame. This will capture the delta from the last frame and update the cumulative time.
  pub fn next(self) -> Self {
    let delta = self.instant.elapsed();
    Self {
      fixed_time_step: self.fixed_time_step,
      instant: Instant::now(),
      delta,
      time: self.time + delta,
      fixed_time: self.fixed_time,
      accumulator: self.accumulator + delta.as_nanos() as u64
    }
  }

  /// Determine whether this frame has accumulated enough delta for a fixed frame.
  pub fn has_fixed(&self) -> bool {
    self.accumulator >= self.fixed_time_step
  }
}
