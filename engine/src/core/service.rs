
/// Does this make sense?
pub trait Service: Send + Sync {
  fn name(&self) -> String;
  fn start(&mut self) -> Result<(), &str>;
  fn stop(&mut self) -> Result<(), &str>;
}
