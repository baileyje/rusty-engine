//! Realistic game scenario benchmarks.
//!
//! These scenarios simulate real-world ECS usage patterns with representative
//! entity counts, component combinations, and system workloads.
//!
//! # Scenarios
//!
//! - **Particles**: High entity count, simple components, short lifetimes
//! - **Game World**: Mixed archetypes, AI, combat, inventory systems
//! - **Physics**: Transform integration, collision detection patterns

pub mod game_world;
pub mod particles;
pub mod physics;

pub use game_world::{GameWorldConfig, GameWorldScenario};
pub use particles::{ParticleConfig, ParticleScenario};
pub use physics::{PhysicsConfig, PhysicsScenario};

/// Common trait for benchmark scenarios.
pub trait Scenario {
    /// Human-readable name of the scenario.
    fn name(&self) -> &'static str;

    /// Brief description of what this scenario tests.
    fn description(&self) -> &'static str;

    /// Number of entities in this scenario.
    fn entity_count(&self) -> usize;

    /// Set up the scenario (spawn entities, initialize state).
    fn setup(&mut self);

    /// Run one "frame" of the scenario.
    fn update(&mut self);

    /// Clean up the scenario.
    fn teardown(&mut self);
}
