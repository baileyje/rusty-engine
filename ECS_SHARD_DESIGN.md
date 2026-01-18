# ECS World Shard Design

This document describes the shard pattern for controlled mutable aliasing of the `World`, enabling parallel system execution with disjoint component access.

## Problem Statement

The ECS scheduler needs to execute multiple systems that access different components. Rust's borrow checker prevents multiple `&mut World` references, but systems accessing disjoint components are logically safe to run concurrently.

## Design Overview

**Shards** provide controlled mutable access to the world with runtime-enforced access restrictions:

1. Each shard holds a raw pointer to the World and an `AccessGrant`
2. The World tracks active grants using interior mutability (`RefCell<GrantTracker>`)
3. New shards are checked against active grants for conflicts
4. When a shard is consumed, its grant is returned for release on the main thread

## Key Types

```rust
pub struct Shard<'w> {
    world: *mut World,
    grant: AccessGrant,
    _marker: PhantomData<&'w World>,
}

// Shards can be sent to worker threads
unsafe impl Send for Shard<'_> {}
```

## Thread Safety Model

| Type | Send | Sync | Notes |
|------|------|------|-------|
| `World` | No | No | Stays on main thread |
| `Shard<'w>` | Yes | No | Sent to workers, used by one thread |
| `AccessGrant` | Yes | Yes | Plain data, freely copyable |

**Key invariants:**
- World is `!Send + !Sync` (stays on main thread)
- Shards are `Send` but `!Sync` (one thread at a time)
- Grant tracking uses `RefCell` (single-threaded, no mutex needed)

## Parallel Execution Pattern

The scheduler uses a **four-step pattern** for parallel system execution:

### Step 1: Create Shards on Main Thread
```rust
let shards: Vec<Shard> = group.units()
    .iter()
    .map(|unit| world.shard(unit.required_access()))
    .collect::<Result<_, _>>()?;
```

### Step 2: Create Send-Safe System Handles
```rust
struct SystemHandle(*mut system::System);
unsafe impl Send for SystemHandle {}

// Raw pointers wrapped for thread transfer
let handles: Vec<Vec<SystemHandle>> = /* create from system indices */;
```

### Step 3: Execute Units in Parallel
```rust
let grant_futures = executor.scope(|scope| {
    for ((mut shard, _), handles) in shards.into_iter().zip(system_handles) {
        scope.spawn_with_result(move || {
            for handle in handles {
                unsafe { handle.run_parallel(&mut shard); }
            }
            shard.into_grant()  // Return grant, don't drop on worker
        });
    }
});
```

### Step 4: Release Grants on Main Thread
```rust
for future in grant_futures {
    if let Ok(grant) = future.wait() {
        world.release_grant(&grant);
    }
}
```

## Why Raw Pointers Are Necessary

The scheduler needs to give each worker thread access to different systems from `Vec<System>`:

1. **Can't take multiple `&mut`** - borrow checker violation
2. **Can't clone systems** - contain `Box<dyn FnMut>` (not Clone)
3. **`Arc<Mutex<System>>` is too slow** - defeats parallelism
4. **`&mut` references aren't Send** - can't cross thread boundaries

Raw pointers with explicit `Send` wrapper solve all these issues. This is the same pattern as `slice::split_at_mut()` - creating multiple mutable references validated safe by external guarantees.

## Safety Guarantees

1. **Disjoint access**: Scheduler guarantees each unit accesses different system indices
2. **Lifetime guarantees**: Scoped execution ensures systems aren't dropped while workers run
3. **No escape**: Handles don't escape the scope
4. **Scheduler validation**: Conflict detection during planning ensures no data races

## Grant Lifecycle

```
Main Thread                          Worker Thread(s)
-----------                          ----------------
1. world.shard(access)
   - Creates shard, registers grant

2. Send shard to worker -----------> 3. Worker receives shard

                                     4. system.run(&mut shard)
                                        - Accesses components via grant

                                     5. shard.into_grant()
                                        - Returns grant (no Drop)

6. Receive grant <------------------ (grant sent back)

7. world.release_grant(&grant)
   - Removes from active_grants
```

## See Also

- `ECS_SCHEDULER_ALGORITHMS.md` - Graph coloring for parallel scheduling
- `ECS_SYSTEM_REFERENCE.md` - System parameter design (GAT + HRTB)
