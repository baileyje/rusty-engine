use crate::core::ecs::{component, world};

mod function;
pub mod param;

pub use function::FunctionSystem;
pub use param::Param;

/// A system that can be executed on a world.
///
/// This trait is object-safe, allowing systems to be stored in a registry.
pub trait System: Send + Sync {
    /// Get the component specification for this system.
    /// Used by scheduler to order systems and detect conflicts.
    fn component_spec(&self) -> &component::Spec;

    /// Execute the system on the given world.
    ///
    /// # Safety
    ///
    /// Caller must ensure no aliasing violations occur.
    /// The scheduler is responsible for this.
    unsafe fn run(&mut self, world: &mut world::World);
}
