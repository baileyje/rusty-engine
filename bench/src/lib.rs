//! Benchmark utilities for Rusty Engine.
//!
//! This crate provides comprehensive benchmarking infrastructure for the ECS
//! and engine systems, including:
//!
//! - **Microbenchmarks**: Individual ECS operation performance (spawn, iterate, migrate)
//! - **Scenario benchmarks**: Realistic game workloads (particles, game world, physics)
//! - **Memory tracking**: Heap allocation profiling via dhat
//! - **Frame timing**: Update/fixed-update cycle measurement
//!
//! # Running Benchmarks
//!
//! ```bash
//! # Run all benchmarks
//! cargo bench -p rusty_bench
//!
//! # Run specific benchmark group
//! cargo bench -p rusty_bench -- spawn
//!
//! # Run with memory profiling (slower)
//! cargo bench -p rusty_bench --features memory_profiling
//! ```
//!
//! # Benchmark Results
//!
//! Results are written to `target/criterion/` with HTML reports for visualization.
//! Memory profiling results are written to `dhat-heap.json` for viewing with
//! DHAT's viewer.

pub mod components;
pub mod frame_timer;
pub mod memory;
pub mod scenarios;
