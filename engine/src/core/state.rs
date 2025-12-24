/// Enumeration of possible states the engine can be in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// The engine has never been started
    Dead,
    /// The engine is starting up
    Starting,
    /// The engine is running in normal operation
    Running,
    /// The engine is in the process of pausing
    Pausing,
    /// The engine is paused
    Paused,
    /// The engine is in the process of unpausing
    Unpausing,
    /// The engine is shutting down
    Stopping,
    /// The engine has stopped
    Stopped,
}

