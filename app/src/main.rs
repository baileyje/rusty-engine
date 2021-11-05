use rusty_engine::core::{logger::{ChannelLogger}, Engine, Frame, Logic, Service, context::Context, control::Control};

use rusty_cli::cli::{CliControl};

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

  fn on_update(&mut self, frame: Frame<'_, String>, ctx: Context) {
    // println!("on_update");
  }
  fn on_fixed_update(&mut self, frame: Frame<'_, String>, ctx: Context) {
    // print!(".");
    ctx.logger.info("Fixed.".into());
  }
}

fn main() {
  let service_one = TestService {};
  let (logger, log_recv) = ChannelLogger::with_receiver();
  let mut engine = Engine::new(
    String::from("foo"),
    Box::new(TestLogic {}),
    Box::new(logger),
  );
  engine.add(Box::new(service_one));
  let mut control = CliControl::new(Box::new(engine), log_recv);
  control.start();
}
