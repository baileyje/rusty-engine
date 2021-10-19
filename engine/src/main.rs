use engine::core::Control;
use engine::core::{CliControl, Engine, Frame, Logic, Service};

struct TestService {}

impl Service for TestService {
  fn name(&self) -> String {
    "first".into()
  }

  fn start(&mut self) -> Result<(), &str> {
    Ok(())
  }

  fn stop(&mut self) -> Result<(), &str> {
    Ok(())
  }
}

struct TestLogic {}

impl Logic for TestLogic {
  type Data = String;

  fn on_update(&mut self, frame: Frame<'_, String>) {
    // println!("on_update");
  }
  fn on_fixed_update(&mut self, frame: Frame<'_, String>) {
    print!(".");
  }
}

fn main() {
  let service_one = TestService {};
  let mut engine = Engine::new(String::from("foo"), Box::new(TestLogic {}));
  engine.add(Box::new(service_one));

  let mut control = CliControl::new(Box::new(engine));
  control.start();
}
