//! Game world benchmark scenario.
//!
//! Simulates a mixed game world with:
//! - ~10,000 entities across multiple archetypes
//! - NPCs with AI, health, team affiliation
//! - Players with inventory
//! - Projectiles with short lifetimes
//! - Static objects
//!
//! This scenario tests:
//! - Multiple archetype iteration
//! - Complex component combinations
//! - Varied system workloads

use std::io::{self, Write};

use crate::components::{
    AiState, DeltaTime, Health, Lifetime, Npc, Player, Position, Static, Team, Velocity,
};
use crate::scenarios::Scenario;
use crossterm::{QueueableCommand, style, terminal};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rusty_engine::core::tasks::Executor;
use rusty_engine::define_phase;
use rusty_engine::ecs::{Commands, Entity, Query, Schedule, Uniq, World, WorldId};
use rusty_macros::Component;

define_phase!(Update, FixedUpdate, Render);

// fn spawn_projectile(&mut self) -> Entity {
//     let pos = self.random_position();
//     let vel = Velocity {
//         x: self.rng.gen_range(-50.0..50.0),
//         y: self.rng.gen_range(-50.0..50.0),
//         z: 0.0,
//     };
//     let lifetime = Lifetime {
//         remaining: self.rng.gen_range(0.5..2.0),
//         total: 2.0,
//     };
//     let team = Team {
//         id: self.rng.gen_range(0..4),
//     };
//
//     self.world
//         .spawn((pos, vel, lifetime, team, Glyph(".".to_string())))
// }

/// System: Update AI state and decisions.
fn system_ai(
    query: Query<(&Position, &mut AiState, &mut Velocity, &Team)>,
    dt: Uniq<DeltaTime>,
    commands: Commands,
) {
    for (pos, ai, vel, team) in query {
        ai.timer -= dt.0;
        if ai.timer <= 0.0 {
            // Pick new target (simple state change)
            ai.state = (ai.state + 1) % 4;
            ai.timer = 2.0;
        }

        // Move towards target
        let dx = ai.target_x - pos.x;
        let dy = ai.target_y - pos.y;
        let dist = (dx * dx + dy * dy).sqrt().max(0.001);
        let speed = 10.0;
        vel.x = dx / dist * speed;
        vel.y = dy / dist * speed;

        // commands.spawn((
        //     Projectile,
        //     *pos,
        //     Velocity {
        //         x: vel.x * 2.0,
        //         y: vel.y * 2.0,
        //         z: 0.0,
        //     },
        //     Lifetime {
        //         remaining: 3.0,
        //         total: 2.0,
        //     },
        //     *team,
        //     Glyph(".".to_string()),
        // ));
    }
}

/// System: Apply velocity to position.
fn system_movement(query: Query<(&mut Position, &Velocity)>, dt: Uniq<DeltaTime>) {
    for (pos, vel) in query {
        pos.x += vel.x * dt.0;
        pos.y += vel.y * dt.0;
        pos.z += vel.z * dt.0;
    }
}

/// System: Decay projectile lifetimes and defer spawn dead ones.
fn system_projectile_lifetime(
    query: Query<(Entity, &mut Lifetime)>,
    dt: Uniq<DeltaTime>,
    commands: Commands,
) {
    for (entity, lifetime) in query {
        lifetime.remaining -= dt.0;
        if lifetime.remaining <= 0.0 {
            commands.despawn(entity);
        }
    }
}

const MIN_X: f32 = -30.0;
const MAX_X: f32 = 30.0;
const MIN_Y: f32 = -30.0;
const MAX_Y: f32 = 30.0;

#[derive(Component, Clone)]
struct Glyph(String);

fn system_render(query: Query<(&Position, &Glyph)>) {
    let mut stdout = io::stdout();

    stdout
        .queue(terminal::Clear(terminal::ClearType::All))
        .unwrap()
        .queue(crossterm::cursor::Hide)
        .unwrap();

    for (pos, glyph) in query {
        if pos.x >= MIN_X && pos.x <= MAX_X && pos.y >= MIN_Y && pos.y <= MAX_Y {
            let screen_x = (pos.x - MIN_X) as usize;
            let screen_y = (pos.y - MIN_Y) as usize;
            stdout
                .queue(crossterm::cursor::MoveTo(screen_x as u16, screen_y as u16))
                .unwrap()
                .queue(style::Print(&glyph.0))
                .unwrap();
        }
    }
    stdout.flush().unwrap();
    // stdout.execute(crossterm::cursor::Show).unwrap();
}

/// Configuration for the game world benchmark.
#[derive(Clone)]
pub struct GameWorldConfig {
    /// Number of NPC entities.
    pub npc_count: usize,
    /// Number of player entities.
    pub player_count: usize,
    /// Number of projectile entities.
    pub projectile_count: usize,
    /// Number of static objects.
    pub static_count: usize,
    /// Simulated delta time per frame.
    pub delta_time: f32,
    /// Random seed for reproducibility.
    pub seed: u64,
    /// The number of executor threads to use (if applicable).
    pub executor_threads: usize,
}

impl Default for GameWorldConfig {
    fn default() -> Self {
        Self {
            npc_count: 5_000,
            player_count: 100,
            projectile_count: 2_000,
            static_count: 3_000,
            delta_time: 1.0 / 60.0,
            seed: 54321,
            executor_threads: 8,
        }
    }
}

/// Game world benchmark scenario.
pub struct GameWorldScenario {
    config: GameWorldConfig,
    world: World,
    rng: ChaCha8Rng,
    schedule: Schedule,
    executor: Executor,
}

impl GameWorldScenario {
    /// Create a new game world scenario with default config.
    pub fn new() -> Self {
        Self::with_config(GameWorldConfig::default())
    }

    /// Create a new game world scenario with custom config.
    pub fn with_config(config: GameWorldConfig) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(config.seed),
            world: World::new(WorldId::new(0)),
            schedule: Schedule::new(),
            executor: Executor::new(config.executor_threads),
            config,
        }
    }

    fn random_position(&mut self) -> Position {
        Position {
            x: self.rng.gen_range(-500.0..500.0),
            y: self.rng.gen_range(-500.0..500.0),
            z: 0.0,
        }
    }

    fn spawn_npc(&mut self) -> Entity {
        let pos = self.random_position();
        let vel = Velocity::default();
        let health = Health {
            current: 100.0,
            max: 100.0,
        };
        let ai = AiState {
            state: 0,
            timer: self.rng.gen_range(0.0..5.0),
            target_x: self.rng.gen_range(-500.0..500.0),
            target_y: self.rng.gen_range(-500.0..500.0),
        };
        let team = Team {
            id: self.rng.gen_range(0..4),
        };

        self.world
            .spawn((Npc, pos, vel, health, ai, team, Glyph("X".to_string())))
    }

    fn spawn_player(&mut self) -> Entity {
        let pos = self.random_position();
        let vel = Velocity::default();
        let health = Health {
            current: 100.0,
            max: 100.0,
        };
        let team = Team { id: 0 }; // Players on team 0

        self.world
            .spawn((Player, pos, vel, health, team, Glyph("@".to_string())))
    }

    fn spawn_static(&mut self) -> Entity {
        let pos = self.random_position();
        self.world.spawn((Static, pos, Glyph("#".to_string())))
    }

    /// Total entity count.
    pub fn total_count(&self) -> usize {
        self.world.storage().entities().spwaned_len()
    }
}

impl Default for GameWorldScenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario for GameWorldScenario {
    fn name(&self) -> &'static str {
        "game_world"
    }

    fn description(&self) -> &'static str {
        "Mixed game world with NPCs, players, projectiles, and static objects"
    }

    fn entity_count(&self) -> usize {
        self.config.npc_count
            + self.config.player_count
            + self.config.projectile_count
            + self.config.static_count
    }

    fn setup(&mut self) {
        // Spawn NPCs
        for _ in 0..self.config.npc_count {
            self.spawn_npc();
        }

        // Spawn players
        for _ in 0..self.config.player_count {
            self.spawn_player();
        }

        // Spawn projectiles
        // for _ in 0..self.config.projectile_count {
        //     self.spawn_projectile();
        // }

        // Spawn static objects
        for _ in 0..self.config.static_count {
            self.spawn_static();
        }

        self.schedule
            .add_system(Update, system_movement, &mut self.world);
        self.schedule.add_system(Update, system_ai, &mut self.world);
        self.schedule
            .add_system(Update, system_projectile_lifetime, &mut self.world);

        // self.schedule
        //     .add_system(Render, system_render_statics, &mut self.world);
        self.schedule
            .add_system(Render, system_render, &mut self.world);

        self.world.add_unique(DeltaTime(self.config.delta_time));
        // self.world.add_unique(Board::new());
    }

    fn update(&mut self) {
        self.schedule.run(Update, &mut self.world, &self.executor);
        self.schedule.run(Render, &mut self.world, &self.executor);
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
    fn game_world_scenario_setup() {
        let mut scenario = GameWorldScenario::with_config(GameWorldConfig {
            npc_count: 50,
            player_count: 5,
            projectile_count: 20,
            static_count: 25,
            ..Default::default()
        });

        scenario.setup();
        assert_eq!(scenario.total_count(), 80);

        scenario.teardown();
        assert_eq!(scenario.total_count(), 0);
    }
}
