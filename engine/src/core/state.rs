
/// Enumeration of possible states the engine can be in.
#[derive(PartialEq)]
#[derive(Debug)]
#[derive(Copy, Clone)]
pub enum State {
  /// The engine has never been started
  Dead,
  /// The engine is starting up
  Starting,
  /// The engine is running in normal operation
  Running,
  // Paused,
  /// The engine is shutting down
  Stopping,
  /// The engine has stopped
  Stopped,
}