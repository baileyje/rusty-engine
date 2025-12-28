// Example demonstrating the Cell/CellMut usage pattern for ECS tables
//
// This shows how cells serve as the primary interface for type-safe access
// to type-erased columnar storage.

use rusty_engine::core::ecs::{component, storage::Column};
use rusty_macros::Component;

#[derive(Component, Debug, Clone, Copy)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Component, Debug)]
struct Velocity {
    dx: f32,
    dy: f32,
}

fn example_cell_usage() {
    let mut registry = component::Registry::new();
    let pos_id = registry.register::<Position>();
    let vel_id = registry.register::<Velocity>();

    let mut pos_column = Column::new(registry.get_by_id(pos_id).unwrap());
    let mut vel_column = Column::new(registry.get_by_id(vel_id).unwrap());

    // === WRITING DATA (CellMut pattern) ===

    // Reserve space for components
    pos_column.reserve(2);
    vel_column.reserve(2);

    // Write to reserved memory using CellMut, then mark as initialized
    unsafe {
        let mut cell = pos_column.cell_mut(0.into());
        cell.write(Position { x: 1.0, y: 2.0 });

        let mut cell = vel_column.cell_mut(0.into());
        cell.write(Velocity { dx: 0.5, dy: 0.3 });

        let mut cell = pos_column.cell_mut(1.into());
        cell.write(Position { x: 3.0, y: 4.0 });
        
        let mut cell = vel_column.cell_mut(1.into());
        cell.write(Velocity { dx: -0.2, dy: 0.8 });

        // After all writes, mark as initialized
        pos_column.set_len(2);
        vel_column.set_len(2);
    }

    // === READING DATA (Cell pattern) ===

    // Cell is Copy, so you can use it multiple times
    let cell = pos_column.cell(0.into());
    let pos1: &Position = cell.as_ref();
    let pos2: &Position = cell.as_ref(); // Can reuse!
    
    println!("Position 0: ({}, {})", pos1.x, pos1.y);
    assert_eq!(pos1.x, pos2.x); // Both point to same data

    // === MUTATING DATA (CellMut pattern) ===

    // CellMut is NOT Copy - it's consumed when dereferenced
    unsafe {
        let cell = pos_column.cell_mut(0.into());
        let pos: &mut Position = cell.as_mut();  // Consumes cell
        // let pos2 = cell.as_mut(); // ❌ Compile error: cell was moved
        pos.x += 10.0;
    }

    // Verify mutation
    let cell = pos_column.cell(0.into());
    let pos: &Position = cell.as_ref();
    println!("Position after mutation: ({}, {})", pos.x, pos.y);
    assert_eq!(pos.x, 11.0);

    // === WHY THIS DESIGN? ===
    
    // 1. Type erasure: Columns store raw bytes, cells provide typed access
    // 2. Safety: Cell (Copy) for reads, CellMut (not Copy) for writes
    // 3. Ergonomics: Reuse Cell for multiple reads without reborrowing
    // 4. Cache-friendly: Columnar storage for efficient iteration
}

fn main() {
    example_cell_usage();
    println!("\n✅ Cell/CellMut pattern demonstration complete");
}
