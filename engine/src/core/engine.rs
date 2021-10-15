use log::info;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::vec::Vec;

use super::control::Control;
use super::frame::Frame;
use super::service::Service;
use super::sim_loop::SimLoop;
use super::state::State;


/// The engine's core structure. This structure holds all the services required for the engine to run.
pub struct Engine {
  pub state: State,
  services: Vec<Box<dyn Service>>,
}

impl Engine {
  /// Construct a new Core instance with default parameters.
  pub fn new() -> Self {
    // let (sender, receiver) = channel();
    return Self {
      state: State::Dead,
      services: Vec::<Box<dyn Service>>::new(),
    };
  }

  /// Start the engine. Will delegate to all services startup methods. Once service startup is complete the work threads (game, render, ...) will be started.
  pub fn start<Data, UF, FUF>(
    &mut self,
    data: &mut Data,
    on_update: UF,
    on_fixed_update: FUF,
  ) -> Result<(), &str>
  where
    Data: Send + Sync,
    UF: Fn(Frame<Data>) -> () + Send,
    FUF: Fn(Frame<Data>) -> () + Send,
  {
    super::logger::init();
    info!("Revving the engine");
    // Start all the services
    self.state = State::Starting;
    for service in self.services.iter_mut() {
      // service.state = State::Starting;
      info!("Starting: {}", service.name());
      service.start().expect("Failed to start service");
      // service.state = State::Running;
    }
    self.state = State::Running;
    info!("Launching simulation");
    crossbeam::thread::scope(|scope| {
      let stop_handle = Arc::new(AtomicBool::new(false));
      // TODO: Start the control
      let control_stop_handle = stop_handle.clone();
      let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, stop_handle.clone());
      scope
        .builder()
        .name("EngineControl".into())
        .spawn(move |_| {
          Control::start(control_stop_handle);
        })
        .unwrap();
      let sim_stop_handle = stop_handle.clone();
      scope
        .builder()
        .name("EngineSimulation".into())
        .spawn(move |_| {
          SimLoop::start(
            16_666_000,
            data,
            on_update,
            on_fixed_update,
            sim_stop_handle
          );
        })
        .unwrap();
      // TODO: Start the render loop
    })
    .unwrap();
    self.stop()
  }
  /// Stop the engine core.
  pub fn stop(&mut self) -> Result<(), &str> {
    self.state = State::Stopping;
    info!("Killing the engine");
    for service in self.services.iter_mut() {
      // service.state = State::Stopping;
      info!("Stopping: {}", service.name());
      service.stop().expect("Failed to stop service");
      // service.state = State::Stopped;
    }
    self.state = State::Stopped;
    info!("Engine stopped");
    Ok(())
  }

  pub fn add(&mut self, service: Box<dyn Service>) -> &mut Engine {
    self.services.push(service);
    self
  }

}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_start() {
    // let mut core = Engine::<String>::new(String::from("foo"));
    // core.start().expect("Failed to start core");
    // assert!(core.state == State::Running);
  }

  #[test]
  fn test_start_with_service() {
    // let mut core = Engine::<String>::new(String::from("foo"));
    // let service = Service::new(String::from("Some Service"));
    // core.add(service);
    // core.start().expect("Failed to start core");
    // assert!(core.services[0].state == State::Running);
  }

  #[test]
  fn test_start_with_two_services() {
    // let mut core = Engine::<String>::new(String::from("foo"));
    // let service_one = Service::new(String::from("First Service"));
    // let service_two = Service::new(String::from("Second Service"));
    // core.add(service_one).add(service_two);
    // core.start().expect("Failed to start core");
    // assert!(core.services[0].state == State::Running);
    // assert!(core.services[1].state == State::Running);
  }
}
