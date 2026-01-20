//! ECS scenario benchmarks using Criterion.
//!
//! These benchmarks measure realistic game workloads:
//! - Particle system (high entity count, simple components)
//! - Game world (mixed archetypes, AI, combat)
//! - Physics simulation (compute-heavy transforms)

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rusty_bench::{
    frame_timer::measure_frames,
    scenarios::{
        GameWorldConfig, GameWorldScenario, ParticleConfig, ParticleScenario, PhysicsConfig,
        PhysicsScenario, Scenario,
    },
};

// =============================================================================
// Particle System Benchmarks
// =============================================================================

fn bench_particles(c: &mut Criterion) {
    let mut group = c.benchmark_group("scenario/particles");

    for count in [10_000, 50_000, 100_000] {
        group.throughput(Throughput::Elements(count as u64));

        // Full frame update
        group.bench_with_input(BenchmarkId::new("frame", count), &count, |b, &n| {
            let mut scenario = ParticleScenario::with_config(ParticleConfig {
                particle_count: n,
                ..Default::default()
            });
            scenario.setup();

            b.iter(|| {
                scenario.update();
            });
        });

        // Individual systems
        // group.bench_with_input(BenchmarkId::new("movement", count), &count, |b, &n| {
        //     let mut scenario = ParticleScenario::with_config(ParticleConfig {
        //         particle_count: n,
        //         ..Default::default()
        //     });
        //     scenario.setup();
        //
        //     b.iter(|| {
        //         scenario.system_movement();
        //     });
        // });
        //
        // group.bench_with_input(BenchmarkId::new("lifetime", count), &count, |b, &n| {
        //     let mut scenario = ParticleScenario::with_config(ParticleConfig {
        //         particle_count: n,
        //         ..Default::default()
        //     });
        //     scenario.setup();
        //
        //     b.iter(|| {
        //         scenario.system_lifetime_decay();
        //     });
        // });
    }

    group.finish();
}

// =============================================================================
// Game World Benchmarks
// =============================================================================

fn bench_game_world(c: &mut Criterion) {
    let mut group = c.benchmark_group("scenario/game_world");

    // Default config: ~10,100 entities across 4 archetypes
    let configs = [
        (
            "small",
            GameWorldConfig {
                npc_count: 500,
                player_count: 10,
                projectile_count: 200,
                static_count: 300,
                ..Default::default()
            },
        ),
        (
            "medium",
            GameWorldConfig {
                npc_count: 2_500,
                player_count: 50,
                projectile_count: 1_000,
                static_count: 1_500,
                ..Default::default()
            },
        ),
        (
            "large",
            GameWorldConfig {
                npc_count: 5_000,
                player_count: 100,
                projectile_count: 2_000,
                static_count: 3_000,
                ..Default::default()
            },
        ),
    ];

    for (name, config) in configs {
        let total =
            config.npc_count + config.player_count + config.projectile_count + config.static_count;
        group.throughput(Throughput::Elements(total as u64));

        // Full frame update
        group.bench_with_input(BenchmarkId::new("frame", name), &config, |b, config| {
            let mut scenario = GameWorldScenario::with_config(config.clone());
            scenario.setup();

            b.iter(|| {
                scenario.update();
            });
        });
        //
        // // AI system only
        // group.bench_with_input(BenchmarkId::new("ai", name), &config, |b, config| {
        //     let mut scenario = GameWorldScenario::with_config(config.clone());
        //     scenario.setup();
        //
        //     b.iter(|| {
        //         scenario.system_ai();
        //     });
        // });
        //
        // // Movement system only
        // group.bench_with_input(BenchmarkId::new("movement", name), &config, |b, config| {
        //     let mut scenario = GameWorldScenario::with_config(config.clone());
        //     scenario.setup();
        //
        //     b.iter(|| {
        //         scenario.system_movement();
        //     });
        // });
    }

    group.finish();
}

// =============================================================================
// Physics Simulation Benchmarks
// =============================================================================

fn bench_physics(c: &mut Criterion) {
    let mut group = c.benchmark_group("scenario/physics");

    for count in [10_000, 25_000, 50_000] {
        group.throughput(Throughput::Elements(count as u64));

        // Full physics step
        group.bench_with_input(BenchmarkId::new("step", count), &count, |b, &n| {
            let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
                body_count: n,
                ..Default::default()
            });
            scenario.setup();

            b.iter(|| {
                scenario.update();
            });
        });

        // // Individual systems
        // group.bench_with_input(
        //     BenchmarkId::new("integrate_velocity", count),
        //     &count,
        //     |b, &n| {
        //         let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
        //             body_count: n,
        //             ..Default::default()
        //         });
        //         scenario.setup();
        //
        //         b.iter(|| {
        //             scenario.system_integrate_velocity();
        //         });
        //     },
        // );
        //
        // group.bench_with_input(
        //     BenchmarkId::new("update_transforms", count),
        //     &count,
        //     |b, &n| {
        //         let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
        //             body_count: n,
        //             ..Default::default()
        //         });
        //         scenario.setup();
        //
        //         b.iter(|| {
        //             scenario.system_update_transforms();
        //         });
        //     },
        // );
    }

    group.finish();
}

// =============================================================================
// Frame Time Benchmarks (longer running, statistical)
// =============================================================================

fn bench_frame_times(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_times");
    group.sample_size(20); // Fewer samples since each runs many frames

    // Particle scenario: 1000 frames
    group.bench_function("particles_1000_frames", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut scenario = ParticleScenario::with_config(ParticleConfig {
                    particle_count: 50_000,
                    ..Default::default()
                });
                scenario.setup();

                let stats = measure_frames(1000, |_| {
                    scenario.update();
                });

                total += stats.total_duration;
            }

            total
        });
    });

    // Physics scenario: 1000 frames
    group.bench_function("physics_1000_frames", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;

            for _ in 0..iters {
                let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
                    body_count: 25_000,
                    ..Default::default()
                });
                scenario.setup();

                let stats = measure_frames(1000, |_| {
                    scenario.update();
                });

                total += stats.total_duration;
            }

            total
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Configuration
// =============================================================================

criterion_group!(
    benches,
    bench_particles,
    bench_game_world,
    bench_physics,
    bench_frame_times,
);

criterion_main!(benches);
