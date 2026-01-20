//! ECS microbenchmarks using Criterion.
//!
//! These benchmarks measure individual ECS operations in isolation:
//! - Entity spawn/despawn
//! - Component iteration
//! - Component add/remove (migration)
//! - Query performance

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rusty_bench::components::*;
use rusty_engine::ecs::world;

// =============================================================================
// Spawn Benchmarks
// =============================================================================

fn bench_spawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("spawn");

    for count in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(count as u64));

        // Single component spawn
        group.bench_with_input(BenchmarkId::new("single_component", count), &count, |b, &n| {
            b.iter(|| {
                let mut world = world::World::new(world::Id::new(0));
                for _ in 0..n {
                    black_box(world.spawn(Position::default()));
                }
            });
        });

        // Multi-component spawn (4 components like ecs_bench_suite)
        group.bench_with_input(BenchmarkId::new("four_components", count), &count, |b, &n| {
            b.iter(|| {
                let mut world = world::World::new(world::Id::new(0));
                for _ in 0..n {
                    black_box(world.spawn((
                        Transform::default(),
                        Position::default(),
                        Rotation::default(),
                        Velocity::default(),
                    )));
                }
            });
        });

        // Batch spawn using spawn_many
        group.bench_with_input(BenchmarkId::new("batch_single", count), &count, |b, &n| {
            b.iter(|| {
                let mut world = world::World::new(world::Id::new(0));
                let components: Vec<_> = (0..n).map(|_| Position::default()).collect();
                black_box(world.spawn_many(components));
            });
        });

        group.bench_with_input(BenchmarkId::new("batch_four", count), &count, |b, &n| {
            b.iter(|| {
                let mut world = world::World::new(world::Id::new(0));
                let components: Vec<_> = (0..n)
                    .map(|_| {
                        (
                            Transform::default(),
                            Position::default(),
                            Rotation::default(),
                            Velocity::default(),
                        )
                    })
                    .collect();
                black_box(world.spawn_many(components));
            });
        });
    }

    group.finish();
}

// =============================================================================
// Iteration Benchmarks
// =============================================================================

fn bench_simple_iter(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_iter");

    for count in [1_000, 10_000, 100_000] {
        group.throughput(Throughput::Elements(count as u64));

        // Setup world once for iteration benchmarks
        group.bench_with_input(BenchmarkId::new("pos_vel", count), &count, |b, &n| {
            // Setup
            let mut world = world::World::new(world::Id::new(0));
            let components: Vec<_> = (0..n)
                .map(|i| {
                    (
                        Position {
                            x: i as f32,
                            y: 0.0,
                            z: 0.0,
                        },
                        Velocity {
                            x: 1.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    )
                })
                .collect();
            world.spawn_many(components);

            b.iter(|| {
                for (pos, vel) in world.query::<(&mut Position, &Velocity)>() {
                    pos.x += vel.x;
                    pos.y += vel.y;
                    pos.z += vel.z;
                }
            });
        });

        // Single component iteration
        group.bench_with_input(BenchmarkId::new("single", count), &count, |b, &n| {
            let mut world = world::World::new(world::Id::new(0));
            let components: Vec<_> = (0..n)
                .map(|i| Position {
                    x: i as f32,
                    y: 0.0,
                    z: 0.0,
                })
                .collect();
            world.spawn_many(components);

            b.iter(|| {
                for pos in world.query::<&mut Position>() {
                    pos.x += 1.0;
                }
            });
        });

        // Four component iteration
        group.bench_with_input(BenchmarkId::new("four_components", count), &count, |b, &n| {
            let mut world = world::World::new(world::Id::new(0));
            let components: Vec<_> = (0..n)
                .map(|_| {
                    (
                        Transform::default(),
                        Position::default(),
                        Rotation::default(),
                        Velocity::default(),
                    )
                })
                .collect();
            world.spawn_many(components);

            b.iter(|| {
                for (pos, vel, _rot, _transform) in
                    world.query::<(&mut Position, &Velocity, &Rotation, &Transform)>()
                {
                    pos.x += vel.x;
                    pos.y += vel.y;
                    pos.z += vel.z;
                }
            });
        });
    }

    group.finish();
}

// =============================================================================
// Fragmented Iteration Benchmarks
// =============================================================================

fn bench_fragmented_iter(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmented_iter");

    // Create 26 archetypes with 20 entities each (like ecs_bench_suite)
    let archetype_count = 26;
    let entities_per_archetype = 20;
    let total = archetype_count * entities_per_archetype;

    group.throughput(Throughput::Elements(total as u64));

    group.bench_function("26_archetypes", |b| {
        let mut world = world::World::new(world::Id::new(0));

        // Spawn entities with different marker components to create fragmentation
        // Each archetype has Data + one unique marker
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerA));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerB));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerC));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerD));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerE));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerF));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerG));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerH));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerI));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerJ));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerK));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerL));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerM));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerN));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerO));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerP));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerQ));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerR));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerS));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerT));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerU));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerV));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerW));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerX));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerY));
        }
        for _ in 0..entities_per_archetype {
            world.spawn((Data { value: 1.0 }, MarkerZ));
        }

        b.iter(|| {
            for data in world.query::<&mut Data>() {
                data.value *= 2.0;
            }
        });
    });

    group.finish();
}

// =============================================================================
// Component Migration Benchmarks
// =============================================================================

fn bench_add_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_remove");

    for count in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(count as u64));

        // Add component to existing entities
        group.bench_with_input(BenchmarkId::new("add_component", count), &count, |b, &n| {
            b.iter_batched(
                || {
                    // Setup: create world with entities that have Position only
                    let mut world = world::World::new(world::Id::new(0));
                    let components: Vec<_> = (0..n).map(|_| Position::default()).collect();
                    let entities = world.spawn_many(components);
                    (world, entities)
                },
                |(mut world, entities)| {
                    // Benchmark: add Velocity to each entity
                    for entity in entities {
                        world.add_components(entity, Velocity::default());
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });

        // Remove component from existing entities
        group.bench_with_input(
            BenchmarkId::new("remove_component", count),
            &count,
            |b, &n| {
                b.iter_batched(
                    || {
                        // Setup: create world with entities that have Position + Velocity
                        let mut world = world::World::new(world::Id::new(0));
                        let components: Vec<_> = (0..n)
                            .map(|_| (Position::default(), Velocity::default()))
                            .collect();
                        let entities = world.spawn_many(components);
                        (world, entities)
                    },
                    |(mut world, entities)| {
                        // Benchmark: remove Velocity from each entity
                        for entity in entities {
                            world.remove_components::<Velocity>(entity);
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// =============================================================================
// Despawn Benchmarks
// =============================================================================

fn bench_despawn(c: &mut Criterion) {
    let mut group = c.benchmark_group("despawn");

    for count in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::new("single_component", count), &count, |b, &n| {
            b.iter_batched(
                || {
                    let mut world = world::World::new(world::Id::new(0));
                    let components: Vec<_> = (0..n).map(|_| Position::default()).collect();
                    let entities = world.spawn_many(components);
                    (world, entities)
                },
                |(mut world, entities)| {
                    for entity in entities {
                        world.despawn(entity);
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("four_components", count), &count, |b, &n| {
            b.iter_batched(
                || {
                    let mut world = world::World::new(world::Id::new(0));
                    let components: Vec<_> = (0..n)
                        .map(|_| {
                            (
                                Transform::default(),
                                Position::default(),
                                Rotation::default(),
                                Velocity::default(),
                            )
                        })
                        .collect();
                    let entities = world.spawn_many(components);
                    (world, entities)
                },
                |(mut world, entities)| {
                    for entity in entities {
                        world.despawn(entity);
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    benches,
    bench_spawn,
    bench_simple_iter,
    bench_fragmented_iter,
    bench_add_remove,
    bench_despawn,
);

criterion_main!(benches);
