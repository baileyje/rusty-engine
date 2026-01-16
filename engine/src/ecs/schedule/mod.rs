//! Schedule management for organizing and executing ECS systems in phases.
//!
//! This module provides the [`Schedule`] container for organizing systems into named phases
//! and executing them with a thread pool. Phases are identified by marker types implementing
//! the [`phase::Label`] trait.
//!
//! # Architecture
//!
//! ```text
//! Schedule
//!   ├── Phase "FixedUpdate" ─► [physics_system, collision_system, ...]
//!   ├── Phase "Update"      ─► [ai_system, animation_system, ...]
//!   └── Phase "Render"      ─► [culling_system, draw_system, ...]
//! ```
//!
//! # Defining Phases
//!
//! Phases are identified by zero-sized marker types implementing [`phase::Label`].
//! Use the [`define_phase!`] macro for convenience:
//!
//! ```rust,ignore
//! use rusty_engine::define_phase;
//!
//! // Define custom phases for your game
//! define_phase!(FixedUpdate, Update, LateUpdate, Render);
//! ```
//!
//! Or implement manually for more control:
//!
//! ```rust,ignore
//! struct MyPhase;
//! impl Label for MyPhase {
//!     fn name() -> &'static str { "MyPhase" }
//! }
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use rusty_engine::define_phase;
//! use rusty_engine::ecs::schedule::Schedule;
//!
//! define_phase!(Update, Render);
//!
//! let mut schedule = Schedule::new();
//!
//! // Add systems to phases
//! schedule.add_system(Update, movement_system, &mut world);
//! schedule.add_system(Update, physics_system, &mut world);
//! schedule.add_system(Render, draw_system, &mut world);
//!
//! // Run phases in your game loop (order controlled by caller)
//! loop {
//!     schedule.run(Update, &mut world, &executor);
//!     schedule.run(Render, &mut world, &executor);
//! }
//! ```
//!
//! # Phase Execution Order
//!
//! The schedule does not enforce any ordering between phases. The caller controls
//! execution order by calling [`Schedule::run`] in the desired sequence. This provides
//! maximum flexibility for different game architectures.
//!
//! For reusable ordering, use [`Sequence`] to define an execution order once
//! and run it with [`Schedule::run_sequence`]:
//!
//! ```rust,ignore
//! let frame = Sequence::new()
//!     .then(FixedUpdate)
//!     .then(Update)
//!     .then(Render);
//!
//! // Game loop
//! loop {
//!     schedule.run_sequence(&frame, &mut world, &executor);
//! }
//! ```
//!
//! # Relationship to Phase Module
//!
//! Each phase internally uses the [`Phase`] type which handles:
//! - Parallel system scheduling via graph coloring
//! - Exclusive system pre-phase execution
//! - Resource grant management for safe parallel access
//!
//! See the [`phase`](crate::ecs::schedule::phase) module for execution details.

mod phase;
pub mod plan;

use std::collections::HashMap;

pub use phase::{Id, Label, Phase, Sequence};

use crate::{
    core::tasks,
    ecs::{system, world},
};

/// A container for organizing systems into labeled phases.
///
/// `Schedule` provides a simple way to group systems into named phases and execute
/// them with parallel scheduling. Each phase is identified by a marker type implementing
/// [`phase::Label`].
///
/// # Design Philosophy
///
/// The schedule is intentionally simple:
/// - **No implicit ordering**: Phases run when you call [`run`](Self::run)
/// - **No dependencies**: Systems within a phase are scheduled by access patterns, not explicit deps
/// - **No hierarchy**: Phases are flat, not nested
///
/// This simplicity makes the schedule predictable and easy to reason about. Complex
/// orchestration (phase ordering, conditional execution) belongs in the game loop or
/// a higher-level engine abstraction.
///
/// # Thread Safety
///
/// `Schedule` is `!Send` and `!Sync`. It should be owned and operated from a single
/// thread (typically the main thread), though the systems within phases execute in
/// parallel via the provided executor.
///
/// # Example
///
/// ```rust,ignore
/// define_phase!(FixedUpdate, Update, Render);
///
/// let mut schedule = Schedule::new();
/// let executor = Executor::new(4);
///
/// // Setup: add systems to phases
/// schedule.add_system(FixedUpdate, physics_system, &mut world);
/// schedule.add_system(Update, ai_system, &mut world);
/// schedule.add_system(Render, draw_system, &mut world);
///
/// // Game loop: run phases in order
/// loop {
///     // Fixed timestep physics
///     while physics_accumulator >= FIXED_TIMESTEP {
///         schedule.run(FixedUpdate, &mut world, &executor);
///         physics_accumulator -= FIXED_TIMESTEP;
///     }
///
///     // Variable timestep update
///     schedule.run(Update, &mut world, &executor);
///
///     // Render
///     schedule.run(Render, &mut world, &executor);
/// }
/// ```
#[derive(Default)]
pub struct Schedule {
    /// Phases indexed by their Id.
    phases: HashMap<Id, Phase>,
}

impl Schedule {
    /// Creates a new empty schedule.
    #[inline]
    pub fn new() -> Self {
        Self {
            phases: HashMap::new(),
        }
    }

    /// Adds a system to the specified phase.
    ///
    /// If the phase doesn't exist, it will be created. Systems within a phase are
    /// automatically scheduled for parallel execution based on their resource access
    /// patterns.
    ///
    /// # Parameters
    ///
    /// - `_: Label` - A phase label instance (the value is unused, only the type matters)
    /// - `system` - A function or closure that implements [`IntoSystem`](system::IntoSystem)
    /// - `world` - The world, needed to register the system's component access
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// define_phase!(Update);
    ///
    /// // Function systems
    /// fn gravity_system(query: Query<&mut Velocity>) { /* ... */ }
    /// schedule.add_system(Update, gravity_system, &mut world);
    ///
    /// // Closure systems
    /// let speed = 10.0;
    /// schedule.add_system(Update, move |q: Query<&mut Position>| {
    ///     // captures `speed`
    /// }, &mut world);
    /// ```
    pub fn add_system<L: Label, M>(
        &mut self,
        label: L,
        system: impl system::IntoSystem<M>,
        world: &mut world::World,
    ) {
        self.get_or_create_phase(label)
            .add_system(system.into_system(world));
    }

    /// Runs all systems in the specified phase.
    ///
    /// Systems are executed according to the phase's execution plan:
    /// 1. Exclusive systems (requiring `&mut World`) run sequentially first
    /// 2. Parallel systems run in groups based on resource access patterns
    ///
    /// # Returns
    ///
    /// Returns `true` if the phase existed and was executed, `false` if the phase
    /// was not found (no systems were ever added to it).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// define_phase!(Update, Render);
    ///
    /// // Returns true - phase exists
    /// assert!(schedule.run(Update, &mut world, &executor));
    ///
    /// // Returns false - phase was never created
    /// define_phase!(Cleanup);
    /// assert!(!schedule.run(Cleanup, &mut world, &executor));
    /// ```
    pub fn run<L: Label>(
        &mut self,
        label: L,
        world: &mut world::World,
        executor: &tasks::Executor,
    ) -> bool {
        if let Some(phase) = self.phases.get_mut(&label.id()) {
            phase.run(world, executor);
            true
        } else {
            false
        }
    }

    /// Returns `true` if the specified phase exists in the schedule.
    ///
    /// A phase exists if at least one system has been added to it, or if it was
    /// explicitly created via [`get_or_create_phase`](Self::get_or_create_phase).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// define_phase!(Update, Render);
    ///
    /// assert!(!schedule.has_phase(Update));
    /// schedule.add_system(Update, some_system, &mut world);
    /// assert!(schedule.has_phase(Update));
    /// ```
    #[inline]
    pub fn has_phase<L: Label>(&self, lable: L) -> bool {
        self.phases.contains_key(&lable.id())
    }

    /// Returns a reference to the specified phase, if it exists.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(phase) = schedule.get_phase(Update) {
    ///     println!("Update phase has {} systems", phase.systems_len());
    /// }
    /// ```
    #[inline]
    pub fn get_phase<L: Label>(&self, label: L) -> Option<&Phase> {
        self.phases.get(&label.id())
    }

    /// Returns a mutable reference to the specified phase, creating it if it doesn't exist.
    ///
    /// This is useful when you need to configure a phase before adding systems,
    /// such as setting a custom planner.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Create phase with custom planner
    /// let phase = schedule.get_or_create_phase::<Update>();
    /// // phase.set_planner(custom_planner); // if such API existed
    /// ```
    fn get_or_create_phase<L: Label>(&mut self, label: L) -> &mut Phase {
        self.phases.entry(label.id()).or_default()
    }

    /// Returns the number of phases in the schedule.
    #[inline]
    pub fn phase_count(&self) -> usize {
        self.phases.len()
    }

    /// Runs a sequence of phases in order.
    ///
    /// This is a convenience method for running multiple phases in a specific order.
    /// Each phase is run sequentially, with the next phase starting only after the
    /// previous one completes.
    ///
    /// # Returns
    ///
    /// The number of phases that existed and were executed. Phases that don't exist
    /// in the schedule are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// define_phase!(FixedUpdate, Update, Render);
    ///
    /// // Build a reusable sequence
    /// let frame_sequence = Sequence::new()
    ///     .then(FixedUpdate)
    ///     .then(Update)
    ///     .then(Render);
    ///
    /// // Run all phases in order
    /// let phases_run = schedule.run_sequence(&frame_sequence, &mut world, &executor);
    /// assert_eq!(phases_run, 3);
    /// ```
    pub fn run_sequence(
        &mut self,
        sequence: &Sequence,
        world: &mut world::World,
        executor: &tasks::Executor,
    ) -> usize {
        let mut count = 0;
        for type_id in sequence.phases() {
            if let Some(phase) = self.phases.get_mut(type_id) {
                phase.run(world, executor);
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    use rusty_macros::{Component, Unique};

    use crate::{
        core::tasks,
        define_phase,
        ecs::{
            system::param::{Query, UniqMut},
            world,
        },
    };

    use super::*;

    // Test phases defined using the macro
    define_phase!(Update, FixedUpdate, Render);

    #[test]
    fn new_schedule_is_empty() {
        let schedule = Schedule::new();
        assert_eq!(schedule.phase_count(), 0);
    }

    #[test]
    fn default_schedule_is_empty() {
        let schedule = Schedule::default();
        assert_eq!(schedule.phase_count(), 0);
    }

    #[test]
    fn add_system_creates_phase() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();

        fn test_system() {}

        assert!(!schedule.has_phase(Update));
        schedule.add_system(Update, test_system, &mut world);
        assert!(schedule.has_phase(Update));
        assert_eq!(schedule.phase_count(), 1);
    }

    #[test]
    fn add_systems_to_multiple_phases() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();

        fn system_1() {}
        fn system_2() {}
        fn system_3() {}

        schedule.add_system(Update, system_1, &mut world);
        schedule.add_system(Update, system_2, &mut world);
        schedule.add_system(FixedUpdate, system_1, &mut world);
        schedule.add_system(FixedUpdate, system_3, &mut world);

        assert_eq!(schedule.phase_count(), 2);
        assert_eq!(schedule.get_phase(Update).unwrap().systems_len(), 2);
        assert_eq!(schedule.get_phase(FixedUpdate).unwrap().systems_len(), 2);
    }

    #[test]
    fn get_phase_returns_none_for_missing() {
        let schedule = Schedule::new();
        assert!(schedule.get_phase(Update).is_none());
    }

    #[test]
    fn get_or_create_phase_creates_empty_phase() {
        let mut schedule = Schedule::new();

        assert!(!schedule.has_phase(Update));
        let phase = schedule.get_or_create_phase(Update);
        assert_eq!(phase.systems_len(), 0);
        assert!(schedule.has_phase(Update));
    }

    #[test]
    fn run_returns_false_for_missing_phase() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        assert!(!schedule.run(Update, &mut world, &executor));
    }

    #[test]
    fn run_returns_true_for_existing_phase() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        fn test_system() {}
        schedule.add_system(Update, test_system, &mut world);

        assert!(schedule.run(Update, &mut world, &executor));
    }

    #[test]
    fn run_executes_systems() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let system = move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        };

        schedule.add_system(Update, system, &mut world);
        schedule.run(Update, &mut world, &executor);

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_only_executes_specified_phase() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let update_counter = Arc::new(AtomicU32::new(0));
        let fixed_counter = Arc::new(AtomicU32::new(0));

        let update_clone = Arc::clone(&update_counter);
        let fixed_clone = Arc::clone(&fixed_counter);

        schedule.add_system(
            Update,
            move || {
                update_clone.fetch_add(1, Ordering::SeqCst);
            },
            &mut world,
        );
        schedule.add_system(
            FixedUpdate,
            move || {
                fixed_clone.fetch_add(1, Ordering::SeqCst);
            },
            &mut world,
        );

        // Only run Update
        schedule.run(Update, &mut world, &executor);

        assert_eq!(update_counter.load(Ordering::SeqCst), 1);
        assert_eq!(fixed_counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn phases_execute_in_caller_specified_order() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let order1 = Arc::clone(&order);
        let order2 = Arc::clone(&order);
        let order3 = Arc::clone(&order);

        schedule.add_system(
            Update,
            move || {
                order1.lock().unwrap().push("Update");
            },
            &mut world,
        );
        schedule.add_system(
            FixedUpdate,
            move || {
                order2.lock().unwrap().push("FixedUpdate");
            },
            &mut world,
        );
        schedule.add_system(
            Render,
            move || {
                order3.lock().unwrap().push("Render");
            },
            &mut world,
        );

        // Run in specific order
        schedule.run(FixedUpdate, &mut world, &executor);
        schedule.run(Update, &mut world, &executor);
        schedule.run(Render, &mut world, &executor);

        let recorded = order.lock().unwrap();
        assert_eq!(*recorded, vec!["FixedUpdate", "Update", "Render"]);
    }

    #[test]
    fn systems_can_modify_components() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(2);

        #[derive(Component)]
        struct Counter {
            value: i32,
        }

        fn increment(counters: system::param::Query<&mut Counter>) {
            for counter in counters {
                counter.value += 1;
            }
        }

        schedule.add_system(Update, increment, &mut world);

        world.spawn(Counter { value: 0 });
        world.spawn(Counter { value: 10 });

        schedule.run(Update, &mut world, &executor);
        schedule.run(Update, &mut world, &executor);
        schedule.run(Update, &mut world, &executor);

        let values: Vec<i32> = world.query::<&Counter>().map(|c| c.value).collect();
        assert!(values.contains(&3)); // 0 + 3
        assert!(values.contains(&13)); // 10 + 3
    }

    #[test]
    fn complex_multi_phase_workflow() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(2);

        #[derive(Component)]
        struct Item {
            value: i32,
        }

        #[derive(Unique)]
        struct Total {
            num: i32,
        }

        // FixedUpdate: Increment items
        fn increment_items(items: Query<&mut Item>) {
            for item in items {
                item.value += 5;
            }
        }

        // Update: Sum items into total
        fn sum_items(items: Query<&Item>, mut total: UniqMut<Total>) {
            let sum: i32 = items.map(|i| i.value).sum();
            total.num = sum;
        }

        schedule.add_system(FixedUpdate, increment_items, &mut world);
        schedule.add_system(Update, sum_items, &mut world);

        world.spawn(Item { value: 0 });
        world.spawn(Item { value: 0 });
        world.spawn(Item { value: 0 });

        world.add_unique(Total { num: 0 });

        // Simulate game loop: fixed update runs twice, then update
        schedule.run(FixedUpdate, &mut world, &executor);
        schedule.run(FixedUpdate, &mut world, &executor);
        schedule.run(Update, &mut world, &executor);

        // Each item: 0 + 5 + 5 = 10, three items = 30
        let total = world.get_unique::<Total>().unwrap();
        assert_eq!(total.num, 30);
    }

    #[test]
    fn empty_phase_runs_successfully() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        // Create empty phase
        schedule.get_or_create_phase(Update);

        // Should return true (phase exists) even with no systems
        assert!(schedule.run(Update, &mut world, &executor));
    }

    #[test]
    fn run_sequence_executes_in_order() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let order1 = Arc::clone(&order);
        let order2 = Arc::clone(&order);
        let order3 = Arc::clone(&order);

        schedule.add_system(
            Update,
            move || {
                order1.lock().unwrap().push("Update");
            },
            &mut world,
        );
        schedule.add_system(
            FixedUpdate,
            move || {
                order2.lock().unwrap().push("FixedUpdate");
            },
            &mut world,
        );
        schedule.add_system(
            Render,
            move || {
                order3.lock().unwrap().push("Render");
            },
            &mut world,
        );

        // Define sequence: FixedUpdate -> Update -> Render
        let sequence = Sequence::new().then(FixedUpdate).then(Update).then(Render);

        // Run sequence
        let count = schedule.run_sequence(&sequence, &mut world, &executor);

        assert_eq!(count, 3);
        let recorded = order.lock().unwrap();
        assert_eq!(*recorded, vec!["FixedUpdate", "Update", "Render"]);
    }

    #[test]
    fn run_sequence_skips_missing_phases() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        // Only add Update phase
        schedule.add_system(
            Update,
            move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            },
            &mut world,
        );

        // Sequence includes phases that don't exist
        let sequence = Sequence::new()
            .then(FixedUpdate) // doesn't exist
            .then(Update) // exists
            .then(Render); // doesn't exist

        let count = schedule.run_sequence(&sequence, &mut world, &executor);

        // Only Update ran
        assert_eq!(count, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_sequence_empty_returns_zero() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        fn test_system() {}
        schedule.add_system(Update, test_system, &mut world);

        let empty_sequence = Sequence::new();
        let count = schedule.run_sequence(&empty_sequence, &mut world, &executor);

        assert_eq!(count, 0);
    }

    #[test]
    fn run_sequence_can_be_reused() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        schedule.add_system(
            Update,
            move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            },
            &mut world,
        );

        let sequence = Sequence::new().then(Update);

        // Run same sequence multiple times
        schedule.run_sequence(&sequence, &mut world, &executor);
        schedule.run_sequence(&sequence, &mut world, &executor);
        schedule.run_sequence(&sequence, &mut world, &executor);

        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn multiple_sequences_for_different_scenarios() {
        let mut world = world::World::new(world::Id::new(0));
        let mut schedule = Schedule::new();
        let executor = tasks::Executor::new(1);

        let log = Arc::new(std::sync::Mutex::new(Vec::new()));

        let log1 = Arc::clone(&log);
        let log2 = Arc::clone(&log);
        let log3 = Arc::clone(&log);

        schedule.add_system(
            Update,
            move || {
                log1.lock().unwrap().push("Update");
            },
            &mut world,
        );
        schedule.add_system(
            FixedUpdate,
            move || {
                log2.lock().unwrap().push("FixedUpdate");
            },
            &mut world,
        );
        schedule.add_system(
            Render,
            move || {
                log3.lock().unwrap().push("Render");
            },
            &mut world,
        );

        // Different sequences for different scenarios
        let normal_frame = Sequence::new().then(FixedUpdate).then(Update).then(Render);

        let paused_frame = Sequence::new().then(Render); // Only render when paused

        // Normal frame
        schedule.run_sequence(&normal_frame, &mut world, &executor);

        // Paused frame
        schedule.run_sequence(&paused_frame, &mut world, &executor);

        let recorded = log.lock().unwrap();
        assert_eq!(*recorded, vec!["FixedUpdate", "Update", "Render", "Render"]);
    }
}
