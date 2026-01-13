//! System registry for storing and retrieving ECS systems.
//!
//! The [`Registry`] provides a central location for registering systems that can later
//! be scheduled and executed by a scheduler.

use crate::ecs::system::{Id, System};

/// A registry for storing and managing ECS systems.
///
/// The registry assigns unique [`Id`]s to systems when they are registered,
/// allowing them to be retrieved later for execution or inspection.
///
/// # Examples
///
/// ```rust,ignore
/// use rusty_engine::ecs::system::{registry::Registry, function::Wrapper};
///
/// let mut registry = Registry::new();
///
/// fn my_system(query: query::Result<&Position>) {
///     // System logic
/// }
///
/// let system = Wrapper::new(&mut world, my_system);
/// let id = registry.register(system);
///
/// // Later, retrieve the system
/// if let Some(system) = registry.get(id) {
///     println!("System access: {:?}", system.required_access());
/// }
/// ```
#[derive(Default)]
pub struct Registry {
    /// All registered systems, indexed by their [`Id`].
    systems: Vec<System>,
}

impl Registry {
    /// Create a new, empty system registry.
    #[inline]
    pub const fn new() -> Self {
        Self {
            systems: Vec::new(),
        }
    }

    /// Register a system and return its unique identifier.
    ///
    /// The system is stored and can be retrieved later using the returned [`Id`].
    ///
    /// # Parameters
    ///
    /// - `system`: The system to register (must implement [`System`])
    ///
    /// # Returns
    ///
    /// A unique [`Id`] that can be used to retrieve the system.
    #[inline]
    pub fn register(&mut self, system: System) -> Id {
        let id = Id(self.systems.len() as u32);

        self.systems.push(system);

        id
    }

    /// Retrieve a system by its identifier.
    ///
    /// # Parameters
    ///
    /// - `id`: The identifier returned from [`register`](Self::register)
    ///
    /// # Returns
    ///
    /// `Some(&System)` if a system with the given ID exists, `None` otherwise.
    #[inline]
    pub fn get(&self, id: Id) -> Option<&System> {
        self.systems.get(id.index())
    }
}
