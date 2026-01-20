//! Physics simulation benchmark scenario.
//!
//! Simulates a physics-heavy workload with:
//! - ~50,000 rigid bodies
//! - Position, Velocity, Acceleration integration
//! - Transform matrix updates
//!
//! This scenario tests:
//! - Compute-heavy component updates
//! - Large entity iteration
//! - Multi-component access patterns

use crate::components::{Acceleration, DeltaTime, Position, Transform, Velocity};
use crate::scenarios::Scenario;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rusty_engine::core::tasks::Executor;
use rusty_engine::define_phase;
use rusty_engine::ecs::schedule::Schedule;
use rusty_engine::ecs::system::{Query, Uniq};
use rusty_engine::ecs::{entity, world};

/// Configuration for the physics benchmark.
pub struct PhysicsConfig {
    /// Number of physics bodies.
    pub body_count: usize,
    /// Fixed timestep for physics integration.
    pub delta_time: f32,
    /// Random seed for reproducibility.
    pub seed: u64,
    /// The number of executor threads to use (if applicable).
    pub executor_threads: usize,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            body_count: 50_000,
            delta_time: 1.0 / 120.0, // 120 Hz physics
            seed: 99999,
            executor_threads: 8,
        }
    }
}

/// Physics simulation benchmark scenario.
pub struct PhysicsScenario {
    config: PhysicsConfig,
    world: world::World,
    rng: ChaCha8Rng,
    bodies: Vec<entity::Entity>,
    schedule: Schedule,
    executor: Executor,
}

define_phase!(FixedUpdate);

/// System: Integrate acceleration into velocity.
fn system_integrate_acceleration(
    query: Query<(&Acceleration, &mut Velocity)>,
    delta_time: Uniq<DeltaTime>,
) {
    for (accel, vel) in query {
        vel.x += accel.x * delta_time.0;
        vel.y += accel.y * delta_time.0;
        vel.z += accel.z * delta_time.0;
    }
}

/// System: Integrate velocity into position.
fn system_integrate_velocity(query: Query<(&Velocity, &mut Position)>, dt: Uniq<DeltaTime>) {
    for (vel, pos) in query {
        pos.x += vel.x * dt.0;
        pos.y += vel.y * dt.0;
        pos.z += vel.z * dt.0;
    }
}

/// System: Update transform matrices from position.
/// This is intentionally compute-heavy to simulate real transform updates.
fn system_update_transforms(query: Query<(&Position, &mut Transform)>) {
    for (pos, transform) in query {
        // Build translation matrix (simplified - real impl would include rotation/scale)
        transform.matrix[0][3] = pos.x;
        transform.matrix[1][3] = pos.y;
        transform.matrix[2][3] = pos.z;

        // Simulate some additional matrix work (normalize, etc.)
        // This makes the benchmark more representative of real transform systems
        let scale = 1.0
            / (transform.matrix[0][0] * transform.matrix[0][0]
                + transform.matrix[1][1] * transform.matrix[1][1]
                + transform.matrix[2][2] * transform.matrix[2][2])
                .sqrt();

        transform.matrix[0][0] *= scale;
        transform.matrix[1][1] *= scale;
        transform.matrix[2][2] *= scale;
    }
}

/// System: Simple boundary enforcement (keeps bodies in bounds).
fn system_enforce_boundaries(query: Query<(&mut Position, &mut Velocity)>) {
    let bounds = 1000.0;
    for (pos, vel) in query {
        // Bounce off boundaries
        if pos.x.abs() > bounds {
            pos.x = pos.x.signum() * bounds;
            vel.x = -vel.x * 0.8; // Energy loss
        }
        if pos.y.abs() > bounds {
            pos.y = pos.y.signum() * bounds;
            vel.y = -vel.y * 0.8;
        }
        if pos.z.abs() > bounds {
            pos.z = pos.z.signum() * bounds;
            vel.z = -vel.z * 0.8;
        }
    }
}

impl PhysicsScenario {
    /// Create a new physics scenario with default config.
    pub fn new() -> Self {
        Self::with_config(PhysicsConfig::default())
    }

    /// Create a new physics scenario with custom config.
    pub fn with_config(config: PhysicsConfig) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(config.seed),
            world: world::World::new(world::Id::new(0)),
            bodies: Vec::new(),
            schedule: Schedule::new(),
            executor: Executor::new(config.executor_threads),
            config,
        }
    }

    fn spawn_body(&mut self) -> entity::Entity {
        let pos = Position {
            x: self.rng.gen_range(-1000.0..1000.0),
            y: self.rng.gen_range(-1000.0..1000.0),
            z: self.rng.gen_range(-1000.0..1000.0),
        };
        let vel = Velocity {
            x: self.rng.gen_range(-10.0..10.0),
            y: self.rng.gen_range(-10.0..10.0),
            z: self.rng.gen_range(-10.0..10.0),
        };
        let accel = Acceleration {
            x: 0.0,
            y: -9.81, // Gravity
            z: 0.0,
        };
        let transform = Transform::default();

        self.world.spawn((pos, vel, accel, transform))
    }

    /// Current body count.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }
}

impl Default for PhysicsScenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario for PhysicsScenario {
    fn name(&self) -> &'static str {
        "physics"
    }

    fn description(&self) -> &'static str {
        "Physics simulation with acceleration/velocity integration and transforms"
    }

    fn entity_count(&self) -> usize {
        self.config.body_count
    }

    fn setup(&mut self) {
        self.bodies = Vec::with_capacity(self.config.body_count);
        for _ in 0..self.config.body_count {
            let entity = self.spawn_body();
            self.bodies.push(entity);
        }

        self.schedule
            .add_system(FixedUpdate, system_integrate_acceleration, &mut self.world);
        self.schedule
            .add_system(FixedUpdate, system_integrate_velocity, &mut self.world);
        self.schedule
            .add_system(FixedUpdate, system_update_transforms, &mut self.world);
        self.schedule
            .add_system(FixedUpdate, system_enforce_boundaries, &mut self.world);

        self.world.add_unique(DeltaTime(self.config.delta_time));
    }

    fn update(&mut self) {
        self.schedule
            .run(FixedUpdate, &mut self.world, &self.executor);
    }

    fn teardown(&mut self) {
        for entity in self.bodies.drain(..) {
            self.world.despawn(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physics_scenario_setup() {
        let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
            body_count: 100,
            ..Default::default()
        });

        scenario.setup();
        assert_eq!(scenario.body_count(), 100);

        scenario.teardown();
        assert_eq!(scenario.body_count(), 0);
    }

    #[test]
    fn physics_scenario_update() {
        let mut scenario = PhysicsScenario::with_config(PhysicsConfig {
            body_count: 100,
            ..Default::default()
        });

        scenario.setup();

        // Run physics for a few frames
        for _ in 0..60 {
            scenario.update();
        }

        assert_eq!(scenario.body_count(), 100);
        scenario.teardown();
    }
}
