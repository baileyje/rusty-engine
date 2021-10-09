use std::time::{Duration, Instant};

/// A frame represents a specific timeframe elapsed within the engines. Each frame captures total elapsed time as well as the delta time 
/// since the last frame. New frames are intended to be generated from a pervious frame using the `next()` method. Generally this can be 
/// invoked on each iteration of a game loop.
#[derive(Debug)]
pub struct Frame {
  instant: Instant,
  /// The time delta since the last frame
  pub delta: Duration,
  /// The total elapsed time since the first frame
  pub time: Duration,
}

impl Frame {
  /// Construct a new `Frame` with delta and time set to `0`.
  pub fn new() -> Self {
    return Self {
      instant: Instant::now(),
      delta: Duration::ZERO,
      time: Duration::ZERO
    }
  }

  /// Create the next frame from an existing frame. This will capture the delta from the last frame and update the cumulative time.
  pub fn next(self) -> Frame {
    let elapsed = self.instant.elapsed();
    Frame {
      instant: Instant::now(),
      delta: elapsed,
      time: self.time + elapsed
    }
  }

}