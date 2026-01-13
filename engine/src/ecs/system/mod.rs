//! System execution and parameter extraction for ECS logic.
//!
//! # Overview
//!
//! Systems are functions that accept [`Parameter`] types and operate on [`World`](world::World) data.
//! Use [`IntoSystem`] to convert functions into [`System`] instances:
//!
//! ```rust,ignore
//! use rusty_engine::ecs::{query, system::IntoSystem};
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
//! // Convert the function into a system
//! let mut system = IntoSystem::into_system(movement, &mut world);
//!
//! // Execute the system
//! unsafe {
//!     system.run(&mut world);
//! }
//! ```
//!
//! # System Types
//!
//! Systems come in two variants based on their execution context (see [`RunMode`]):
//!
//! - **Parallel**: Query-based systems that can run on worker threads via [`Shard`](world::Shard)
//! - **Exclusive**: Systems with `&mut World` access that must run on the main thread
//!
//! The [`IntoSystem`] trait automatically creates the appropriate variant based on the function signature.
//!
//! # Parameter Types
//!
//! Several types implement [`Parameter`]:
//!
//! - **Query results** (`query::Result<D>`) - Iterator over entities matching component criteria
//! - **Immutable world** (`&World`) - Read-only world access
//! - _Future: Resources, Commands, Events_
//!
//! Note: `&mut World` is handled specially via exclusive systems, not as a parameter.
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
//! ## Multiple Query Parameters
//!
//! ```rust,ignore
//! fn physics(
//!     positions: query::Result<&Position>,
//!     velocities: query::Result<&mut Velocity>,
//! ) {
//!     // Multiple queries in one system
//! }
//! ```
//!
//! ## Exclusive World Access
//!
//! For spawning/despawning entities, use a dedicated exclusive system:
//!
//! ```rust,ignore
//! fn spawner(world: &mut World) {
//!     world.spawn(Enemy { health: 100 });
//! }
//!
//! // Creates an exclusive system (runs on main thread only)
//! let mut system = IntoSystem::into_system(spawner, &mut world);
//! ```
//!
//! ## Optional Components
//!
//! Queries support optional components to match entities that have required components
//! and may have additional optional ones. Note that the archetype storage doesn't store
//! optionals - this is a query-time feature:
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
//! # Custom Systems
//!
//! For advanced use cases, create systems directly:
//!
//! ```rust,ignore
//! // Custom parallel system
//! let system = System::parallel(access_request, |shard| {
//!     // Work with shard...
//! });
//!
//! // Custom exclusive system
//! let system = System::exclusive(access_request, |world| {
//!     // Full world access...
//! });
//! ```
//!
//! # Safety
//!
//! The system framework uses unsafe code to allow multiple parameters to access the world
//! simultaneously. This is safe because:
//!
//! 1. **Disjoint access**: Each parameter accesses different components
//! 2. **Validation**: Access requests prevent aliasing (enforced by scheduler)
//! 3. **Shards**: Parallel systems receive restricted [`Shard`](world::Shard) views
//!
//! See [`function::WithSystemParams`] for implementation details.

use crate::ecs::world;

pub mod function;
pub mod param;
pub mod registry;

pub use param::Parameter;
pub use registry::Registry;

/// A system identifier. This is a non-zero unique identifier for a system type in the ECS.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(u32);

impl Id {
    /// Construct a new system Id from a raw u32 value.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the index of this system if it were to live in indexable storage (e.g. Vec)
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
}

/// Enumeration of system kinds based on their execution context.
pub enum RunMode {
    /// Exclusive world access, runs on main thread only
    Exclusive(Box<dyn FnMut(&mut world::World) + 'static>),

    /// Can run in parallel on worker threads via Shard
    Parallel(Box<dyn FnMut(&mut world::Shard<'_>) + Send + Sync + 'static>),
}

/// A system that can be executed on a world.
///
/// Systems encapsulate logic that operates on world data.
pub struct System {
    /// The access requirements for this system.
    required_access: world::AccessRequest,

    /// The run mode of this system.
    run_mode: RunMode,
}

impl System {
    /// Create an exclusive (main-thread only) system
    pub fn exclusive(
        required_access: world::AccessRequest,
        run: impl FnMut(&mut world::World) + 'static,
    ) -> Self {
        Self {
            required_access,
            run_mode: RunMode::Exclusive(Box::new(run)),
        }
    }

    /// Create a parallel-capable system
    pub fn parallel(
        required_access: world::AccessRequest,
        run: impl FnMut(&mut world::Shard<'_>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            required_access,
            run_mode: RunMode::Parallel(Box::new(run)),
        }
    }

    /// Check if this system can run in parallel on worker threads.
    ///
    /// Returns `true` for parallel systems that can execute via [`world::Shard`],
    /// `false` for exclusive systems that require main-thread execution.
    pub fn is_parallel(&self) -> bool {
        matches!(self.run_mode, RunMode::Parallel(_))
    }

    /// Get the world access requirements for this system.
    ///
    /// Returns a reference to the [`world::AccessRequest`] describing which
    /// components this system reads/writes, or if it requires exclusive world access.
    /// This information is used by schedulers to detect conflicts and determine
    /// safe execution order.
    pub fn required_access(&self) -> &world::AccessRequest {
        &self.required_access
    }

    /// Run the system on the main thread.
    ///
    /// This method executes the system, providing it with access to world data according
    /// to its access requirements. For exclusive systems, this provides direct `&mut World`
    /// access. For parallel systems, this creates a [`world::Shard`] with the appropriate
    /// access grant and executes the system through it.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    ///
    /// 1. **Access validation**: The system's `required_access` has been validated against
    ///    all currently executing or queued systems to ensure no conflicting access occurs.
    ///    This means:
    ///    - No two systems with mutable access to the same component run concurrently
    ///    - No mutable + immutable access to the same component occurs concurrently
    ///    - Exclusive world access (`&mut World`) doesn't run with any other system
    ///
    /// 2. **No concurrent execution**: This system is not currently executing on another
    ///    thread. The `&mut self` requirement helps enforce this, but the caller must
    ///    ensure systems aren't cloned or otherwise duplicated.
    ///
    /// 3. **Grant availability**: For parallel systems, the world must be able to create
    ///    a shard with the required access grant. This should always succeed if access
    ///    validation was performed correctly.
    ///
    /// # Implementation Note
    ///
    /// The unsafety stems from the parameter extraction process within system functions.
    /// When a system with multiple parameters executes (via [`IntoSystem`]), it creates
    /// aliased mutable pointers to the world to extract each parameter independently. This
    /// is sound because each parameter accesses disjoint data (different components), which
    /// is verified by comparing their access requests before execution.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Create a system
    /// let mut system = IntoSystem::into_system(my_system, &mut world);
    ///
    /// // Validate access (scheduler's responsibility)
    /// if !conflicts_with_running_systems(system.required_access()) {
    ///     // Safe because we validated no conflicts
    ///     unsafe { system.run(&mut world); }
    /// }
    /// ```
    pub unsafe fn run(&mut self, world: &mut world::World) {
        match &mut self.run_mode {
            RunMode::Exclusive(func) => func(world),
            RunMode::Parallel(func) => {
                // For main-thread execution of parallel systems, create a shard.
                let mut shard = world.shard(&self.required_access).expect(
                    "Failed to create shard for parallel system on main thread. This indicates a bug in the scheduler or world access management.",
                );
                // Execute the system with the shard.
                func(&mut shard);
                // Release the shard back to the world.
                world.release_shard(shard);
            }
        }
    }

    /// Run the system on a worker thread via a shard.
    ///
    /// This method is specifically for parallel systems executing on worker threads.
    /// The system receives a [`world::Shard`] that has already been validated to have
    /// the required access grant.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    ///
    /// 1. **Parallel system only**: This method panics if called on an exclusive system,
    ///    but the caller should verify `is_parallel()` returns `true` before calling.
    ///
    /// 2. **Valid shard access**: The provided shard must have an access grant that
    ///    satisfies this system's `required_access`. The shard's grant should have been
    ///    obtained from the world using this system's access request.
    ///
    /// 3. **No concurrent execution**: This system is not currently executing on another
    ///    thread. The `&mut self` requirement helps enforce this.
    ///
    /// 4. **Thread safety**: The shard must have been properly sent to the worker thread
    ///    (it implements `Send` but not `Sync`), and the grant will need to be returned
    ///    to the main thread for release after execution.
    ///
    /// # Panics
    ///
    /// Panics if called on an exclusive system (one created with `System::exclusive`).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // On main thread: create shard and send to worker
    /// let grant = world.request_grant(&system.required_access())?;
    /// let shard = world.create_shard(grant);
    ///
    /// // Send to worker thread
    /// thread::spawn(move || {
    ///     // Safe because:
    ///     // - Shard has validated grant
    ///     // - System is not executing elsewhere
    ///     // - Shard was properly sent (it's Send)
    ///     unsafe { system.run_parallel(&mut shard); }
    ///
    ///     // Return grant to main thread
    ///     let grant = shard.into_grant();
    ///     send_to_main_thread(grant);
    /// });
    /// ```
    pub unsafe fn run_parallel(&mut self, shard: &mut world::Shard<'_>) {
        match &mut self.run_mode {
            RunMode::Exclusive(_) => {
                panic!("Cannot run exclusive system in parallel")
            }
            RunMode::Parallel(func) => func(shard),
        }
    }
}

/// A trait for converting types into boxed systems.
pub trait IntoSystem<Marker = ()>: Sized {
    /// Convert the instance into System.
    fn into_system(instance: Self, world: &mut world::World) -> System;
}
