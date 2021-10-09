use log::info;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use std::vec::Vec;

pub mod frame;
mod logger;
pub mod service;
mod state;

use frame::Frame;
use service::Service;
use state::State;

#[derive(Debug)]
struct Internal {
  pub state: State,
  services: Vec<Service>,
}

/// The engine's core structure. This structure holds all the services required for the engine to run.
#[derive(Debug)]
pub struct Core {
  // Split out the internal state to share across threads
  internal: Arc<Mutex<Internal>>,
}

impl Core {
  /// Construct a new Core instance with default parameters.
  pub fn new() -> Self {
    return Self {
      internal: Arc::new(Mutex::new(Internal {
        state: State::Dead,
        services: Vec::<Service>::new(),
      })),
    };
  }

  /// Start the engine. Will delegate to all services startup methods. Once service startup is complete the work threads (game, render, ...) will be started.
  pub fn start(&self) -> Result<(), &str> {
    logger::init();
    info!("Revving the engine!");
    let mut internals = self.internal.lock().unwrap();
    // Start all the services
    internals.state = State::Starting;
    for service in internals.services.iter_mut() {
      service.state = State::Starting;
      info!("Starting {}...", service.name);
      service.start().expect("Failed to start service");
      service.state = State::Running;
    }
    // Start the main game loop
    let main_handle = self.start_main_loop();
    // Start a control thread to take signals
    let control_handle = self.start_control_loop();

    // TODO: Start the main render loop
    internals.state = State::Running;
    drop(internals); // DROP our guarded reference
    main_handle.join().unwrap();
    control_handle.join().unwrap();
    Ok(())
  }
  /// Stop the engine core.
  pub fn stop(&self) -> Result<(), &str> {
    self.internal.lock().unwrap().state = State::Stopping;
    info!("Killing the engine....");
    for service in self.internal.lock().unwrap().services.iter_mut() {
      service.state = State::Stopping;
      info!("Stopping {}...", service.name);
      service.stop().expect("Failed to stop service");
      service.state = State::Stopped;
    }
    self.internal.lock().unwrap().state = State::Stopped;
    Ok(())
  }

  pub fn add(&self, service: Service) -> &Core {
    self.internal.lock().unwrap().services.push(service);
    self
  }

  fn start_main_loop(&self) -> thread::JoinHandle<()> {
    let internal = self.internal.clone();
    let fixed_time_step = 17;
    thread::Builder::new()
      .name("EngineMain".into())
      .spawn(move || {
        let mut frame = Frame::new();
        let mut accumulator = 0;
        let mut last_state = State::Starting;
        while last_state == State::Starting || last_state == State::Running {
          let internal = internal.lock().unwrap();
          frame = frame.next();
          accumulator += frame.delta.as_millis();
          while accumulator >= fixed_time_step {
            // println!("Running fixed.... {}", frame.time.as_millis());
            print!(".");
            accumulator -= fixed_time_step;
          }
          last_state = internal.state;
          // TODO: Remove when real work exists....
          thread::sleep(time::Duration::from_millis(1));
        }
      })
      .unwrap()
  }

  fn start_control_loop(&self) -> thread::JoinHandle<()> {
    let internal = self.internal.clone();
    thread::Builder::new()
      .name("EngineControl".into())
      .spawn(move || {
        let mut command = String::new();
        io::stdin()
          .read_line(&mut command)
          .ok()
          .expect("Failed to read line");
        println!("Command..... {}", command);
        let command = command.trim();
        if command == "stop" {
          println!("Stopping....");
          internal.lock().unwrap().state = State::Stopped;
        }
      })
      .unwrap()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_start() {
    let mut core = Core::new();
    core.start().expect("Failed to start core");
    // assert!(core.state == State::Running);
  }

  #[test]
  fn test_start_with_service() {
    let mut core = Core::new();
    let service = Service::new(String::from("Some Service"));
    core.add(service);
    core.start().expect("Failed to start core");
    // assert!(core.services[0].state == State::Running);
  }

  #[test]
  fn test_start_with_two_services() {
    let mut core = Core::new();
    let service_one = Service::new(String::from("First Service"));
    let service_two = Service::new(String::from("Second Service"));
    core.add(service_one).add(service_two);
    core.start().expect("Failed to start core");
    // assert!(core.services[0].state == State::Running);
    // assert!(core.services[1].state == State::Running);
  }
}
