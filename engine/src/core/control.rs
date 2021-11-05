
use super::state::State;

pub trait Control {

  fn start(&mut self);

}

/// Trait describing a controllable component. This will generally be an Engine instance.
pub trait EngineControl {
  /// Start the component.
  fn start(&mut self) -> Result<(), &str>;
  /// Pause the component.
  fn pause(&mut self) -> Result<(), &str>;
  /// Unpause the component.
  fn unpause(&mut self) -> Result<(), &str>;
  /// Stop the component.
  fn stop(&mut self) -> Result<(), &str>;
  /// Get the current component state.
  fn state(&self) -> State;

  /// Flush any log data
  fn flush(&self);
}