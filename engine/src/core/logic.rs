use crate::core::context::Context;

/// The `on_fixed_update` is called on a fixed interval for time sensitive functionality. Depending on the work performed in each phase there may be multiple updates per fixed update
/// or vice versa. There is no strong correlation between the two.
pub trait Logic {
    /// Called when the simulation is initializing
    fn on_init(&mut self);

    /// Called on every frame of the engine.
    fn on_update(&mut self, ctx: Context);

    /// Called on a fixed frame based on the engine's fixed update interval.
    fn on_fixed_update(&mut self, ctx: Context);
}
