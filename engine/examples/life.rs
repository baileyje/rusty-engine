use std::collections::HashMap;

use rusty_engine::{
    core::tasks::{self},
    define_phase,
    ecs::{
        entity, schedule,
        system::Query,
        world::{self},
    },
};
use rusty_macros::Component;

const GRID_WIDTH: usize = 50;
const GRID_HEIGHT: usize = 50;

#[derive(Component)]
struct Cell;

#[derive(Component, PartialEq, Eq, Hash, Clone, Copy)]
struct Position {
    x: i32,
    y: i32,
}

fn compute_cells(world: &mut world::World) {
    let mut new_cells = Vec::new();
    let mut to_remove = Vec::new();

    let directions = [
        (-1, 1),
        (0, 1),
        (1, 1),
        (-1, 0),
        (1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];

    let cells = world.query::<(entity::Entity, &Position, &Cell)>();

    // Store positions of live cells for easy lookup
    let mut pos_map = HashMap::new();
    let mut min_x = 0;
    let mut max_x = 0;
    let mut min_y = 0;
    let mut max_y = 0;

    // Iterate over existing cells to calculate min/max bounds, and track all existing positions in
    // stet
    for (entity, pos, _cell) in cells {
        pos_map.insert(*pos, entity);

        min_x = min_x.min(pos.x);
        max_x = max_x.max(pos.x);
        min_y = min_y.min(pos.y);
        max_y = max_y.max(pos.y);
    }

    println!(
        "Current live cells: {}, Bounds: x({}-{}), y({}-{})",
        pos_map.len(),
        min_x,
        max_x,
        min_y,
        max_y
    );

    // Iterate over an expanded grid to find new cells to add
    for x in (min_x - 1)..=(max_x + 1) {
        for y in (min_y - 1)..=(max_y + 1) {
            let current_pos = Position { x, y };
            let mut live_neighbors = 0;
            for (dx, dy) in &directions {
                let neighbor_pos = Position {
                    x: current_pos.x + dx,
                    y: current_pos.y + dy,
                };

                if pos_map.contains_key(&neighbor_pos) {
                    live_neighbors += 1;
                }
            }

            // Apply Game of Life rules
            if pos_map.contains_key(&current_pos) {
                // Cell is currently alive
                if !(2..=3).contains(&live_neighbors) {
                    to_remove.push(current_pos);
                }
            } else {
                // Cell is currently dead
                if live_neighbors == 3 {
                    new_cells.push(current_pos);
                }
            }
        }
    }

    for pos in new_cells {
        world.spawn((pos, Cell {}));
    }

    for pos in to_remove {
        if let Some(entity) = pos_map.get(&pos) {
            world.despawn(*entity);
        }
    }

    // thread::sleep(std::time::Duration::from_millis(500));
}

fn render_world(cells: Query<(&Position, &Cell)>) {
    let mut grid = vec![vec!['.'; GRID_WIDTH]; GRID_HEIGHT];

    for (pos, _) in cells {
        if pos.x >= 0 && pos.x < GRID_WIDTH as i32 && pos.y >= 0 && pos.y < GRID_HEIGHT as i32 {
            grid[GRID_HEIGHT - pos.y as usize - 1][pos.x as usize] = '#';
        }
    }

    println!("\nWorld State:");
    for row in grid {
        let line: String = row.into_iter().collect();
        println!("{}", line);
    }
}

fn main() {
    println!("=============================================================");
    println!("Game of life!");
    println!("=============================================================");

    let mut world = world::World::new(world::Id::new(0));

    define_phase!(Update, Render);

    let mut schedule = schedule::Schedule::new();

    let executor = tasks::Executor::new(4);
    schedule.add_system(Update, compute_cells, &mut world);
    schedule.add_system(Render, render_world, &mut world);

    let tick = schedule::Sequence::new().then(Update).then(Render);

    world.spawn((Position { x: 5, y: 5 }, Cell));
    world.spawn((Position { x: 5, y: 6 }, Cell));
    world.spawn((Position { x: 5, y: 7 }, Cell));

    // world.spawn((Position { x: 5, y: 5 }, Cell));
    // world.spawn((Position { x: 6, y: 5 }, Cell));
    // world.spawn((Position { x: 6, y: 6 }, Cell));
    // world.spawn((Position { x: 6, y: 7 }, Cell));
    // world.spawn((Position { x: 7, y: 7 }, Cell));
    // world.spawn((Position { x: 7, y: 8 }, Cell));
    // world.spawn((Position { x: 8, y: 8 }, Cell));

    loop {
        schedule.run_sequence(&tick, &mut world, &executor);
    }
}
