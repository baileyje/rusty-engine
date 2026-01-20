//! Memory profiling utilities using dhat.
//!
//! This module provides utilities for tracking memory allocations during
//! benchmarks. It uses dhat-rs to track heap allocations.
//!
//! # Usage
//!
//! Memory profiling adds overhead and should only be enabled when needed:
//!
//! ```bash
//! cargo bench -p rusty_bench --features memory_profiling
//! ```
//!
//! # Viewing Results
//!
//! After running with memory profiling, view results at:
//! <https://nnethercote.github.io/dh_view/dh_view.html>
//!
//! Load the generated `dhat-heap.json` file.

/// Memory statistics captured during a benchmark run.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total bytes allocated during the measurement.
    pub bytes_allocated: u64,
    /// Total number of allocations.
    pub allocation_count: u64,
    /// Peak heap usage in bytes.
    pub peak_bytes: u64,
}

impl MemoryStats {
    /// Calculate bytes per entity for a given entity count.
    pub fn bytes_per_entity(&self, entity_count: usize) -> f64 {
        if entity_count == 0 {
            0.0
        } else {
            self.bytes_allocated as f64 / entity_count as f64
        }
    }

    /// Calculate allocations per entity for a given entity count.
    pub fn allocations_per_entity(&self, entity_count: usize) -> f64 {
        if entity_count == 0 {
            0.0
        } else {
            self.allocation_count as f64 / entity_count as f64
        }
    }
}

impl std::fmt::Display for MemoryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "allocated: {} bytes ({} allocs), peak: {} bytes",
            self.bytes_allocated, self.allocation_count, self.peak_bytes
        )
    }
}

/// Guard that captures memory statistics when dropped.
///
/// Create this at the start of a measurement, perform operations,
/// then drop it (or call `finish()`) to get statistics.
#[cfg(feature = "memory_profiling")]
pub struct MemoryProfiler {
    _profiler: dhat::Profiler,
}

#[cfg(feature = "memory_profiling")]
impl MemoryProfiler {
    /// Start memory profiling.
    ///
    /// Note: Only one profiler can be active at a time.
    pub fn start() -> Self {
        Self {
            _profiler: dhat::Profiler::new_heap(),
        }
    }

    /// Finish profiling and get statistics.
    ///
    /// This also writes the detailed heap profile to `dhat-heap.json`.
    pub fn finish(self) -> MemoryStats {
        let stats = dhat::HeapStats::get();
        MemoryStats {
            bytes_allocated: stats.total_bytes as u64,
            allocation_count: stats.total_blocks as u64,
            peak_bytes: stats.max_bytes as u64,
        }
    }
}

#[cfg(not(feature = "memory_profiling"))]
pub struct MemoryProfiler;

#[cfg(not(feature = "memory_profiling"))]
impl MemoryProfiler {
    /// No-op when memory profiling is disabled.
    pub fn start() -> Self {
        Self
    }

    /// Returns empty stats when memory profiling is disabled.
    pub fn finish(self) -> MemoryStats {
        MemoryStats::default()
    }
}

/// Measure memory usage of a closure.
///
/// When the `memory_profiling` feature is enabled, this tracks allocations.
/// Otherwise, it just runs the closure and returns empty stats.
pub fn measure_memory<F, R>(f: F) -> (R, MemoryStats)
where
    F: FnOnce() -> R,
{
    let profiler = MemoryProfiler::start();
    let result = f();
    let stats = profiler.finish();
    (result, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_stats_display() {
        let stats = MemoryStats {
            bytes_allocated: 1024,
            allocation_count: 10,
            peak_bytes: 512,
        };
        let display = format!("{}", stats);
        assert!(display.contains("1024 bytes"));
        assert!(display.contains("10 allocs"));
        assert!(display.contains("peak: 512"));
    }

    #[test]
    fn bytes_per_entity_calculation() {
        let stats = MemoryStats {
            bytes_allocated: 10000,
            allocation_count: 100,
            peak_bytes: 5000,
        };
        assert!((stats.bytes_per_entity(100) - 100.0).abs() < f64::EPSILON);
        assert!((stats.allocations_per_entity(100) - 1.0).abs() < f64::EPSILON);
    }
}
