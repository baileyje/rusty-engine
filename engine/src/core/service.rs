use super::state::State;

#[derive(Debug)]
pub struct Service {
  pub name: String,
  pub state: State,
}

impl Service {
  pub fn new(name: String) -> Service {
    return Service {
      name,
      state: State::Dead,
    };
  }

  pub fn start(&mut self) -> Result<(), &str> {
    // TODO: Other things....
    Ok(())
  }

  pub fn stop(&mut self) -> Result<(), &str> {
    // TODO: Other things....
    Ok(())
  }

  pub fn pause(&mut self) -> Result<(), &str> {
    // TODO: Other things....
    Ok(())
  }
  pub fn unpause(&mut self) -> Result<(), &str> {
    // TODO: Other things....
    Ok(())
  }
}
