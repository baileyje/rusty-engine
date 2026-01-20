//! Frame timing utilities for measuring update/fixed-update performance.
//!
//! This module provides utilities for measuring frame times in realistic
//! game loop scenarios, tracking statistics like average, percentiles,
//! and variance.

use std::time::{Duration, Instant};

/// Statistics collected from frame timing measurements.
#[derive(Debug, Clone)]
pub struct FrameStats {
    /// Total number of frames measured.
    pub frame_count: usize,
    /// Total duration of all frames.
    pub total_duration: Duration,
    /// Minimum frame time observed.
    pub min_frame_time: Duration,
    /// Maximum frame time observed.
    pub max_frame_time: Duration,
    /// Sorted frame times for percentile calculations.
    sorted_times: Vec<Duration>,
}

impl FrameStats {
    /// Create new frame stats from a collection of frame times.
    pub fn from_times(times: Vec<Duration>) -> Self {
        let frame_count = times.len();
        let total_duration: Duration = times.iter().sum();
        let min_frame_time = times.iter().min().copied().unwrap_or(Duration::ZERO);
        let max_frame_time = times.iter().max().copied().unwrap_or(Duration::ZERO);

        let mut sorted_times = times;
        sorted_times.sort();

        Self {
            frame_count,
            total_duration,
            min_frame_time,
            max_frame_time,
            sorted_times,
        }
    }

    /// Average frame time.
    pub fn average(&self) -> Duration {
        if self.frame_count == 0 {
            Duration::ZERO
        } else {
            self.total_duration / self.frame_count as u32
        }
    }

    /// Median frame time (50th percentile).
    pub fn median(&self) -> Duration {
        self.percentile(50)
    }

    /// Get a specific percentile (0-100).
    pub fn percentile(&self, p: usize) -> Duration {
        if self.sorted_times.is_empty() {
            return Duration::ZERO;
        }
        let p = p.min(100);
        let index = (self.sorted_times.len() * p / 100).min(self.sorted_times.len() - 1);
        self.sorted_times[index]
    }

    /// 99th percentile (worst 1% of frames).
    pub fn p99(&self) -> Duration {
        self.percentile(99)
    }

    /// 95th percentile.
    pub fn p95(&self) -> Duration {
        self.percentile(95)
    }

    /// Standard deviation of frame times.
    pub fn std_dev(&self) -> Duration {
        if self.frame_count < 2 {
            return Duration::ZERO;
        }

        let avg_nanos = self.average().as_nanos() as f64;
        let variance: f64 = self
            .sorted_times
            .iter()
            .map(|t| {
                let diff = t.as_nanos() as f64 - avg_nanos;
                diff * diff
            })
            .sum::<f64>()
            / (self.frame_count - 1) as f64;

        Duration::from_nanos(variance.sqrt() as u64)
    }

    /// Frames per second based on average frame time.
    pub fn fps(&self) -> f64 {
        let avg = self.average();
        if avg.is_zero() {
            0.0
        } else {
            1.0 / avg.as_secs_f64()
        }
    }
}

impl std::fmt::Display for FrameStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} frames, avg: {:.2}ms ({:.1} fps), p99: {:.2}ms, min: {:.2}ms, max: {:.2}ms",
            self.frame_count,
            self.average().as_secs_f64() * 1000.0,
            self.fps(),
            self.p99().as_secs_f64() * 1000.0,
            self.min_frame_time.as_secs_f64() * 1000.0,
            self.max_frame_time.as_secs_f64() * 1000.0,
        )
    }
}

/// Timer for measuring individual frames in a loop.
pub struct FrameTimer {
    frame_times: Vec<Duration>,
    frame_start: Option<Instant>,
}

impl FrameTimer {
    /// Create a new frame timer with pre-allocated capacity.
    pub fn new(expected_frames: usize) -> Self {
        Self {
            frame_times: Vec::with_capacity(expected_frames),
            frame_start: None,
        }
    }

    /// Mark the start of a frame.
    pub fn begin_frame(&mut self) {
        self.frame_start = Some(Instant::now());
    }

    /// Mark the end of a frame and record the duration.
    pub fn end_frame(&mut self) {
        if let Some(start) = self.frame_start.take() {
            self.frame_times.push(start.elapsed());
        }
    }

    /// Get statistics from all recorded frames.
    pub fn stats(self) -> FrameStats {
        FrameStats::from_times(self.frame_times)
    }

    /// Number of frames recorded so far.
    pub fn frame_count(&self) -> usize {
        self.frame_times.len()
    }
}

/// Run a simulated game loop and measure frame times.
///
/// # Arguments
/// * `frame_count` - Number of frames to simulate
/// * `frame_fn` - Function to call each frame (receives frame number)
///
/// # Returns
/// Frame statistics for the simulation run.
pub fn measure_frames<F>(frame_count: usize, mut frame_fn: F) -> FrameStats
where
    F: FnMut(usize),
{
    let mut timer = FrameTimer::new(frame_count);

    for frame in 0..frame_count {
        timer.begin_frame();
        frame_fn(frame);
        timer.end_frame();
    }

    timer.stats()
}

/// Run a simulated game loop with separate update and fixed-update phases.
///
/// This simulates a more realistic game loop where:
/// - `update_fn` is called once per frame (variable timestep)
/// - `fixed_update_fn` is called at a fixed rate (e.g., physics)
///
/// # Arguments
/// * `frame_count` - Number of frames to simulate
/// * `fixed_updates_per_frame` - How many fixed updates per frame (simulates accumulator)
/// * `update_fn` - Variable update function
/// * `fixed_update_fn` - Fixed timestep update function
pub fn measure_game_loop<U, F>(
    frame_count: usize,
    fixed_updates_per_frame: usize,
    mut update_fn: U,
    mut fixed_update_fn: F,
) -> (FrameStats, FrameStats)
where
    U: FnMut(usize),
    F: FnMut(usize),
{
    let mut update_timer = FrameTimer::new(frame_count);
    let mut fixed_timer = FrameTimer::new(frame_count * fixed_updates_per_frame);

    for frame in 0..frame_count {
        // Fixed updates (physics, etc.)
        for _ in 0..fixed_updates_per_frame {
            fixed_timer.begin_frame();
            fixed_update_fn(frame);
            fixed_timer.end_frame();
        }

        // Variable update
        update_timer.begin_frame();
        update_fn(frame);
        update_timer.end_frame();
    }

    (update_timer.stats(), fixed_timer.stats())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn frame_stats_calculations() {
        let times = vec![
            Duration::from_millis(10),
            Duration::from_millis(12),
            Duration::from_millis(11),
            Duration::from_millis(15),
            Duration::from_millis(10),
        ];
        let stats = FrameStats::from_times(times);

        assert_eq!(stats.frame_count, 5);
        assert_eq!(stats.min_frame_time, Duration::from_millis(10));
        assert_eq!(stats.max_frame_time, Duration::from_millis(15));
        // Total: 58ms, average: 11.6ms
        assert!(stats.average().as_millis() >= 11 && stats.average().as_millis() <= 12);
    }

    #[test]
    fn frame_timer_basic() {
        let mut timer = FrameTimer::new(3);

        for _ in 0..3 {
            timer.begin_frame();
            thread::sleep(Duration::from_micros(100));
            timer.end_frame();
        }

        let stats = timer.stats();
        assert_eq!(stats.frame_count, 3);
        assert!(stats.average() >= Duration::from_micros(100));
    }

    #[test]
    fn measure_frames_helper() {
        let stats = measure_frames(5, |_frame| {
            // Simulate some work
            let mut sum = 0u64;
            for i in 0..1000 {
                sum = sum.wrapping_add(i);
            }
            std::hint::black_box(sum);
        });

        assert_eq!(stats.frame_count, 5);
    }
}
