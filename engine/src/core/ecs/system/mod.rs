//! System execution and parameter extraction for ECS logic.
//!
//! This module provides a framework for defining game logic as **systems** - functions that
//! operate on world data. The key innovation is the [`Parameter`] trait, which uses Generic
//! Associated Types (GATs) to hide lifetime complexity from function signatures.
//!
//! # Overview
//!
//! Systems are functions that accept [`Parameter`] types and operate on [`World`](world::World) data:
//!
//! ```rust,ignore
//! use rusty_engine::core::ecs::{query, system::function::Wrapper};
//! use rusty_macros::Component;
//!
//! #[derive(Component)]
//! struct Position { x: f32, y: f32 }
//!
//! #[derive(Component)]
//! struct Velocity { dx: f32, dy: f32 }
//!
//! // Clean signature - no explicit lifetimes!
//! fn movement(query: query::Result<(&Velocity, &mut Position)>) {
//!     for (vel, pos) in query {
//!         pos.x += vel.dx;
//!         pos.y += vel.dy;
//!     }
//! }
//!
//! // Wrap the function to make it a system
//! let mut system = Wrapper::new(world.components(), movement);
//!
//! // Execute the system
//! unsafe {
//!     system.run(&mut world);
//! }
//! ```
//!
//! # Parameter Types
//!
//! Several types implement [`Parameter`]:
//!
//! - **Query results** (`query::Result<D>`) - Iterator over entities matching component criteria
//! - **World access** (`&mut World`) - Direct access to world for spawning/despawning entities
//! - _Future: Resources, Commands, Events_
//!
//! # System Function Examples
//!
//! ## Immutable Query
//!
//! ```rust,ignore
//! fn print_positions(query: query::Result<&Position>) {
//!     for pos in query {
//!         println!("Position: ({}, {})", pos.x, pos.y);
//!     }
//! }
//! ```
//!
//! ## Mutable Query
//!
//! ```rust,ignore
//! fn apply_gravity(query: query::Result<&mut Velocity>) {
//!     for vel in query {
//!         vel.dy -= 9.8;
//!     }
//! }
//! ```
//!
//! ## Multiple Parameters
//!
//! ```rust,ignore
//! fn complex_system(
//!     positions: query::Result<&Position>,
//!     mut velocities: query::Result<&mut Velocity>,
//!     world: &mut World,
//! ) {
//!     // Use all parameters
//! }
//! ```
//!
//! ## Optional Components
//!
//! ```rust,ignore
//! fn healing(query: query::Result<(&Player, Option<&mut Health>)>) {
//!     for (player, maybe_health) in query {
//!         if let Some(health) = maybe_health {
//!             health.current += 1;
//!         }
//!     }
//! }
//! ```
//!
//! # How It Works
//!
//! The magic happens through the [`Parameter`] trait's Generic Associated Type:
//!
//! ```rust,ignore
//! pub trait Parameter {
//!     type Value<'w>: Parameter;  // GAT carries world lifetime
//!
//!     unsafe fn get<'w>(world: &'w mut World) -> Self::Value<'w>;
//! }
//! ```
//!
//! When you write `query: query::Result<&Position>`:
//! - The signature uses `query::Result<'_, &Position>` (elided lifetime)
//! - At runtime, [`Parameter::get()`] returns `query::Result<'w, &Position>`
//! - The `'w` lifetime is tied to the world reference
//! - Rust's type system ensures references can't outlive the world
//!
//! # Safety
//!
//! The system framework uses unsafe code to allow multiple parameters to access the world
//! simultaneously. This is safe because:
//!
//! 1. **Disjoint access**: Each parameter accesses different components
//! 2. **Validation**: Component specs prevent aliasing (enforced by scheduler)
//! 3. **Scope**: Raw pointer aliasing is limited to parameter extraction
//!
//! See [`function::WithSystemParams`] for implementation details.
//!
//! # Scheduler Integration (Future)
//!
//! Systems will be registered with a scheduler that:
//! - Validates component specs don't conflict
//! - Determines safe execution order
//! - Runs systems in parallel when possible
//!
//! ```rust,ignore
//! scheduler.add_system(physics);      // Writes Position
//! scheduler.add_system(rendering);    // Reads Position - must run after physics
//! scheduler.add_system(ai);           // Writes AIState - can run parallel with physics
//! ```

use crate::core::ecs::{component, world};

pub mod function;
pub mod param;

pub use param::Parameter;

/// A system that can be executed on a world.
///
/// Systems encapsulate logic that operates on world data. This trait allows systems
/// to be stored in a registry and executed by a scheduler.
///
/// # Implementation
///
/// You typically don't implement this trait directly. Instead, use [`function::Wrapper`]
/// to convert a function into a system:
///
/// ```rust,ignore
/// use rusty_engine::core::ecs::system::function::Wrapper;
///
/// fn my_logic(query: query::Result<&Position>) {
///     // System logic here
/// }
///
/// let system = Wrapper::new(world.components(), my_logic);
/// ```
///
/// # Safety
///
/// The `run` method is unsafe because it may create aliased mutable references to the world.
/// Safety is ensured by:
/// - Component specs preventing conflicting access
/// - Scheduler validating system compatibility
/// - Single-threaded execution (currently)
pub trait System: Send + Sync {
    /// Get the component specification for this system.
    ///
    /// The component spec describes which components this system accesses and how
    /// (read vs write). The scheduler uses this to:
    /// - Detect conflicts between systems
    /// - Determine safe execution order
    /// - Enable parallel execution when safe
    ///
    /// # Returns
    ///
    /// A reference to the component specification computed when the system was created.
    fn component_spec(&self) -> &component::Spec;

    /// Execute the system on the given world.
    ///
    /// # Safety
    ///
    /// Caller must ensure:
    /// - No aliasing violations occur (validated via component specs)
    /// - System is not run concurrently with conflicting systems
    /// - World reference is valid for the duration of the call
    ///
    /// The scheduler is responsible for upholding these invariants.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut system = Wrapper::new(world.components(), my_system);
    ///
    /// // Safe because we're the only one accessing the world
    /// unsafe {
    ///     system.run(&mut world);
    /// }
    /// ```
    unsafe fn run(&mut self, world: &mut world::World);
}
