//! Particle system benchmark scenario.
//!
//! Simulates a high-volume particle system with:
//! - 100,000 particles
//! - Simple components: Position, Velocity, Lifetime, Color, Size
//! - Systems: movement, lifetime decay, despawn dead particles
//!
//! This scenario tests:
//! - High entity count iteration performance
//! - Simple component access patterns
//! - Entity spawn/despawn throughput (particles dying and respawning)

use crossterm::{ExecutableCommand, style, terminal};
use std::io::{self};

use crate::components::{Color, DeltaTime, Lifetime, Particle, Position, Size, Velocity};
use crate::scenarios::Scenario;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rusty_engine::core::tasks::Executor;
use rusty_engine::define_phase;
use rusty_engine::ecs::{Commands, Entity, Query, Schedule, Uniq, UniqMut, World, WorldId};
use rusty_macros::Unique;

/// Configuration for the particle benchmark.
pub struct ParticleConfig {
    /// Total number of particles to maintain.
    pub particle_count: usize,
    /// Simulated delta time per frame.
    pub delta_time: f32,
    /// Random seed for reproducibility.
    pub seed: u64,
    /// The number of executor threads to use (if applicable).
    pub executor_threads: usize,
}

impl Default for ParticleConfig {
    fn default() -> Self {
        Self {
            particle_count: 100_000,
            delta_time: 1.0 / 60.0, // 60 FPS
            seed: 12345,
            executor_threads: 4,
        }
    }
}

#[derive(Unique)]
struct ParicleFactory(ChaCha8Rng);

impl ParicleFactory {
    fn create_particle(&mut self) -> (Particle, Position, Velocity, Lifetime, Color, Size) {
        let rng = &mut self.0;
        let pos = Position {
            x: rng.gen_range(-100.0..100.0),
            y: rng.gen_range(-100.0..100.0),
            z: rng.gen_range(-100.0..100.0),
        };
        let vel = Velocity {
            x: rng.gen_range(-10.0..10.0),
            y: rng.gen_range(-10.0..10.0),
            z: rng.gen_range(-10.0..10.0),
        };
        let lifetime = Lifetime {
            remaining: rng.gen_range(1.0..5.0),
            total: 5.0,
        };
        let color = Color {
            r: rng.gen_range(0.0..1.0),
            g: rng.gen_range(0.0..1.0),
            b: rng.gen_range(0.0..1.0),
            a: 1.0,
        };
        let size = Size {
            width: rng.gen_range(0.1..2.0),
            height: rng.gen_range(0.1..2.0),
        };

        (Particle, pos, vel, lifetime, color, size)
    }
}

define_phase!(Update, Render);

/// System: Update particle positions based on velocity.
fn system_movement(query: Query<(&mut Position, &Velocity)>, dt: Uniq<DeltaTime>) {
    for (pos, vel) in query {
        pos.x += vel.x * dt.0;
        pos.y += vel.y * dt.0;
        pos.z += vel.z * dt.0;
    }
}

/// System: Decay particle lifetimes.
fn system_lifetime_decay(query: Query<&mut Lifetime>, dt: Uniq<DeltaTime>) {
    for lifetime in query {
        lifetime.remaining -= dt.0;
    }
}

/// System: Fade particles based on remaining lifetime.
fn system_fade(query: Query<(&Lifetime, &mut Color)>) {
    for (lifetime, color) in query {
        color.a = (lifetime.remaining / lifetime.total).max(0.0);
    }
}

/// System: Collect dead particles (lifetime <= 0)
fn system_kill_particles(
    commands: Commands,
    query: Query<(Entity, &Lifetime)>,
    mut factory: UniqMut<ParicleFactory>,
) {
    for (entity, life) in query {
        if life.remaining <= 0.0 {
            commands.despawn(entity);
            commands.spawn(factory.create_particle());
        }
    }
}

const SCREEN_WIDTH: usize = 61;
const SCREEN_HEIGHT: usize = 61;

const MIN_X: f32 = -30.0;
const MAX_X: f32 = 30.0;
const MIN_Y: f32 = -30.0;
const MAX_Y: f32 = 30.0;

fn system_render(query: Query<(&Position, &Color, &Size, &Lifetime)>) {
    let mut visible: Vec<Vec<Option<(Color, Size, Lifetime)>>> = Vec::new();
    visible.resize_with(SCREEN_HEIGHT, || {
        let mut row = Vec::new();
        row.resize(SCREEN_WIDTH, None);
        row
    });
    for (pos, col, siz, lif) in query {
        if pos.x >= MIN_X && pos.x <= MAX_X && pos.y >= MIN_Y && pos.y <= MAX_Y {
            let screen_x = MAX_X + pos.x;
            let screen_y = MAX_Y + pos.y;

            // println!("screen: {:?}:{:?}", screen_x, screen_y);

            visible[screen_y as usize][screen_x as usize] = Some((*col, *siz, *lif));
        }
    }
    let mut stdout = io::stdout();
    stdout
        .execute(terminal::Clear(terminal::ClearType::All))
        .unwrap();

    for row in visible {
        for col in row {
            if let Some((col, siz, lif)) = col {
                if lif.remaining < 1.0 {
                    stdout
                        .execute(style::SetAttribute(style::Attribute::Dim))
                        .unwrap();
                } else if lif.remaining > 3.0 {
                    stdout
                        .execute(style::SetAttribute(style::Attribute::Bold))
                        .unwrap();
                } else {
                    stdout
                        .execute(style::SetAttribute(style::Attribute::Reset))
                        .unwrap();
                }
                stdout
                    .execute(style::SetForegroundColor(style::Color::Rgb {
                        r: (col.r * 255.0) as u8,
                        g: (col.g * 255.0) as u8,
                        b: (col.b * 256.0) as u8,
                    }))
                    .unwrap();
                if siz.width + siz.height < 1.0 {
                    print!(". ");
                } else if siz.width + siz.height > 3.0 {
                    print!("@ ");
                } else {
                    print!("* ");
                }
            } else {
                print!("  ");
            }
        }
        println!();
    }
}

/// Particle system benchmark scenario.
pub struct ParticleScenario {
    config: ParticleConfig,
    world: World,
    schedule: Schedule,
    executor: Executor,
}

impl ParticleScenario {
    /// Create a new particle scenario with default config.
    pub fn new() -> Self {
        Self::with_config(ParticleConfig::default())
    }

    /// Create a new particle scenario with custom config.
    pub fn with_config(config: ParticleConfig) -> Self {
        Self {
            world: World::new(WorldId::new(0)),
            schedule: Schedule::new(),
            executor: Executor::new(config.executor_threads),
            config,
        }
    }

    /// Get current particle count.
    pub fn current_count(&mut self) -> usize {
        println!("Hmm: {:?}", self.world.storage().entities());
        self.world.storage().entities().spwaned_len()
    }
}

impl Default for ParticleScenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario for ParticleScenario {
    fn name(&self) -> &'static str {
        "particles"
    }

    fn description(&self) -> &'static str {
        "High-volume particle system with movement, lifetime, and respawn"
    }

    fn entity_count(&self) -> usize {
        self.config.particle_count
    }

    fn setup(&mut self) {
        let mut factory = ParicleFactory(ChaCha8Rng::seed_from_u64(self.config.seed));

        for _ in 0..self.config.particle_count {
            self.world.spawn(factory.create_particle());
        }

        self.schedule
            .add_system(Update, system_movement, &mut self.world);
        self.schedule
            .add_system(Update, system_fade, &mut self.world);
        self.schedule
            .add_system(Update, system_lifetime_decay, &mut self.world);
        self.schedule
            .add_system(Update, system_kill_particles, &mut self.world);

        self.schedule
            .add_system(Render, system_render, &mut self.world);

        self.world.add_unique(DeltaTime(self.config.delta_time));
        self.world.add_unique(factory);
    }

    fn update(&mut self) {
        self.schedule.run(Update, &mut self.world, &self.executor);
        // self.schedule.run(Render, &mut self.world, &self.executor);
    }

    fn teardown(&mut self) {
        let entities = self.world.query::<Entity>().collect::<Vec<_>>();
        for entity in entities {
            self.world.despawn(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_scenario_setup() {
        let mut scenario = ParticleScenario::with_config(ParticleConfig {
            particle_count: 100,
            ..Default::default()
        });

        scenario.setup();
        assert_eq!(scenario.current_count(), 100);

        scenario.teardown();
        assert_eq!(scenario.current_count(), 0);
    }

    #[test]
    fn particle_scenario_update() {
        let mut scenario = ParticleScenario::with_config(ParticleConfig {
            particle_count: 100,
            ..Default::default()
        });

        scenario.setup();

        // Run a few frames
        for _ in 0..10 {
            scenario.update();
        }

        // Should still have ~100 particles (some died and respawned)
        assert!(scenario.current_count() > 0);

        scenario.teardown();
    }
}
