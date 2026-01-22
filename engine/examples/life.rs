use crossterm::{ExecutableCommand, QueueableCommand, cursor, event, style, terminal};
use rusty_engine::{
    core::tasks::{self},
    define_phase,
    ecs::{
        Commands, Entity, Uniq, UniqMut, World, schedule,
        system::{Consumer, Producer, Query},
    },
};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
};

use rusty_macros::{Component, Event, Unique};

const ROW_OFFSET: u16 = 12;
const MIN_X: i32 = -50;
const MAX_X: i32 = 50;
const MIN_Y: i32 = -50;
const MAX_Y: i32 = 50;

#[derive(Component, Clone)]
struct Glyph(String);

#[derive(Event, Clone)]
struct MouseDown(Position);

#[derive(Component)]
struct Cell;

#[derive(Component, PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct Position {
    x: i32,
    y: i32,
}

struct ViewPosition {
    x: u16,
    y: u16,
}

struct View(u16, u16);

impl View {
    fn in_bounds(&self, pos: &ViewPosition) -> bool {
        let (width, height) = (self.0, self.1);
        pos.x < width && pos.y < height
    }

    fn as_view_pos(&self, pos: &Position) -> Option<ViewPosition> {
        let (width, height) = (self.0, self.1);
        let center_x = width as i32 / 2;
        let center_y = height as i32 / 2;
        let x = pos.x + center_x;
        let y = pos.y + center_y;
        if x < 0 || x >= width as i32 || y < 0 || y >= height as i32 {
            return None;
        }
        Some(ViewPosition {
            x: x as u16,
            y: y as u16,
        })
    }

    fn as_world_pos(&self, pos: &ViewPosition) -> Position {
        let (width, height) = (self.0, self.1);
        let center_x = width as i32 / 2;
        let center_y = height as i32 / 2;
        Position {
            x: pos.x as i32 - center_x,
            y: pos.y as i32 - center_y,
        }
    }
}

#[derive(Unique)]
struct GameState {
    running: bool,
    rendering: bool,
    dump: bool,
    view: View,
}

fn gather_input(mut mouse: Producer<MouseDown>, mut state: UniqMut<GameState>) {
    if !event::poll(std::time::Duration::from_millis(1)).unwrap() {
        return;
    }
    event::read()
        .map(|event| match event {
            event::Event::Key(key_event) => {
                if key_event.code == crossterm::event::KeyCode::Char('q') {
                    terminal::disable_raw_mode().unwrap();
                    std::process::exit(0);
                } else if key_event.code == crossterm::event::KeyCode::Char('p') {
                    state.running = !state.running;
                } else if key_event.code == crossterm::event::KeyCode::Char('d') {
                    state.running = false;
                    state.dump = true;
                } else if key_event.code == crossterm::event::KeyCode::Char('r') {
                    state.rendering = !state.rendering;
                }
            }
            event::Event::Mouse(mouse_event) => {
                if let event::MouseEventKind::Up(_) = mouse_event.kind {
                    mouse.send(MouseDown(state.view.as_world_pos(&ViewPosition {
                        x: mouse_event.column,
                        y: mouse_event.row,
                    })));
                }
            }
            _ => {}
        })
        .ok();
}

fn handle_input(mouse: Consumer<MouseDown>, commands: Commands) {
    for event in mouse.iter() {
        commands.spawn(event.0);
    }
}

fn setup_rendeer() -> (u16, u16) {
    terminal::enable_raw_mode().unwrap();
    io::stdout()
        .queue(event::EnableMouseCapture)
        .unwrap()
        .queue(terminal::Clear(terminal::ClearType::All))
        .unwrap()
        .queue(cursor::Hide)
        .unwrap()
        .flush()
        .unwrap();
    terminal::size().unwrap()
}

fn render(query: Query<&Position>, state: Uniq<GameState>) {
    let mut stdout = io::stdout();

    // Clear the screen from the previous below row offset
    stdout
        .queue(cursor::MoveTo(0, ROW_OFFSET))
        .unwrap()
        .queue(terminal::Clear(terminal::ClearType::FromCursorDown))
        .unwrap()
        .queue(cursor::MoveTo(0, 0))
        .unwrap()
        .queue(style::Print(format!("Entities: {:?}", query.len())))
        .unwrap()
        .queue(cursor::MoveTo(20, 0))
        .unwrap()
        .queue(style::Print(format!(
            "State: {}",
            if state.running { "^" } else { "v" }
        )))
        .unwrap();

    if state.dump {
        terminal::disable_raw_mode().unwrap();
        stdout
            .queue(cursor::MoveTo(0, ROW_OFFSET))
            .unwrap()
            .flush()
            .unwrap();

        println!("Final State Dump:");
        println!("Count: {}", query.len());
        for pos in query {
            println!("Cell at: {:?}", pos);
        }
        std::process::exit(0);
    }

    if !state.rendering {
        stdout.flush().unwrap();
        return;
    }

    for pos in query {
        if let Some(view_pos) = state.view.as_view_pos(pos) {
            stdout
                .queue(cursor::MoveTo(view_pos.x, view_pos.y))
                .unwrap()
                .queue(style::Print("■"))
                .unwrap();
        }
    }

    stdout.queue(cursor::MoveTo(0, 1)).unwrap().flush().unwrap();
}

fn log_status(str: &str) {
    let mut stdout = io::stdout();
    stdout
        .queue(cursor::MoveTo(0, 1))
        .unwrap()
        .queue(style::Print(str))
        .unwrap()
        .flush()
        .unwrap();
}

/// The directions neighboring a cell can exist. Simplifies calculating neighbor positions.
const DIRECTIONS: [(i32, i32); 8] = [
    (-1, 1),
    (0, 1),
    (1, 1),
    (-1, 0),
    (1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

fn cell_neighbors(pos: &Position) -> [Position; 8] {
    DIRECTIONS.map(|(dx, dy)| Position {
        x: pos.x + dx,
        y: pos.y + dy,
    })
}

fn compute(cells: Query<(Entity, &Position)>, commands: Commands, state: Uniq<GameState>) {
    if !state.running {
        return;
    }

    // Store positions of live cells for easy lookup
    let mut live_cells = HashMap::new();

    // Create a hash set of possible cells to consider for birth
    let mut possible_cells = HashSet::<Position>::new();

    // Iterate over existing cells to calculate min/max bounds, and track all existing positions in
    // stet
    for (entity, pos) in cells {
        // Add the live cell and its neighbors to the tracking structures
        live_cells.insert(*pos, entity);
        // Add position to possible cells for survival consideration
        possible_cells.insert(*pos);
        // Collect neighbors for birth consideration
        possible_cells.extend(cell_neighbors(pos).iter());
    }

    for pos in &possible_cells {
        let mut live_neighbors = 0;
        for neighbor in cell_neighbors(pos).iter() {
            if live_cells.contains_key(neighbor) {
                live_neighbors += 1;
            }
        }
        // Apply Game of Life rules
        if let Some(live) = live_cells.get(pos) {
            // Cell is currently alive
            if !(2..=3).contains(&live_neighbors) {
                commands.despawn(*live);
            }
        } else {
            // Cell is currently dead
            if live_neighbors == 3 {
                commands.spawn((*pos, Cell));
            }
        }
    }
    // thread::sleep(std::time::Duration::from_millis(100));
}

fn main() {
    let view = setup_rendeer();

    println!("=============================================================");
    println!("Game of life!");
    println!("=============================================================");

    let mut world = World::default();

    define_phase!(PreUpdate, Update, Render);

    world.register_event::<MouseDown>();
    world.add_unique(GameState {
        running: false,
        dump: false,
        rendering: true,
        view: View(view.0, view.1 - ROW_OFFSET),
    });

    let mut schedule = schedule::Schedule::new();

    let executor = tasks::Executor::new(4);
    schedule.add_system(PreUpdate, gather_input, &mut world);
    schedule.add_system(Update, handle_input, &mut world);
    schedule.add_system(Update, compute, &mut world);
    schedule.add_system(Render, render, &mut world);

    let tick = schedule::Sequence::new()
        .then(PreUpdate)
        .then(Update)
        .then(Render);
    //
    // world.spawn((Position { x: 11, y: 11 }, Cell));
    // world.spawn((Position { x: 11, y: 12 }, Cell));
    // world.spawn((Position { x: 11, y: 13 }, Cell));

    // world.spawn((Position { x: 5, y: 5 }, Cell));
    // world.spawn((Position { x: 6, y: 5 }, Cell));
    // world.spawn((Position { x: 6, y: 6 }, Cell));
    // world.spawn((Position { x: 6, y: 7 }, Cell));
    // world.spawn((Position { x: 7, y: 7 }, Cell));
    // world.spawn((Position { x: 7, y: 8 }, Cell));
    // world.spawn((Position { x: 8, y: 8 }, Cell));

    loop {
        schedule.run_sequence(&tick, &mut world, &executor);
        world.swap_event_buffers();
    }
}
