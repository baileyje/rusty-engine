use std::marker::PhantomData;

use crate::ecs::world;

/// Trait for planning the execution of systems in an ECS schedule phase.
///
/// The input to the planner is a list of systems with their world resource access requirements.
///
/// The output is an optimized execution plan that defines the order and grouping of systems
pub trait Planner {
    fn plan(&self, tasks: &[Task]) -> Vec<Group>;
}

/// A task represents a single system to be scheduled for execution.
///
/// Each task consists of a system index (within its phase) and its world resource access requirements.
/// The planner uses these requirements to determine which tasks can run in parallel
/// without violating Rust's aliasing rules.
#[derive(Debug, Clone)]
pub struct Task {
    /// The system id for this task.
    system_index: usize,

    /// The required access to run this system.
    required_access: world::AccessRequest,
}

impl Task {
    /// Create a new Task with the given system index and required access.
    pub fn new(system_index: usize, required_access: world::AccessRequest) -> Self {
        Self {
            system_index,
            required_access,
        }
    }
}

/// A unit of work in the execution plan containing one or more tasks with identical resource requirements.
///
/// # Purpose
///
/// Units bundle tasks that require exactly the same world resource access. This optimization provides:
/// - **Reduced synchronization overhead**: Resources are acquired once for the entire unit
/// - **Improved cache locality**: Sequential execution keeps data hot in CPU caches
/// - **Fewer borrow operations**: Single borrow covers all tasks in the unit
///
/// # Execution Model
///
/// Tasks within a unit execute **sequentially** with shared resource access. The executor will:
/// 1. Acquire the required resources from the world once
/// 2. Execute each task in order with the same resources
/// 3. Release resources after all tasks complete
///
/// # Example
///
/// ```rust,ignore
/// // Three physics systems all need (read Position, write Velocity)
/// // They get bundled into one unit:
/// let unit = Unit {
///     system_ids: vec![apply_gravity, apply_wind, apply_friction],
///     required_access: AccessRequest::to_components(pos_read, vel_write)
/// };
///
/// // At runtime, this unit acquires resources once:
/// let (pos, vel) = world.borrow_resources(&unit.required_access);
/// for system_id in unit.system_ids {
///     execute_system(system_id, pos, vel);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Unit {
    /// The system index for this unit.
    system_indexes: Vec<usize>,

    /// The required access to run all systems in this unit.
    required_access: world::AccessRequest,
}

impl Unit {
    /// Create a new Unit with the given system ids and required access.
    #[inline]
    pub const fn new(system_indexes: Vec<usize>, required_access: world::AccessRequest) -> Self {
        Self {
            system_indexes,
            required_access,
        }
    }

    /// Create a unit that is a single task.
    #[inline]
    pub fn single(task: Task) -> Self {
        Self::new(vec![task.system_index], task.required_access)
    }

    /// Add a task to this unit.
    ///
    /// # Note
    ///
    /// The caller must ensure the task has identical resource requirements to existing tasks.
    #[inline]
    pub fn add_task(&mut self, task: Task) {
        self.system_indexes.push(task.system_index);
    }

    /// Returns `true` if this unit requires mutable world access.
    ///
    /// Units requiring mutable world access cannot run in parallel with any other units
    /// and are typically prioritized to run first in the schedule.
    #[inline]
    pub fn is_world_mut(&self) -> bool {
        self.required_access.world_mut()
    }

    /// Get the system ids in this unit.
    #[inline]
    pub fn system_indexes(&self) -> &[usize] {
        self.system_indexes.as_slice()
    }

    /// Get the required access for this unit.
    #[inline]
    pub fn required_access(&self) -> &world::AccessRequest {
        &self.required_access
    }
}

/// Converts a list of tasks into units by bundling tasks with identical resource requirements.
///
/// This is the first phase of the scheduling algorithm. Tasks that require exactly the same
/// world resource access are grouped into units, allowing them to share a single resource
/// acquisition and maintain cache locality during execution.
///
/// # Algorithm
///
/// 1. Create a unit for the first task
/// 2. For each remaining task:
///    - If a unit with matching access exists, add task to that unit
///    - Otherwise, create a new unit for the task
///
/// # Performance
///
/// Time: O(n × u) where n = tasks, u = unique access patterns
/// Space: O(n)
///
/// In typical ECS workloads, u << n (many systems share access patterns), making this
/// effectively O(n) in practice.
///
/// # Example
///
/// ```rust,ignore
/// let tasks = vec![
///     Task::new(sys1, read_position),      // Group A
///     Task::new(sys2, read_position),      // Group A (same access)
///     Task::new(sys3, write_velocity),     // Group B (different access)
/// ];
///
/// let units = unitize(&tasks);
/// assert_eq!(units.len(), 2);  // Two distinct access patterns
/// assert_eq!(units[0].system_ids.len(), 2);  // First unit has 2 tasks
/// ```
pub fn unitize(tasks: &[Task]) -> Vec<Unit> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let mut units = Vec::new();
    // Create initial unit from first task
    units.push(Unit::single(tasks[0].clone()));

    // Bundle remaining tasks with matching units
    for task in tasks.iter().skip(1) {
        // Ensure no exclusive tasks get into the parallel units execution plan.
        debug_assert!(
            !task.required_access.world_mut(),
            "World mutable tasks should not be bundled into units."
        );

        let mut found_unit = false;
        for unit in units.iter_mut() {
            // If we have the exact same requirements, we can add to this unit
            if unit.required_access == task.required_access {
                unit.add_task(task.clone());
                found_unit = true;
                break;
            }
        }

        if !found_unit {
            // Create a new unit for this task
            units.push(Unit::single(task.clone()))
        }
    }

    units
}

/// Prioritizes units based on their world access requirements and scheduling difficulty.
///
/// This ordering heuristic improves the greedy graph coloring algorithm by processing
/// "harder to schedule" units first, typically resulting in fewer parallel groups.
///
/// # Priority Order (highest to lowest)
///
/// 1. **Mutable world access** - Cannot run with anything else, highest priority
/// 2. **Immutable world access** - Conflicts with any mutable access
/// 3. **Component access by difficulty**:
///    - Mutable component access scores higher (conflicts with both reads and writes)
///    - More components = higher score (more potential conflicts)
///    - Scoring: mutable_count × 2 + immutable_count
///
/// # Rationale
///
/// Scheduling constrained units first gives the greedy algorithm more options for placing
/// less-constrained units later. Units requiring exclusive world access will naturally
/// form their own groups and execute first.
///
/// # Example
///
/// ```rust,ignore
/// let mut units = vec![
///     Unit::new([sys1], AccessRequest::NONE),                    // Priority: 0
///     Unit::new([sys2], read_position),                          // Priority: 1  
///     Unit::new([sys3], write_velocity),                         // Priority: 2
///     Unit::new([sys4], read_pos_write_vel),                     // Priority: 3
///     Unit::new([sys5], AccessRequest::to_world(false)),         // Priority: MAX-1
/// ];
///
/// prioritize(&mut units);
/// // Result: [sys6, sys5, sys4, sys3, sys2, sys1]
/// ```
pub fn prioritize(units: &mut [Unit]) {
    fn difficulty(unit: &Unit) -> u32 {
        // Immutable world access conflicts with all mutable component access
        if unit.required_access.world() {
            return u32::MAX;
        }

        // Component-level access: score by potential conflicts
        // Mutable access conflicts with both reads and writes, so weight it higher
        let mut score = 0;
        score += unit.required_access.resources_len() as u32; // Immutable: 1 point each
        score += unit.required_access.resources_mut_len() as u32 * 2; // Mutable: 2 points each
        score
    }

    // Sort in descending order (highest difficulty first)
    units.sort_by_key(|u| std::cmp::Reverse(difficulty(u)));
}

/// A group of units that can execute in parallel without resource conflicts.
///
/// # Execution Model
///
/// Groups represent a "wave" of parallelism in the schedule. All units within a group:
/// - Have **no resource conflicts** with each other (verified by conflict detection)
/// - Can execute **simultaneously** on different threads
/// - Must **complete** before the next group begins
///
/// # Structure
///
/// - **Within a unit**:
///     - Tasks with mutable access execute sequentially with shared resource access
///     - Tasks with immutable access can potentially execute in parallel within the unit
/// - **Within a group**: Units execute in parallel (on separate threads)
/// - **Across groups**: Groups execute sequentially (barriers between groups)
///
/// # Example Execution
///
/// ```text
/// Group 1 (parallel):                Group 2 (parallel):
///   ┌─────────────┐                    ┌─────────────┐
///   │ Unit A      │                    │ Unit D      │
///   │  - System 1 │ ──┐                │  - System 7 │
///   │  - System 2 │   │                └─────────────┘
///   └─────────────┘   │ Barrier
///   ┌─────────────┐   │                ┌─────────────┐
///   │ Unit B      │   │                │ Unit E      │
///   │  - System 3 │ ──┘                │  - System 8 │
///   └─────────────┘                    │  - System 9 │
///   ┌─────────────┐                    └─────────────┘
///   │ Unit C      │
///   │  - System 4 │
///   │  - System 5 │
///   │  - System 6 │
///   └─────────────┘
/// ```
///
/// # Relationship to Units
///
/// Groups contain units (not individual tasks) because:
/// - Units handle sequential execution with resource sharing
/// - Groups handle parallel execution with conflict-free resources
/// - This two-level structure optimizes both cache locality (units) and parallelism (groups)
///
/// # Special Cases
///
/// - **Empty group**: Not allowed (should not be created by planner)
/// - **Single-unit group**: Valid - represents sequential bottleneck
/// - **Units with `AccessRequest::NONE`**: Can be placed in any group (no conflicts)
#[derive(Debug, Clone)]
pub struct Group {
    units: Vec<Unit>,
}

impl Group {
    /// Create a new Group with the given units.
    #[inline]
    pub const fn new(units: Vec<Unit>) -> Self {
        Self { units }
    }

    /// Create a group that is a single unit.
    #[inline]
    pub fn single(unit: Unit) -> Self {
        Self::new(vec![unit])
    }

    /// Push a unit to this group.
    #[inline]
    pub fn push(&mut self, unit: Unit) {
        self.units.push(unit);
    }

    /// Get the units in this group.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }
}

/// A simple sequential planner that creates one group per unit.
///
/// This planner provides no parallelism but still benefits from unit bundling:
/// - Tasks with identical access are bundled into units
/// - Each unit becomes its own sequential group
/// - Resource acquisition overhead is reduced via bundling
///
/// # Use Cases
///
/// - Single-threaded platforms
/// - Debugging (deterministic execution order)
/// - Baseline for benchmarking parallel schedulers
/// - Systems that cannot be parallelized safely
///
/// # Performance
///
/// While this planner doesn't enable parallel execution, unit bundling still provides:
/// - Reduced borrow/release cycles
/// - Better cache locality within units
/// - Simpler execution model
pub struct SequentialPlanner;

impl Planner for SequentialPlanner {
    fn plan(&self, tasks: &[Task]) -> Vec<Group> {
        // Step 1: Bundle tasks with identical access into units
        let mut units = unitize(tasks);

        // Step 2: Prioritize units (exclusive world access first)
        prioritize(&mut units);

        // Step 3: Create one group per unit (no parallelism)
        units.into_iter().map(Group::single).collect()
    }
}

/// A graph coloring planner that uses greedy algorithms to maximize parallelism.
///
/// This planner treats scheduling as a graph coloring problem:
/// - **Nodes**: Units (bundles of tasks with identical access)
/// - **Edges**: Resource conflicts between units
/// - **Colors**: Groups (parallel execution waves)
/// - **Goal**: Minimize number of colors (maximize parallelism)
///
/// The generic parameter `Algo` selects the coloring algorithm heuristic.
pub struct GraphColorPlanner<Algo>(PhantomData<Algo>);

/// Welsh-Powell algorithm: prioritizes units by difficulty, then greedily assigns to groups.
///
/// This is a simple, fast greedy algorithm that typically produces near-optimal results.
/// It works well for ECS scheduling where conflict patterns tend to be localized.
pub struct WelshPowell;

/// DSATUR (Degree of Saturation) algorithm: dynamically selects the most constrained unit next.
///
/// This algorithm typically produces 10-30% fewer groups than Welsh-Powell but has higher
/// overhead. Not yet implemented.
#[allow(dead_code)]
struct Dsatur;

impl GraphColorPlanner<WelshPowell> {
    /// Constant instance of the Welsh-Powell planner.
    pub const WELSH_POWELL: GraphColorPlanner<WelshPowell> = GraphColorPlanner(PhantomData);
}

/// Implementation of the greedy graph coloring planner using the Welsh-Powell algorithm.
///
/// # Algorithm
///
/// 1. **Unitize**: Bundle tasks with identical access
/// 2. **Prioritize**: Sort units by difficulty (constrained units first)
/// 3. **Greedy coloring**: For each unit, place in first compatible group
///
/// # Complexity
///
/// - Time: O(n log n + u² × g) where n=tasks, u=units, g=groups
/// - Space: O(n + g)
///
/// In practice, u << n (bundling) and g is small (parallelism limited by hardware),
/// making this very efficient.
///
/// # Parallelism Quality
///
/// Welsh-Powell typically achieves 70-90% of optimal parallelism with O(n²) complexity,
/// which is excellent for typical ECS workloads (10-100 systems).
impl Planner for GraphColorPlanner<WelshPowell> {
    fn plan(&self, tasks: &[Task]) -> Vec<Group> {
        // Step 1: Bundle tasks with identical access into units
        let mut units = unitize(tasks);

        // Step 2: Prioritize units (constrained units first improves greedy algorithm)
        prioritize(&mut units);

        // Step 3: Greedy graph coloring - assign each unit to first compatible group
        let mut groups: Vec<Group> = Vec::new();
        for unit in units.into_iter() {
            // Find the first group where this unit doesn't conflict with any existing unit
            let group = groups.iter_mut().find(|group| {
                // Unit can join if it doesn't conflict with ANY unit already in the group
                !group
                    .units
                    .iter()
                    .any(|other| unit.required_access.conflicts_with(&other.required_access))
            });

            match group {
                Some(group) => {
                    // Compatible group found - add unit to it
                    group.units.push(unit);
                }
                None => {
                    // No compatible group - create new group for this unit
                    groups.push(Group::single(unit));
                }
            }
        }

        groups
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::ecs::world;

    fn create_tasks() -> Vec<Task> {
        vec![
            Task::new(1, world::AccessRequest::NONE),
            Task::new(2, world::AccessRequest::NONE),
            Task::new(
                3,
                world::AccessRequest::to_resources(
                    &[world::TypeId::new(1), world::TypeId::new(2)],
                    &[],
                ),
            ),
            Task::new(
                4,
                world::AccessRequest::to_resources(
                    &[world::TypeId::new(1), world::TypeId::new(2)],
                    &[],
                ),
            ),
            Task::new(
                5,
                world::AccessRequest::to_resources(
                    &[],
                    &[world::TypeId::new(1), world::TypeId::new(2)],
                ),
            ),
            Task::new(6, world::AccessRequest::to_world(false)),
            Task::new(
                7,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)]),
            ),
        ]
    }

    #[test]
    fn unitize_simple() {
        // Given

        let tasks = create_tasks();

        // When
        let units = unitize(&tasks);

        // Then
        assert_eq!(units.len(), 5);
        assert_eq!(units[0].system_indexes, vec![1, 2]);
        assert_eq!(units[0].required_access, world::AccessRequest::NONE);
        assert_eq!(units[1].system_indexes, vec![3, 4]);
        assert_eq!(
            units[1].required_access,
            world::AccessRequest::to_resources(
                &[world::TypeId::new(1), world::TypeId::new(2)],
                &[],
            ),
        );

        assert_eq!(units[3].system_indexes, vec![6]);
        assert_eq!(
            units[3].required_access,
            world::AccessRequest::to_world(false)
        );

        assert_eq!(units[4].system_indexes, vec![7]);
        assert_eq!(
            units[4].required_access,
            world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)],),
        );
    }

    #[test]
    fn prioritize_simple() {
        // Given
        let tasks = create_tasks();
        let mut units = unitize(&tasks);

        // When
        prioritize(&mut units);

        // Then
        assert_eq!(units.len(), 5); // Sanity check we didn't lose any units.
        assert_eq!(
            units[0].required_access,
            world::AccessRequest::to_world(false)
        );
        assert_eq!(
            units[1].required_access,
            world::AccessRequest::to_resources(
                &[],
                &[world::TypeId::new(1), world::TypeId::new(2)],
            )
        );
        assert_eq!(
            units[2].required_access,
            world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)])
        );
        assert_eq!(
            units[3].required_access,
            world::AccessRequest::to_resources(
                &[world::TypeId::new(1), world::TypeId::new(2)],
                &[],
            )
        );
        assert_eq!(units[4].required_access, world::AccessRequest::NONE);
    }

    #[test]
    fn greedy_graph_color_planner_wp() {
        // Given
        let tasks = create_tasks();
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then
        assert_eq!(groups.len(), 2);
        let mut groups = groups.into_iter();

        // Group 1: Immutable world access + immutable component access (no conflicts). None
        let group = groups.next().unwrap();
        assert_eq!(group.units.len(), 3);
        assert_eq!(
            group.units[0].required_access,
            world::AccessRequest::to_world(false)
        );
        assert_eq!(
            group.units[1].required_access,
            world::AccessRequest::to_resources(
                &[world::TypeId::new(1), world::TypeId::new(2)],
                &[],
            )
        );
        assert_eq!(group.units[2].required_access, world::AccessRequest::NONE);

        // Group 3: Mutable component access (conflicts with Group 2's reads)
        let group = groups.next().unwrap();
        assert_eq!(group.units.len(), 2);
        assert_eq!(
            group.units[0].required_access,
            world::AccessRequest::to_resources(
                &[],
                &[world::TypeId::new(1), world::TypeId::new(2)],
            )
        );
        assert_eq!(
            group.units[1].required_access,
            world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)],)
        );
    }

    // ==================== Edge Cases ====================

    #[test]
    fn unitize_empty_tasks() {
        // Given: Empty task list
        let tasks: Vec<Task> = vec![];

        // When
        let units = unitize(&tasks);

        // Then: Should return empty without panic
        assert_eq!(units.len(), 0);
    }

    #[test]
    fn unitize_single_task() {
        // Given: Single task
        let tasks = vec![Task::new(1, world::AccessRequest::to_world(true))];

        // When
        let units = unitize(&tasks);

        // Then: Single unit
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].system_indexes.len(), 1);
    }

    #[test]
    fn prioritize_empty_units() {
        // Given: Empty units
        let mut units: Vec<Unit> = vec![];

        // When
        prioritize(&mut units);

        // Then: Should not panic
        assert_eq!(units.len(), 0);
    }

    #[test]
    fn sequential_planner_empty_tasks() {
        // Given
        let planner = SequentialPlanner;
        let tasks: Vec<Task> = vec![];

        // When
        let groups = planner.plan(&tasks);

        // Then
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn graph_color_planner_empty_tasks() {
        // Given
        let planner = GraphColorPlanner::WELSH_POWELL;
        let tasks: Vec<Task> = vec![];

        // When
        let groups = planner.plan(&tasks);

        // Then
        assert_eq!(groups.len(), 0);
    }

    // ==================== Conflict Detection Tests ====================

    #[test]
    fn multiple_readers_same_component_parallel() {
        // Given: Multiple systems reading the same component
        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[]),
            ),
            Task::new(
                2,
                world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[]),
            ),
            Task::new(
                3,
                world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: All readers should be in one unit in one group (no conflicts)
        assert_eq!(groups.len(), 1, "Readers should run in parallel");
        assert_eq!(groups[0].units.len(), 1, "Should bundle into one unit");
        assert_eq!(
            groups[0].units[0].system_indexes.len(),
            3,
            "All 3 systems in unit"
        );
    }

    #[test]
    fn writer_conflicts_with_readers() {
        // Given: One writer and multiple readers of same component
        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[]),
            ),
            Task::new(
                2,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: Must be in separate groups
        assert_eq!(groups.len(), 2, "Reader and writer must be sequential");
    }

    #[test]
    fn multiple_writers_same_component_sequential() {
        // Given: Multiple systems writing the same component
        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
            Task::new(
                2,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
            Task::new(
                3,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: All writers bundled together (same access), but in separate groups from readers
        assert_eq!(groups.len(), 1, "Same access = one unit = one group");
        assert_eq!(groups[0].units.len(), 1, "Should bundle into one unit");
        assert_eq!(
            groups[0].units[0].system_indexes.len(),
            3,
            "All 3 systems bundled"
        );
    }

    #[test]
    fn disjoint_components_parallel() {
        // Given: Systems accessing completely different components
        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
            Task::new(
                2,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(2)]),
            ),
            Task::new(
                3,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: All can run in parallel (no conflicts)
        assert_eq!(groups.len(), 1, "Different components = no conflicts");
        assert_eq!(groups[0].units.len(), 3, "Three separate units");
    }

    // ==================== Complex Workload Tests ====================

    #[test]
    fn physics_pipeline_simulation() {
        // Simulates a typical physics pipeline:
        // 1. Multiple force applications (read position, write velocity)
        // 2. Integration (read velocity, write position)
        // 3. Collision detection (read position)

        let pos_spec = &[world::TypeId::new(1)]; // Position
        let vel_spec = &[world::TypeId::new(2)]; // Velocity

        let tasks = vec![
            // Force applications: read pos, write vel (should bundle)
            Task::new(1, world::AccessRequest::to_resources(pos_spec, vel_spec)),
            Task::new(2, world::AccessRequest::to_resources(pos_spec, vel_spec)),
            Task::new(3, world::AccessRequest::to_resources(pos_spec, vel_spec)),
            // Integration: read vel, write pos (conflicts with force apps)
            Task::new(4, world::AccessRequest::to_resources(vel_spec, pos_spec)),
            // Collision detection: read pos only (can run with force apps, not integration)
            Task::new(5, world::AccessRequest::to_resources(pos_spec, &[])),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: Expect 2 groups
        // Group 1: Force applications (bundled) + collision detection (reads position)
        // Group 2: Integration (writes position, conflicts with both)
        assert_eq!(groups.len(), 2, "Physics pipeline should have 2 phases");

        // First group should have force bundle + collision
        assert_eq!(groups[0].units.len(), 2, "Force bundle + collision unit");

        // One of the units should have 3 systems (the bundled force applications)
        let force_unit = groups[0].units.iter().find(|u| u.system_indexes.len() == 3);
        assert!(force_unit.is_some(), "Force applications should bundle");
    }

    #[test]
    fn rendering_pipeline_simulation() {
        // Simulates a rendering pipeline:
        // 1. Frustum culling (read position, read camera)
        // 2. LOD selection (read position, read camera)
        // 3. Animation update (write transform)
        // 4. Render preparation (read transform, read material)

        let pos_spec = world::TypeId::new(1); // Position
        let camera_spec = world::TypeId::new(2); // Camera
        let transform_spec = world::TypeId::new(3); // Transform
        let material_spec = world::TypeId::new(4); // Material

        let tasks = vec![
            // Culling: read position + camera (should bundle with LOD)
            Task::new(
                1,
                world::AccessRequest::to_resources(&[camera_spec, pos_spec], &[]),
            ),
            // LOD: read position + camera (should bundle with culling)
            Task::new(
                2,
                world::AccessRequest::to_resources(&[camera_spec, pos_spec], &[]),
            ),
            // Animation: write transform (different components = parallel)
            Task::new(
                3,
                world::AccessRequest::to_resources(&[], &[transform_spec]),
            ),
            // Render prep: read transform + material (conflicts with animation write)
            Task::new(
                4,
                world::AccessRequest::to_resources(&[transform_spec, material_spec], &[]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: Should have 2 groups
        // Group 1: Culling+LOD (bundled) + Animation (disjoint components)
        // Group 2: Render prep (conflicts with animation)
        assert_eq!(groups.len(), 2, "Rendering pipeline should have 2 phases");

        // First group should have culling/LOD bundle
        let first_group_has_bundle = groups[0].units.iter().any(|u| u.system_indexes.len() == 2);
        assert!(first_group_has_bundle, "Culling and LOD should bundle");
    }

    #[test]
    fn mixed_read_write_dependency_chain() {
        // Tests a dependency chain: A writes X, B reads X writes Y, C reads Y

        let comp1 = &[world::TypeId::new(1)];
        let comp2 = &[world::TypeId::new(2)];

        let tasks = vec![
            // System A: writes Comp1
            Task::new(1, world::AccessRequest::to_resources(&[], comp1)),
            // System B: reads Comp1, writes Comp2
            Task::new(2, world::AccessRequest::to_resources(comp1, comp2)),
            // System C: reads Comp2
            Task::new(3, world::AccessRequest::to_resources(comp2, &[])),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: A and C don't conflict (different components), but B conflicts with both
        // Group 1: A (writes Comp1) + C (reads Comp2) - no conflict
        // Group 2: B (reads Comp1, writes Comp2) - conflicts with both A and C
        assert_eq!(groups.len(), 2, "Should have 2 groups: (A+C), (B)");

        // Verify first group has 2 units (A and C)
        if groups[0].units.len() == 2 {
            // Expected: A and C in first group
            assert_eq!(
                groups[1].units.len(),
                1,
                "B should be alone in second group"
            );
        } else {
            // If prioritization put B first (due to mixed access), that's also valid
            assert_eq!(groups[0].units.len(), 1, "B in first group alone");
            assert_eq!(groups[1].units.len(), 2, "A and C in second group together");
        }
    }

    #[test]
    fn high_parallelism_workload() {
        // Tests a workload with many independent systems

        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(1)]),
            ),
            Task::new(
                2,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(2)]),
            ),
            Task::new(
                3,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(3)]),
            ),
            Task::new(
                4,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(4)]),
            ),
            Task::new(
                5,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(5)]),
            ),
        ];
        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: All systems access different components, should run in parallel
        assert_eq!(
            groups.len(),
            1,
            "Independent systems should run in parallel"
        );
        assert_eq!(
            groups[0].units.len(),
            5,
            "5 separate units (different access patterns)"
        );
    }

    #[test]
    fn bundling_efficiency_test() {
        // Tests that bundling actually groups many systems with identical access
        let access =
            world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[world::TypeId::new(2)]);

        // Create 20 systems with identical access
        let tasks: Vec<Task> = (1..=20).map(|i| Task::new(i, access.clone())).collect();

        let planner = GraphColorPlanner::WELSH_POWELL;

        // When
        let groups = planner.plan(&tasks);

        // Then: Should be one group with one unit containing all 20 systems
        assert_eq!(groups.len(), 1, "Identical access = one group");
        assert_eq!(groups[0].units.len(), 1, "Should bundle into single unit");
        assert_eq!(
            groups[0].units[0].system_indexes.len(),
            20,
            "All 20 systems should bundle together"
        );
    }

    #[test]
    fn sequential_planner_preserves_priority_order() {
        // Given
        let tasks = vec![
            Task::new(
                1,
                world::AccessRequest::to_resources(&[world::TypeId::new(1)], &[]),
            ),
            Task::new(2, world::AccessRequest::to_world(false)),
            Task::new(
                3,
                world::AccessRequest::to_resources(&[], &[world::TypeId::new(2)]),
            ),
        ];
        let planner = SequentialPlanner;

        // When
        let groups = planner.plan(&tasks);

        // Then: Should have 3 groups in priority order (world_mut first)
        assert_eq!(groups.len(), 3);
        assert!(
            groups[0].units[0].required_access.world(),
            "World read access should be first"
        );
    }
}
