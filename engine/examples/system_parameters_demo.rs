//! Comprehensive demonstration of the system parameter design.
//!
//! This example shows:
//! - Clean system signatures without explicit lifetimes
//! - Multiple parameter types
//! - Query patterns (immutable, mutable, mixed, optional)
//! - Entity access
//! - World access

use rusty_engine::{
    core::tasks::{self},
    ecs::{
        entity, schedule,
        system::{IntoSystem, param::Query},
        world::{self},
    },
};
use rusty_macros::Component;

// ============================================================================
// Components
// ============================================================================

#[derive(Component, Debug)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Component, Debug)]
struct Velocity {
    dx: f32,
    dy: f32,
}

#[derive(Component, Debug)]
struct Health {
    current: i32,
    max: i32,
}

#[derive(Component, Debug)]
struct Player;

#[derive(Component, Debug)]
struct Enemy;

// ============================================================================
// Systems - Notice the clean signatures!
// ============================================================================

/// Simple immutable query - read positions
fn print_positions(positions: Query<&Position>) {
    println!("\n=== Positions ===");
    for (i, pos) in positions.enumerate() {
        println!("  Entity {}: ({:.1}, {:.1})", i, pos.x, pos.y);
    }
}

// /// Simple mutable query - apply friction to velocities
// fn apply_friction(velocities: Query<&mut Velocity>) {
//     for vel in velocities {
//         vel.dx *= 0.95;
//         vel.dy *= 0.95;
//     }
// }

/// Mixed query - apply velocity to position
fn apply_velocity(query: Query<(&Velocity, &mut Position)>) {
    for (vel, pos) in query {
        pos.x += vel.dx;
        pos.y += vel.dy;
    }
}

/// Multiple parameters - read positions, write velocities
fn gravity_system(positions: Query<&Position>, velocities: Query<&mut Velocity>) {
    println!("\n=== Applying Gravity ===");
    println!("  Entities with position: {}", positions.len());
    println!("  Entities with velocity: {}", velocities.len());

    for vel in velocities {
        vel.dy -= 0.5; // Gravity
    }
}

/// Optional components - heal players with health
fn healing_system(players: Query<(&Player, Option<&mut Health>)>) {
    println!("\n=== Healing Players ===");
    for (_player, maybe_health) in players {
        if let Some(health) = maybe_health {
            if health.current < health.max {
                health.current += 1;
                println!("  Healed player: {}/{}", health.current, health.max);
            }
        } else {
            println!("  Player has no health component");
        }
    }
}

/// Entity IDs - get entity handles
fn entity_system(query: Query<(entity::Entity, &Position, &Velocity)>) {
    println!("\n=== Entities ===");
    for (entity, pos, vel) in query {
        println!(
            "  {:?}: pos=({:.1}, {:.1}), vel=({:.1}, {:.1})",
            entity, pos.x, pos.y, vel.dx, vel.dy
        );
    }
}

/// Direct world access via WorldSystem - exclusive world access for spawning
///
/// WorldSystem is used when you need exclusive &mut World access.
/// It cannot be combined with queries in the same system - use separate systems
/// for querying and world mutation.
fn spawner_system(world: &mut world::World) {
    println!("\n=== Spawner ===");

    // Count enemies using a query directly on world
    let enemy_count = world.query::<&Enemy>().count();
    println!("  Current enemies: {}", enemy_count);

    if enemy_count < 3 {
        println!("  Spawning new enemy!");
        world.spawn((
            Enemy,
            Position { x: 100.0, y: 100.0 },
            Velocity { dx: -1.0, dy: 0.0 },
        ));
    }
}

/// Complex query - player-enemy interaction
fn collision_system(
    players: Query<(&Player, &Position, &mut Health)>,
    enemies: Query<(&Enemy, &Position)>,
) {
    println!("\n=== Collision Detection ===");

    // Collect enemy positions first (can't have two active queries at once)
    let enemy_positions: Vec<(f32, f32)> = enemies.map(|(_enemy, pos)| (pos.x, pos.y)).collect();

    for (_player, player_pos, health) in players {
        for (enemy_x, enemy_y) in &enemy_positions {
            let dx = player_pos.x - enemy_x;
            let dy = player_pos.y - enemy_y;
            let distance = (dx * dx + dy * dy).sqrt();

            if distance < 10.0 {
                health.current -= 1;
                println!(
                    "  Player hit! Distance: {:.1}, Health: {}",
                    distance, health.current
                );
            }
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("=============================================================");
    println!("System Parameters Demo");
    println!("=============================================================");

    let mut world = world::World::new(world::Id::new(0));
    let mut update_phase = schedule::Phase::new();
    let mut final_phase = schedule::Phase::new();
    let executor = tasks::Executor::new(4);

    // Spawn player
    println!("\nSpawning entities...");
    world.spawn((
        Player,
        Position { x: 0.0, y: 0.0 },
        Velocity { dx: 2.0, dy: 1.0 },
        Health {
            current: 100,
            max: 100,
        },
    ));

    // Spawn enemies
    world.spawn((
        Enemy,
        Position { x: 50.0, y: 50.0 },
        Velocity { dx: -1.0, dy: 0.5 },
    ));

    world.spawn((
        Enemy,
        Position { x: -30.0, y: 20.0 },
        Velocity { dx: 0.5, dy: -0.5 },
    ));

    world.spawn((
        Enemy,
        Position { x: 4.0, y: 2.0 },
        Velocity { dx: 0.5, dy: -0.5 },
    ));

    // Spawn entity with just position (no velocity)
    world.spawn(Position { x: 10.0, y: 10.0 });

    println!("Spawned entities");

    // Create systems using IntoSystem
    update_phase.add_system(print_positions.into_system(&mut world));
    update_phase.add_system(apply_velocity.into_system(&mut world));
    update_phase.add_system(apply_velocity.into_system(&mut world));
    update_phase.add_system(gravity_system.into_system(&mut world));
    update_phase.add_system(healing_system.into_system(&mut world));
    update_phase.add_system(entity_system.into_system(&mut world));
    update_phase.add_system(spawner_system.into_system(&mut world)); // Exclusive world access
    update_phase.add_system(collision_system.into_system(&mut world));

    final_phase.add_system(entity_system.into_system(&mut world));
    final_phase.add_system(print_positions.into_system(&mut world));

    // Run simulation for 3 frames
    for frame in 0..3 {
        println!("\n");
        println!("=============================================================");
        println!("Frame {}", frame);
        println!("=============================================================");

        update_phase.run(&mut world, &executor);

        // let access = AccessRequest::to_components(
        //     <Position>::into_spec(world.components()),
        //     component::Spec::EMPTY,
        // );

        // let mut shard1 = world.shard(&access).unwrap();
        // unsafe {
        //     // Display state
        //     print_sys.run_parallel(&mut shard1);
        //
        //     world.release_shard(shard1);
        //
        //     entity_sys.run(&mut world);
        //
        //     // Game logic
        //     gravity_sys.run(&mut world);
        //     friction_sys.run(&mut world);
        //     velocity_sys.run(&mut world);
        //     collision_sys.run(&mut world);
        //     healing_sys.run(&mut world);
        //
        //     // Spawning
        //     spawner_sys.run(&mut world);
        // }
    }

    println!("\n");
    println!("=============================================================");
    println!("Final State");
    println!("=============================================================");
    final_phase.run(&mut world, &executor);

    println!("\n=============================================================");
    println!("Demo complete!");
    println!("=============================================================");
}
