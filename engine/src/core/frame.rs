use std::time::{Duration, Instant};

/// A frame represents a specific timeframe elapsed within the engines. Each frame captures total elapsed time as well as the delta time
/// since the last frame. New frames are intended to be generated from a pervious frame using the `next()` method. Generally this can be
/// invoked on each iteration of a game loop.
#[derive(Debug)]
pub struct Frame {
  fixed_time_step: u64,
  instant: Instant,
  /// The time delta since the last frame
  pub delta: Duration,
  /// The total elapsed time since the first frame
  pub time: Duration,
  /// The total elapsed time since the first frame but incremented by the fixed time step
  pub fixed_time: Duration,
}

impl Frame {
  /// Construct a new `Frame` with delta and time set to `0`. Caller must provide a fixed time step in nano seconds.
  pub fn new(fixed_time_step: u64) -> Self {
    return Self {
      fixed_time_step,
      instant: Instant::now(),
      delta: Duration::ZERO,
      time: Duration::ZERO,
      fixed_time: Duration::ZERO,
    };
  }

  /// Increment the fixed frame time accumulation
  pub fn increment_fixed(&mut self) {
    self.fixed_time += Duration::from_nanos(self.fixed_time_step);
  }

  /// Create the next frame from an existing frame. This will capture the delta from the last frame and update the cumulative time.
  pub fn next(self) -> Frame {
    let elapsed = self.instant.elapsed();
    Frame {
      fixed_time_step: self.fixed_time_step,
      instant: Instant::now(),
      delta: elapsed,
      time: self.time + elapsed,
      fixed_time: self.fixed_time,
    }
  }
}
