use log::{info, set_boxed_logger, set_max_level, Level};
use rusty_engine::core::{
    context::Context, log::ChannelLogger, runner::looped, Engine, Logic, Service,
};

use rusty_cli::cli::CliControl;

struct TestService {}

impl Service for TestService {
    fn start(&mut self, _engine: &mut Engine) -> Result<(), &str> {
        Ok(())
    }

    fn stop(&mut self, _engine: &mut Engine) -> Result<(), &str> {
        Ok(())
    }
}

struct TestLogic {}

impl Logic for TestLogic {
    fn on_init(&mut self) {
        // ctx.logger.info(format!("{}\r\n", data));
        // *data = String::from("foo")
    }

    fn on_update(&mut self, ctx: Context) {
        info!("Running Update: {}", ctx.time.time.as_millis());
        std::thread::sleep(std::time::Duration::from_millis(500))
    }

    fn on_fixed_update(&mut self, ctx: Context) {
        info!("Running Fixed: {}", ctx.time.fixed_time.as_millis());
    }
}

fn main() {
    let (logger, log_recv) = ChannelLogger::with_receiver();

    set_boxed_logger(Box::new(logger)).expect("Failed to set logger");
    set_max_level(Level::Info.to_level_filter());

    let runner = looped;
    let mut engine = Engine::new(Box::new(runner), Box::new(TestLogic {}));
    engine.add(Box::new(TestService {}));
    let control = engine.control();
    let mut cli = CliControl::new(log_recv, control);

    let cli_handle = std::thread::spawn(move || {
        cli.run().unwrap();
    });

    engine.start().unwrap();

    cli_handle.join().unwrap();
}
