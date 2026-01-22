//! Phase execution for ECS system scheduling.
//!
//! A [`Phase`] represents a named execution stage in the game loop (e.g., "Update", "FixedUpdate",
//! "Render"). Each phase contains systems that are scheduled for parallel execution based on their
//! resource access requirements.
//!
//! # Architecture
//!
//! ```text
//! Schedule
//!   ├── Phase: "FixedUpdate"
//!   │     ├── exclusive_systems: [spawn_system, despawn_system]
//!   │     ├── systems: [physics_a, physics_b, physics_c, ...]
//!   │     └── plan: [Group([Unit([0,1]), Unit([2])]), Group([Unit([3,4])])]
//!   │
//!   ├── Phase: "Update"
//!   │     ├── exclusive_systems: [event_processor]
//!   │     ├── systems: [ai_system, animation_system, ...]
//!   │     └── plan: [...]
//!   │
//!   └── Phase: "Render"
//!         └── ...
//! ```
//!
//! # Execution Model
//!
//! Phase execution follows a strict two-stage model:
//!
//! 1. **Pre-phase (Sequential)**: All exclusive systems (requiring `&mut World`) run first,
//!    in registration order. This handles structural changes like spawn/despawn.
//!
//! 2. **Main phase (Parallel)**: Non-exclusive systems run in groups. Within each group,
//!    units execute in parallel. Within each unit, systems execute sequentially.
//!
//! ```text
//! Phase::run()
//!   │
//!   ├─► Pre-phase: exclusive_systems (sequential)
//!   │     ├── system_a.run(&mut world)
//!   │     └── system_b.run(&mut world)
//!   │
//!   └─► Main phase: plan groups (sequential across groups)
//!         │
//!         ├─► Group 1 (parallel across units)
//!         │     ├── Unit A: [sys1, sys2] → Thread 1
//!         │     └── Unit B: [sys3]       → Thread 2
//!         │     └── ─── barrier ───
//!         │
//!         └─► Group 2 (parallel across units)
//!               └── Unit C: [sys4, sys5] → Thread 1
//! ```
//!
//! # Safety Invariants
//!
//! The phase execution system maintains several critical invariants:
//!
//! ## Invariant 1: Exclusive System Isolation
//!
//! Systems requiring mutable world access (`world_mut() == true`) are stored separately
//! in `exclusive_systems` and execute before any parallel work begins. This ensures:
//! - Structural changes (spawn/despawn) complete before component access
//! - No parallel system observes mid-mutation world state
//! - Command buffers can be flushed safely
//!
//! ## Invariant 2: Disjoint System Access
//!
//! The planner guarantees that within any [`Group`], no two [`Unit`]s have conflicting
//! resource access. This is verified by [`AccessRequest::conflicts_with()`] during planning.
//! Consequently:
//! - Units in the same group can execute on different threads simultaneously
//! - Each unit's shard grant is non-overlapping with other units in the group
//!
//! ## Invariant 3: Unique System Indices Per Group
//!
//! Each system index appears in **exactly one** unit within a group. The planner
//! constructs units by bundling systems with identical access patterns, ensuring
//! no system is executed twice or accessed from multiple threads.
//!
//! ## Invariant 4: Systems Vec Stability During Execution
//!
//! The `systems` vector must not be modified while `run_group()` is executing.
//! Raw pointers into the vector are used for zero-copy parallel dispatch.
//! The executor scope ensures all workers complete before `run_group()` returns.
//!
//! ## Invariant 5: Grant Lifecycle Management
//!
//! Every shard created via `world.shard()` must have its grant released via either:
//! - `world.release_shard(shard)` - normal path
//! - `world.release_grant(&grant)` after `shard.into_grant()` - parallel path
//!
//! Failure to release grants will cause subsequent shard requests to fail with
//! conflict errors, even if the original access is complete.
//!
//! # Performance Characteristics
//!
//! - **Plan cloning**: The plan is cloned each `run()` call to avoid borrow conflicts.
//!   For phases with many groups/units, consider caching or using indices.
//! - **Shard creation**: O(g) where g = active grants. Typically small.
//! - **System dispatch**: Zero-copy via raw pointers. No per-system allocation.
//! - **Bundling benefit**: Systems with identical access share one shard, reducing
//!   grant tracking overhead and improving cache locality.

use std::any::TypeId;

use crate::{
    core::tasks,
    ecs::{
        schedule::plan::{GraphColorPlanner, Group, Planner, Task, Unit},
        system, world,
    },
};

/// Wrapper struct over a type ID to cleanup the schedule code by providing an opaque phase ID.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Id(TypeId);

impl Id {
    /// Construct a new ID from a label type.
    #[inline]
    pub const fn new<L: Label>() -> Self {
        Self(TypeId::of::<L>())
    }
}

/// A marker trait for phase identifiers.
///
/// Phase labels are zero-sized types used to identify phases in a [`Schedule`].
/// The trait provides a human-readable name for debugging and logging.
///
/// # Implementing
///
/// The easiest way to define phase labels is with the [`define_phase!`] macro:
///
/// ```rust,ignore
/// define_phase!(Update, FixedUpdate, Render);
/// ```
///
/// For custom behavior, implement the trait manually:
///
/// ```rust,ignore
/// struct MyPhase;
///
/// impl Label for MyPhase {
///     fn name() -> &'static str { "MyPhase" }
/// }
/// ```
///
/// # Design Notes
///
/// Phase labels use the type system for compile-time safety - you can't misspell
/// a phase name or use an undefined phase without a compiler error. The [`name()`]
/// method exists solely for debugging and logging purposes.
pub trait Label: 'static {
    /// Returns a human-readable name for this phase.
    ///
    /// Used for debugging, logging, and error messages. Should return a stable
    /// identifier (typically the struct name).
    fn name() -> &'static str;

    /// Get the phase ID for a label.
    fn id(self) -> Id;
}

/// Defines one or more phase label types.
///
/// This macro creates zero-sized structs that implement [`Label`], providing
/// an ergonomic way to define phases for your game or application.
///
/// # Example
///
/// ```rust,ignore
/// use rusty_engine::define_phase;
///
/// // Define multiple phases at once
/// define_phase!(PreUpdate, Update, PostUpdate, Render);
///
/// // Use in schedule
/// schedule.add_system(Update, my_system, &mut world);
/// schedule.run(Update, &mut world, &executor);
/// ```
///
/// # Generated Code
///
/// For each identifier, the macro generates:
///
/// ```rust,ignore
/// #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
/// pub struct Update;
///
/// impl Label for Update {
///     fn name() -> &'static str { "Update" }
/// }
/// ```
#[macro_export]
macro_rules! define_phase {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
            pub struct $name;

            impl $crate::ecs::schedule::Label for $name {
                #[inline]
                fn name() -> &'static str {
                    stringify!($name)
                }

                fn id(self) -> $crate::ecs::schedule::Id {
                     $crate::ecs::schedule::Id::new::<Self>()
                }
            }
        )*
    };
}

/// A Send-safe wrapper around a system pointer for parallel execution.
///
/// This type enables zero-copy parallel dispatch of systems by wrapping a raw pointer
/// to a [`System`] in the phase's systems vector. The pointer can be sent across thread
/// boundaries, allowing workers to execute systems without cloning or moving them.
///
/// # Design Rationale
///
/// The alternative approaches (documented in `SYSTEM_STORAGE_ALTERNATIVES.md`) include:
/// - `mem::swap` with placeholder systems (~5-10% overhead)
/// - `Vec<Option<System>>` with take/restore
/// - Reconstructing vectors per group
///
/// Raw pointers were chosen for zero overhead in the hot path, with safety guaranteed
/// by the scheduler's invariants rather than Rust's borrow checker.
///
/// # Safety Contract
///
/// This struct is marked as `Send`, but the caller **must** ensure:
///
/// 1. **Pointer Validity**: The pointer must point to a valid `System` for the entire
///    duration of its use. The source `Vec<System>` must not be reallocated, dropped,
///    or have elements removed while handles exist.
///
/// 2. **Exclusive Access**: No two `SystemHandle` instances may point to the same system
///    concurrently. The scheduler guarantees this by assigning each system index to
///    exactly one unit per group (see Invariant 3 in module docs).
///
/// 3. **Scope Containment**: All handles must be used and dropped within the executor
///    scope that created them. The scoped thread pool ensures all workers complete
///    before the scope exits, at which point handles are invalidated.
///
/// 4. **No Concurrent Mutation**: The systems vector must not be mutated (push, pop,
///    clear, etc.) while any handles exist. Only the pointed-to systems may be mutated
///    through their handles.
///
/// # Example Safety Violation
///
/// ```rust,ignore
/// // UNSAFE: Two handles to the same system
/// let handle1 = SystemHandle(systems.as_mut_ptr().add(0));
/// let handle2 = SystemHandle(systems.as_mut_ptr().add(0)); // Same index!
/// // Concurrent execution would cause data race
/// ```
///
/// The planner prevents this by construction - each system index appears in only one unit.
struct SystemHandle(*mut system::System);

// SAFETY: SystemHandle can be sent across threads because:
// 1. The pointed-to System contains only Send types (boxed closures marked Send)
// 2. The scheduler ensures exclusive access (no two threads access the same system)
// 3. The executor scope ensures the source Vec outlives all handles
unsafe impl Send for SystemHandle {}

impl SystemHandle {
    /// Execute the system with the given shard.
    ///
    /// Invokes the system's parallel execution path, which runs the system function
    /// with access to the world mediated through the shard's grant.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The pointer is valid (source Vec not modified)
    /// - No other thread is concurrently accessing this same system
    /// - The shard's grant covers the system's required access
    ///
    /// These invariants are maintained by the phase executor:
    /// - Pointer validity: Executor scope ensures Vec outlives handles
    /// - Exclusive access: Planner assigns each index to one unit per group
    /// - Grant coverage: Shard created from unit's `required_access()`
    #[inline]
    unsafe fn run_parallel(
        &mut self,
        shard: &mut world::Shard,
        command_buffer: &system::CommandBuffer,
    ) {
        // SAFETY: Caller ensures no concurrent access to the same system.
        // The system's run_parallel is unsafe because it bypasses normal
        // borrow checking, relying on the shard's grant for safety.
        unsafe { (*self.0).run_parallel(shard, command_buffer) }
    }
}

/// A named execution stage containing systems scheduled for parallel execution.
///
/// Phases represent logical stages in the game loop such as "Update", "FixedUpdate", and "Render".
/// Each phase maintains its own systems and execution plan, enabling isolation between stages.
///
/// See the [module documentation](self) for execution model details, safety invariants,
/// and guidance on integrating phases into a `Schedule` container.
///
/// # Thread Safety
///
/// `Phase` is `!Send` and `!Sync`. All parallel execution is contained within [`run()`](Self::run)
/// using scoped threads, with orchestration performed on a single thread.
///
/// # Example
///
/// ```rust,ignore
/// let mut phase = Phase::new();
///
/// // Exclusive systems (world_mut) are automatically separated
/// phase.add_system(spawn_entities.into_system(&mut world));
///
/// // Parallel systems are organized by the planner
/// phase.add_system(physics_system.into_system(&mut world));
/// phase.add_system(ai_system.into_system(&mut world));
///
/// phase.run(&mut world, &executor);
/// ```
pub struct Phase {
    /// Systems requiring exclusive world access, run sequentially in pre-phase.
    ///
    /// Routed here automatically by [`add_system()`](Self::add_system) when
    /// `required_access().world_mut() == true`. Execution order matches addition order.
    exclusive_systems: Vec<system::System>,

    /// Non-exclusive systems eligible for parallel execution.
    ///
    /// # Invariant
    ///
    /// All systems here have `required_access().world_mut() == false`.
    /// See module Invariant 4 for stability requirements during execution.
    systems: Vec<system::System>,

    /// The execution plan generated by the planner.
    ///
    /// Automatically regenerated by [`add_system()`](Self::add_system).
    /// Structure: `Vec<Group>` where each group contains non-conflicting units.
    plan: Vec<Group>,

    /// The planner algorithm for generating execution plans.
    ///
    /// Defaults to [`GraphColorPlanner<WelshPowell>`]. Can be customized via
    /// [`with_planner()`](Self::with_planner) for debugging or optimization.
    planner: Box<dyn Planner>,
}

impl Phase {
    /// Creates a new phase with the default Welsh-Powell graph coloring planner.
    #[inline]
    pub fn new() -> Self {
        Self::with_planner(Box::new(GraphColorPlanner::WELSH_POWELL))
    }

    /// Creates a new phase with a custom planner.
    ///
    /// Use [`SequentialPlanner`](super::plan::SequentialPlanner) for debugging
    /// or single-threaded execution.
    #[inline]
    pub fn with_planner(planner: Box<dyn Planner>) -> Self {
        Self {
            exclusive_systems: Vec::new(),
            systems: Vec::new(),
            plan: Vec::new(),
            planner,
        }
    }

    /// Adds a system to this phase, automatically routing based on access requirements.
    ///
    /// - Systems with `world_mut()` access → `exclusive_systems` (pre-phase)
    /// - All other systems → `systems` (parallel execution)
    ///
    /// The execution plan is regenerated after adding non-exclusive systems.
    pub fn add_system(&mut self, system: system::System) {
        if system.required_access().world_mut() {
            self.exclusive_systems.push(system);
            return;
        }
        self.systems.push(system);
        self.plan();
    }

    // Get the length of the systems for this phase.
    #[inline]
    pub fn systems_len(&self) -> usize {
        self.systems.len() + self.exclusive_systems.len()
    }

    /// Regenerates the execution plan using the configured planner.
    ///
    /// Called automatically by [`add_system()`](Self::add_system). May be called
    /// manually if system access patterns change dynamically.
    pub fn plan(&mut self) {
        let tasks = self
            .systems
            .iter()
            .enumerate()
            .map(|(idx, sys)| Task::new(idx, sys.required_access().clone()))
            .collect::<Vec<_>>();
        self.plan = self.planner.plan(&tasks);
    }

    /// Executes all systems in this phase.
    ///
    /// Runs the pre-phase (exclusive systems) then the main phase (parallel groups).
    /// See module documentation for the execution model.
    pub fn run(&mut self, world: &mut world::World, executor: &tasks::Executor) {
        // Pre-phase: exclusive systems with full world access
        for system in self.exclusive_systems.iter_mut() {
            // SAFETY: Exclusive phase - no other systems running, no active shards.
            // The world reference is unique here.
            unsafe { system.run_exclusive(world) };
        }

        // Main phase: parallel groups

        // Create a command buffer for systems in this phase
        let command_buffer = system::CommandBuffer::new();

        // Execute each group sequentially
        for group in self.plan.iter() {
            run_group(&mut self.systems, world, group, &command_buffer, executor);
        }

        // Execute the commands in the buffer
        command_buffer.flush(world);
    }
}

/// Executes a group of units, parallelizing when beneficial.
///
/// Single-unit groups run inline to avoid parallelization overhead.
/// Multi-unit groups dispatch to the executor's thread pool.
///
/// Note: This is a detached function from the phase to avoid borrowing Phase multiple times in
/// iterating the plan.
///
/// # Grant Lifecycle
///
/// 1. Shards created on main thread (validates grants via `GrantTracker`)
/// 2. Shards moved to workers, converted to grants via `into_grant()`
/// 3. Grants returned to main thread and released
///
/// This ensures grant tracking remains single-threaded while execution is parallel.
fn run_group(
    systems: &mut [system::System],
    world: &mut world::World,
    group: &Group,
    command_buffer: &system::CommandBuffer,
    executor: &tasks::Executor,
) {
    // Optimization: single unit runs inline without thread dispatch overhead
    if group.units().len() == 1 {
        let unit = &group.units()[0];
        match world.shard(unit.required_access()) {
            Ok(mut shard) => {
                for &idx in unit.system_indexes() {
                    if let Some(system) = systems.get_mut(idx) {
                        // SAFETY: Shard's grant covers this system's required access
                        // (unit's required_access is the shared access of all its systems).
                        unsafe { system.run_parallel(&mut shard, command_buffer) };
                    }
                }
                world.release_shard(shard);
            }
            Err(e) => {
                eprintln!("Failed to create shard for unit: {:?}", e);
            }
        }
        return;
    }

    // === Multi-unit parallel execution ===

    // Step 1: Acquire all shards on main thread
    // This validates grants and ensures no conflicts between units in this group.
    // If any shard fails, we must release all previously acquired shards.
    let mut shards: Vec<(world::Shard, &Unit)> = Vec::new();
    for unit in group.units() {
        match world.shard(unit.required_access()) {
            Ok(shard) => shards.push((shard, unit)),
            Err(e) => {
                eprintln!("Failed to create shard for unit: {:?}", e);
                for (shard, _) in shards {
                    world.release_shard(shard);
                }
                return;
            }
        }
    }

    // Step 2: Create system handles for parallel dispatch
    // SAFETY: See SystemHandle documentation for the full safety contract.
    // Key invariants maintained here:
    // - Indices come from planner, which ensures validity and uniqueness per group
    // - Systems vec is not modified during the executor scope below
    // - Each handle is used by exactly one worker thread
    let system_handles: Vec<Vec<SystemHandle>> = group
        .units()
        .iter()
        .map(|unit| {
            unit.system_indexes()
                .iter()
                .map(|&idx| {
                    // SAFETY: idx < self.systems.len() guaranteed by planner construction
                    SystemHandle(unsafe { systems.as_mut_ptr().add(idx) })
                })
                .collect()
        })
        .collect();

    // Step 3: Dispatch to thread pool
    // The scoped executor ensures all workers complete before this block exits,
    // which is critical for pointer validity (systems vec must outlive handles).
    let grant_futures = executor.scope(|scope| {
        let mut futures = Vec::new();

        for ((mut shard, _unit), mut handles) in shards.into_iter().zip(system_handles.into_iter())
        {
            let future = scope.spawn_with_result(move || {
                // Sequential execution within unit (cache-friendly, shared shard)
                for handle in handles.iter_mut() {
                    // SAFETY: See SystemHandle::run_parallel documentation.
                    // Exclusive access guaranteed by planner's disjoint index assignment.
                    unsafe {
                        handle.run_parallel(&mut shard, command_buffer);
                    }
                }

                // Convert shard to grant for main-thread release
                shard.into_grant()
            });

            futures.push(future);
        }

        futures
    });

    // Step 4: Collect results and release grants
    // This must happen on the main thread (GrantTracker is not thread-safe).
    for future in grant_futures {
        match future.wait() {
            Ok(grant) => world.release_grant(&grant),
            Err(e) => eprintln!("Task failed to complete: {:?}", e),
        }
    }
}

/// Implements the default constructor for Phase.
impl Default for Phase {
    fn default() -> Self {
        Self::new()
    }
}

/// A reusable sequence of phases for ordered execution.
///
/// `Sequence` allows you to define an execution order once and reuse it
/// across multiple frames. This is useful for game loops that always run
/// phases in the same order.
///
/// # Building a Sequence
///
/// Use the builder pattern to construct a sequence:
///
/// ```rust,ignore
/// use rusty_engine::define_phase;
/// use rusty_engine::ecs::schedule::Sequence;
///
/// define_phase!(FixedUpdate, Update, LateUpdate, Render);
///
/// let sequence = Sequence::new()
///     .then(FixedUpdate)
///     .then(Update)
///     .then(LateUpdate)
///     .then(Render);
/// ```
///
/// # Running a Sequence
///
/// Pass the sequence to [`Schedule::run_sequence`]:
///
/// ```rust,ignore
/// // Game loop
/// loop {
///     schedule.run_sequence(&sequence, &mut world, &executor);
/// }
/// ```
///
/// # Flexibility
///
/// Sequences are just data - you can have multiple sequences for different
/// scenarios:
///
/// ```rust,ignore
/// // Normal frame
/// let normal_frame = Sequence::new()
///     .then(FixedUpdate)
///     .then(Update)
///     .then(Render);
///
/// // Paused frame (skip Update)
/// let paused_frame = Sequence::new()
///     .then(Render);
///
/// // Loading frame (minimal processing)
/// let loading_frame = Sequence::new()
///     .then(LoadingUpdate)
///     .then(Render);
/// ```
#[derive(Debug, Clone, Default)]
pub struct Sequence {
    phases: Vec<Id>,
}

impl Sequence {
    /// Creates a new empty phase sequence.
    #[inline]
    pub fn new() -> Self {
        Self { phases: Vec::new() }
    }

    /// Creates a sequence with pre-allocated capacity.
    ///
    /// Use this when you know how many phases will be in the sequence
    /// to avoid reallocations.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            phases: Vec::with_capacity(capacity),
        }
    }

    /// Adds a phase to the end of the sequence.
    ///
    /// Returns `self` for method chaining.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sequence = Sequence::new()
    ///     .then(Update)
    ///     .then(Render);
    /// ```
    #[inline]
    pub fn then<L: Label>(mut self, label: L) -> Self {
        self.phases.push(label.id());
        self
    }

    /// Adds a phase to the sequence in place.
    ///
    /// Unlike [`then`](Self::then), this doesn't consume self, allowing
    /// conditional or dynamic sequence building.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut sequence = Sequence::new();
    /// sequence.push(Update);
    /// if should_render {
    ///     sequence.push(Render);
    /// }
    /// ```
    #[inline]
    pub fn push<L: Label>(&mut self, label: L) {
        self.phases.push(label.id());
    }

    /// Returns the list if phase IDs for this sequence.
    pub fn phases(&self) -> &[Id] {
        &self.phases
    }

    /// Returns the number of phases in the sequence.
    #[inline]
    pub fn len(&self) -> usize {
        self.phases.len()
    }

    /// Returns `true` if the sequence contains no phases.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.phases.is_empty()
    }

    /// Clears all phases from the sequence.
    #[inline]
    pub fn clear(&mut self) {
        self.phases.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    use std::thread;
    use std::time::Duration;

    use rusty_macros::Component;

    use crate::ecs::{
        system::{IntoSystem, param::Query},
        world,
    };

    use super::*;

    // Test phases defined using the macro
    define_phase!(Update, FixedUpdate, Render);

    // Test components for mixed workflows
    #[derive(Component, Clone, Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Clone, Debug, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Component, Clone, Debug, PartialEq)]
    struct Health {
        value: i32,
    }

    #[test]
    fn empty_phase() {
        // When
        let phase = Phase::new();
        // Then
        assert!(phase.systems.is_empty());
        assert!(phase.plan.is_empty());
    }

    #[test]
    fn add_systems() {
        // Given
        let mut world = world::World::new(world::Id::new(0));

        fn system_a() {}
        fn system_b() {}
        let mut phase = Phase::new();
        let system_a = system_a.into_system(&mut world);
        let system_b = system_b.into_system(&mut world);

        // When
        phase.add_system(system_a);

        // Then
        assert_eq!(phase.systems.len(), 1);
        assert_eq!(phase.plan.len(), 1);

        // And When
        phase.add_system(system_b);

        // Then
        assert_eq!(phase.systems.len(), 2);
        assert_eq!(
            phase
                .plan
                .iter()
                .map(|g| g
                    .units()
                    .iter()
                    .map(|u| u.system_indexes().len())
                    .sum::<usize>())
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn exclusive_systems_run_first() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);
        let counter_clone2 = Arc::clone(&counter);

        fn exclusive_system(_world: &mut world::World) {
            // Exclusive world access
        }

        let regular_system = move || {
            // Regular system should run after exclusive
            counter_clone.fetch_add(1, Ordering::SeqCst);
        };

        let exclusive_checker = move |_world: &mut world::World| {
            // Verify no regular systems have run yet
            assert_eq!(counter_clone2.load(Ordering::SeqCst), 0);
        };

        let mut phase = Phase::new();
        phase.add_system(regular_system.into_system(&mut world));
        phase.add_system(exclusive_system.into_system(&mut world));
        phase.add_system(exclusive_checker.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(2);
        phase.run(&mut world, &executor);

        // Then: Counter should have been incremented (regular system ran)
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sequential_execution_preserves_order() {
        // Given: Systems that must run in order
        let mut world = world::World::new(world::Id::new(0));
        let execution_order = Arc::new(Mutex::new(Vec::new()));

        let order1 = Arc::clone(&execution_order);
        let system_a = move || {
            order1.lock().unwrap().push(1);
        };

        let order2 = Arc::clone(&execution_order);
        let system_b = move || {
            order2.lock().unwrap().push(2);
        };

        let order3 = Arc::clone(&execution_order);
        let system_c = move || {
            order3.lock().unwrap().push(3);
        };

        let mut phase = Phase::new();
        phase.add_system(system_a.into_system(&mut world));
        phase.add_system(system_b.into_system(&mut world));
        phase.add_system(system_c.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(1);
        phase.run(&mut world, &executor);

        // Then: Should execute in order (all have no resources, bundled together)
        let order = execution_order.lock().unwrap();
        assert_eq!(*order, vec![1, 2, 3]);
    }

    #[test]
    fn parallel_systems_with_disjoint_components() {
        // Given: Systems that can run in parallel
        let mut world = world::World::new(world::Id::new(0));

        // Spawn some entities
        world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 1.0 }));
        world.spawn((Position { x: 10.0, y: 10.0 }, Health { value: 100 }));

        let start_time = Arc::new(Mutex::new(None::<std::time::Instant>));
        let system_times = Arc::new(Mutex::new(Vec::new()));

        // System A: Read Position (should run in parallel with B)
        let start_clone = Arc::clone(&start_time);
        let times_clone = Arc::clone(&system_times);
        let system_a = move |_query: Query<&Position>| {
            let start = start_clone.lock().unwrap().unwrap();
            thread::sleep(Duration::from_millis(50));
            let elapsed = start.elapsed().as_millis();
            times_clone.lock().unwrap().push(("A", elapsed));
        };

        // System B: Read Health (should run in parallel with A)
        let start_clone2 = Arc::clone(&start_time);
        let times_clone2 = Arc::clone(&system_times);
        let system_b = move |_query: Query<&Health>| {
            let start = start_clone2.lock().unwrap().unwrap();
            thread::sleep(Duration::from_millis(50));
            let elapsed = start.elapsed().as_millis();
            times_clone2.lock().unwrap().push(("B", elapsed));
        };

        let mut phase = Phase::new();
        phase.add_system(system_a.into_system(&mut world));
        phase.add_system(system_b.into_system(&mut world));

        // When
        *start_time.lock().unwrap() = Some(std::time::Instant::now());
        let executor = tasks::Executor::new(2);
        phase.run(&mut world, &executor);

        // Then: Both should complete around the same time (parallel)
        let times = system_times.lock().unwrap();
        assert_eq!(times.len(), 2);

        // Both should complete within ~100ms if parallel (not 100ms sequential)
        let max_time = times.iter().map(|(_, t)| t).max().unwrap();
        assert!(
            *max_time < 150,
            "Systems should run in parallel, took {}ms",
            max_time
        );
    }

    #[test]
    fn conflicting_access_runs_sequentially() {
        // Given: Systems with conflicting access must run sequentially
        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Position { x: 0.0, y: 0.0 });

        let execution_times = Arc::new(Mutex::new(Vec::new()));

        // System A: Write Position
        let times1 = Arc::clone(&execution_times);
        let system_a = move |_query: Query<&mut Position>| {
            let start = std::time::Instant::now();
            thread::sleep(Duration::from_millis(50));
            times1
                .lock()
                .unwrap()
                .push(("A", start.elapsed().as_millis()));
        };

        // System B: Read Position (conflicts with A)
        let times2 = Arc::clone(&execution_times);
        let system_b = move |_query: Query<&Position>| {
            let start = std::time::Instant::now();
            thread::sleep(Duration::from_millis(50));
            times2
                .lock()
                .unwrap()
                .push(("B", start.elapsed().as_millis()));
        };

        let mut phase = Phase::new();
        phase.add_system(system_a.into_system(&mut world));
        phase.add_system(system_b.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(2);
        let total_start = std::time::Instant::now();
        phase.run(&mut world, &executor);
        let total_time = total_start.elapsed().as_millis();

        // Then: Should take >100ms (sequential: 50ms + 50ms)
        assert!(
            total_time >= 100,
            "Conflicting systems should run sequentially, took {}ms",
            total_time
        );
    }

    #[test]
    fn mixed_parallel_and_sequential_workflow() {
        // Given: Complex workflow with multiple groups
        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 1.0 }));
        world.spawn((Position { x: 5.0, y: 5.0 }, Health { value: 100 }));

        let execution_log = Arc::new(Mutex::new(Vec::new()));

        // Group 1 (Parallel): These can run simultaneously
        let log1 = Arc::clone(&execution_log);
        let reader_position = move |_query: Query<&Position>| {
            log1.lock().unwrap().push("read_position");
            thread::sleep(Duration::from_millis(20));
        };

        let log2 = Arc::clone(&execution_log);
        let reader_health = move |_query: Query<&Health>| {
            log2.lock().unwrap().push("read_health");
            thread::sleep(Duration::from_millis(20));
        };

        // Group 2 (Sequential after Group 1): Writes Position
        let log3 = Arc::clone(&execution_log);
        let writer_position = move |_query: Query<&mut Position>| {
            log3.lock().unwrap().push("write_position");
            thread::sleep(Duration::from_millis(20));
        };

        let mut phase = Phase::new();
        phase.add_system(reader_position.into_system(&mut world));
        phase.add_system(reader_health.into_system(&mut world));
        phase.add_system(writer_position.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(2);
        let start = std::time::Instant::now();
        phase.run(&mut world, &executor);
        let elapsed = start.elapsed().as_millis();

        // Then: Should have all executed
        let log = execution_log.lock().unwrap();
        assert_eq!(log.len(), 3);
        assert!(log.contains(&"read_position"));
        assert!(log.contains(&"read_health"));
        assert!(log.contains(&"write_position"));

        // Should take ~40ms (20ms parallel + 20ms sequential), not 60ms (all sequential)
        assert!(
            elapsed < 55,
            "Mixed workflow should benefit from parallelism, took {}ms",
            elapsed
        );
    }

    #[test]
    fn physics_rendering_pipeline_simulation() {
        // Simulates a realistic game loop with physics and rendering phases
        let mut world = world::World::new(world::Id::new(0));
        world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 1.0 }));

        let execution_phases = Arc::new(Mutex::new(Vec::new()));

        // Physics: Apply forces (read pos, write vel) - can bundle with other force systems
        let phases1 = Arc::clone(&execution_phases);
        let apply_gravity = move |_query: Query<(&Position, &mut Velocity)>| {
            phases1.lock().unwrap().push("gravity");
        };

        let phases2 = Arc::clone(&execution_phases);
        let apply_wind = move |_query: Query<(&Position, &mut Velocity)>| {
            phases2.lock().unwrap().push("wind");
        };

        // Physics: Integration (read vel, write pos) - conflicts with forces
        let phases3 = Arc::clone(&execution_phases);
        let integrate = move |_query: Query<(&Velocity, &mut Position)>| {
            phases3.lock().unwrap().push("integrate");
        };

        // Rendering: Culling (read pos) - can run parallel with forces
        let phases4 = Arc::clone(&execution_phases);
        let culling = move |_query: Query<&Position>| {
            phases4.lock().unwrap().push("culling");
        };

        let mut phase = Phase::new();
        phase.add_system(apply_gravity.into_system(&mut world));
        phase.add_system(apply_wind.into_system(&mut world));
        phase.add_system(integrate.into_system(&mut world));
        phase.add_system(culling.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(4);
        phase.run(&mut world, &executor);

        // Then: All phases executed
        let phases = execution_phases.lock().unwrap();
        assert_eq!(phases.len(), 4);

        // Verify expected grouping: gravity+wind should bundle (same access)
        // Integration and culling in separate groups
    }

    #[test]
    fn many_parallel_systems() {
        // Given: Many systems with no conflicts
        let mut world = world::World::new(world::Id::new(0));
        let counter = Arc::new(AtomicU64::new(0));

        let mut phase = Phase::new();

        // Add 10 systems that don't conflict
        for _ in 0..10 {
            let counter_clone = Arc::clone(&counter);
            let system = move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            };
            phase.add_system(system.into_system(&mut world));
        }

        // When
        let executor = tasks::Executor::new(4);
        phase.run(&mut world, &executor);

        // Then: All systems executed
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn bundled_systems_share_resources() {
        // Given: Multiple systems with identical access should bundle
        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Position { x: 0.0, y: 0.0 });

        let executions = Arc::new(AtomicU64::new(0));

        // Create 5 systems that all read Position (should bundle)
        let mut phase = Phase::new();
        for _ in 0..5 {
            let exec_clone = Arc::clone(&executions);
            let system = move |_query: Query<&Position>| {
                exec_clone.fetch_add(1, Ordering::SeqCst);
            };
            phase.add_system(system.into_system(&mut world));
        }

        // When
        let executor = tasks::Executor::new(2);
        phase.run(&mut world, &executor);

        // Then: All 5 systems executed
        assert_eq!(executions.load(Ordering::SeqCst), 5);

        // Verify they're in one bundle (same access)
        assert_eq!(phase.plan.len(), 1, "Should have one group");
        assert_eq!(
            phase.plan[0].units().len(),
            1,
            "Should have one unit (bundled)"
        );
        assert_eq!(
            phase.plan[0].units()[0].system_indexes().len(),
            5,
            "Should have 5 systems in bundle"
        );
    }

    #[test]
    fn multiple_readers_same_component() {
        // Given: Multiple systems reading the same component
        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Position { x: 1.0, y: 2.0 });

        let reads = Arc::new(AtomicU64::new(0));

        let mut phase = Phase::new();
        for _ in 0..3 {
            let reads_clone = Arc::clone(&reads);
            let reader = move |_query: Query<&Position>| {
                reads_clone.fetch_add(1, Ordering::SeqCst);
            };
            phase.add_system(reader.into_system(&mut world));
        }

        // When
        let executor = tasks::Executor::new(3);
        phase.run(&mut world, &executor);

        // Then: All readers executed (they can run in parallel or bundled)
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn writer_serializes_with_readers() {
        // Given: One writer and readers of same component
        let mut world = world::World::new(world::Id::new(0));
        world.spawn(Position { x: 0.0, y: 0.0 });

        let operations = Arc::new(Mutex::new(Vec::new()));

        let ops1 = Arc::clone(&operations);
        let reader = move |_query: Query<&Position>| {
            ops1.lock().unwrap().push("read");
        };

        let ops2 = Arc::clone(&operations);
        let writer = move |_query: Query<&mut Position>| {
            ops2.lock().unwrap().push("write");
        };

        let mut phase = Phase::new();
        phase.add_system(reader.into_system(&mut world));
        phase.add_system(writer.into_system(&mut world));

        // When
        let executor = tasks::Executor::new(2);
        phase.run(&mut world, &executor);

        // Then: Both executed, but in separate groups
        let ops = operations.lock().unwrap();
        assert_eq!(ops.len(), 2);

        // Should be in different groups due to conflict
        assert_eq!(
            phase.plan.len(),
            2,
            "Reader and writer should be in separate groups"
        );
    }

    #[test]
    fn phase_label_provides_name() {
        assert_eq!(Update::name(), "Update");
        assert_eq!(FixedUpdate::name(), "FixedUpdate");
        assert_eq!(Render::name(), "Render");
    }

    #[test]
    fn different_phases_have_different_ids() {
        assert_ne!(Update.id(), FixedUpdate.id());
        assert_ne!(Update.id(), Render.id());
    }

    #[test]
    fn phase_sequence_new_is_empty() {
        let sequence = Sequence::new();
        assert!(sequence.is_empty());
        assert_eq!(sequence.len(), 0);
    }

    #[test]
    fn phase_sequence_then_adds_phases() {
        let sequence = Sequence::new().then(Update).then(FixedUpdate).then(Render);

        assert_eq!(sequence.len(), 3);
        assert!(!sequence.is_empty());
    }

    #[test]
    fn phase_sequence_push_adds_phases() {
        let mut sequence = Sequence::new();
        sequence.push(Update);
        sequence.push(Render);

        assert_eq!(sequence.len(), 2);
    }

    #[test]
    fn phase_sequence_clear_removes_all() {
        let mut sequence = Sequence::new().then(Update).then(Render);

        assert_eq!(sequence.len(), 2);
        sequence.clear();
        assert!(sequence.is_empty());
    }

    #[test]
    fn phase_sequence_with_capacity() {
        let sequence = Sequence::with_capacity(10);
        assert!(sequence.is_empty());
    }
}
