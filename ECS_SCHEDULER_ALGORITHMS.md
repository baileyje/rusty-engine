# ECS Scheduler Algorithms - Parallel System Grouping

Research notes on algorithms for scheduling ECS systems into parallel execution groups.

## Problem Statement

Given a set of systems where each system declares:
- **Read access** to certain components (shared, non-exclusive)
- **Write access** to certain components (exclusive, mutable)

Goal: Partition systems into minimal groups where systems within each group can run in parallel without resource conflicts.

## Conflict Rules (Readers-Writer Pattern)

- **Write vs Write** on same component: **CONFLICT**
- **Write vs Read** on same component: **CONFLICT**
- **Read vs Read** on same component: **NO CONFLICT** (parallel reads allowed)

## Three-Phase Algorithm

### Phase 0: Extract Exclusive World Access Systems
Systems requiring full world mutability (spawn/despawn, structural changes) run sequentially as a pre-phase before all other systems.

### Phase 1: Bundle Systems with Identical Access
Systems with identical access patterns are bundled into a single execution unit:
- **Single resource checkout** for the entire bundle
- **Cache locality** - data stays hot in L1/L2 across bundled systems
- **Reduced scheduling overhead** - fewer parallel tasks to coordinate

### Phase 2: Schedule Bundles via Graph Coloring
This is a graph coloring problem:
- Bundles are nodes
- Edges connect bundles that conflict
- Each color (group) contains bundles that can run in parallel
- Goal: minimize number of colors (groups)

**Time Complexity:** O(n log n + b² × c) where n = systems, b = bundles, c = components

## Scheduling Algorithms

### Welsh-Powell (Greedy)
1. Calculate conflict degree for each bundle
2. Sort by degree descending (most constrained first)
3. Greedily assign to first compatible group

Simple, fast, typically achieves 70-90% of optimal.

### DSATUR (Degree of Saturation)
Dynamically picks the most constrained bundle next based on how many groups it's already blocked from. Typically produces 10-30% fewer groups than simple greedy.

### Optimal (Backtracking)
Guarantees optimal solution but exponential worst-case. Only practical for <20 systems.

## Performance Comparison

| Algorithm | Build Schedule | Quality |
|-----------|---------------|---------|
| Greedy | O(b² × c) | ~80% optimal |
| DSATUR | O(b² × c) | ~90% optimal |
| Optimal | O(2^b × c) | 100% optimal |

## Bundling Benefits

**Without bundling:** N systems → N separate `World::shard()` calls
**With bundling:** N systems in B bundles → B shard calls (B << N when access patterns repeat)

Common scenarios with high bundling benefit:
- **Physics pipelines**: Multiple force systems (gravity, wind, friction) with identical access
- **AI updates**: Multiple behavior systems reading/writing same state components
- **Rendering**: Multiple culling/LOD systems with identical queries

## Execution Order

```
Frame N:
  1. Pre-phase (sequential):
     - Exclusive system A (spawn/despawn)
     - Exclusive system B (structural changes)

  2. Regular phases (parallel):
     Group 1 (parallel):
       - Bundle A: [System 1, 2, 3] (sequential, shared shard)
       - Bundle B: [System 4] (single shard)

     Group 2 (after Group 1):
       - Bundle C: [System 5, 6] (sequential, shared shard)
```

## References

1. **Graph Coloring**
   - Welsh & Powell (1967) - Upper bound for chromatic number
   - Brélaz (1979) - DSATUR algorithm

2. **ECS Schedulers**
   - [Bevy's System Execution Model](https://github.com/bevyengine/bevy/tree/main/crates/bevy_ecs/src/schedule)
   - [Specs Parallel Dispatcher](https://docs.rs/specs/latest/specs/struct.Dispatcher.html)

3. **Readers-Writer Problem**
   - Courtois, Heymans & Parnas (1971) - Concurrent control with readers and writers

## See Also

- `ECS_SHARD_DESIGN.md` - Shard pattern and parallel execution
- `ECS_SYSTEM_REFERENCE.md` - System parameter design
