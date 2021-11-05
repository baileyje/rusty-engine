use super::logger::Logger;

pub struct Context<'a> {
  pub logger: &'a dyn Logger
}

impl <'a> Context<'a> {
  pub fn new(logger: &'a dyn Logger) -> Self {
    return Self {
      logger
    }
  }
}