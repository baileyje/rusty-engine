use log::info;
use std::sync::{Arc, Mutex};
use std::vec::Vec;

use super::control::Controllable;
use super::frame::{Frame, TimeFrame};
use super::service::Service;
use super::state::State;
use super::thread::EngineThread;

/// Primary Logic for the engine. Broken into `on_update` and `on_fixed_update` . The `on_update` function will be called on every frame of the engin and has non-deterministic timing.
/// The `on_fixed_update` is called on a fixed interval for time sensitive functionality. Depending on the work performed in each phase there may be multiple updates per fixed update
/// or vice versa. There is no strong correlation between the two.
pub trait Logic {
  type Data;

  /// Called on every frame of the engine.
  fn on_update(&mut self, frame: Frame<Self::Data>);

  /// Called on a fixed frame based on the engine's fixed update interval.
  fn on_fixed_update(&mut self, frame: Frame<Self::Data>);
}

/// Internal protected state of the engine.
struct EngineInternal<Data> {
  state: State,
  data: Data,
  logic: Box<dyn Logic<Data = Data> + Send + Sync>,
}

impl<Data> EngineInternal<Data> {
  /// Called on every tick (loop) of the engine's primary logic thread.
  fn engine_tick(&mut self, time_frame: TimeFrame) -> TimeFrame {
    let mut time_frame = time_frame.next();
    while time_frame.has_fixed() {
      time_frame.increment_fixed();
      self
        .logic
        .on_fixed_update(Frame::new(time_frame, &mut self.data));
    }
    self.logic.on_update(Frame::new(time_frame, &mut self.data));
    time_frame
  }
}

/// The engine's core structure. This structure holds all the services required for the engine to run.
pub struct Engine<Data> {
  internal: Arc<Mutex<EngineInternal<Data>>>,
  threads: Vec<EngineThread>,
  services: Vec<Box<dyn Service>>,
}

impl<'a, Data: 'static> Engine<Data>
where
  Data: Send,
{
  /// Construct a new Core instance with default parameters.
  pub fn new(
    data: Data,
    logic: Box<dyn Logic<Data = Data> + Send + Sync>,
  ) -> Self {
    return Self {
      internal: Arc::new(Mutex::new(EngineInternal {
        data,
        state: State::Dead,
        logic,
      })),
      threads: Vec::<EngineThread>::new(),
      services: Vec::<Box<dyn Service>>::new(),
    };
  }

  /// Change the internal engine state.
  fn change_state(&mut self, new_state: State) {
    self.internal.lock().unwrap().state = new_state;
  }

  /// Add a new service to the engine.
  pub fn add(&mut self, service: Box<dyn Service>) -> &mut Engine<Data> {
    self.services.push(service);
    self
  }

  /// Join all the engine threads to ensure the outer thread waits for the engines execution.
  pub fn join(&mut self) {
    for thread in self.threads.iter_mut() {
      thread.join();
    }
  }
}

impl<'a, Data: 'static> Controllable for Engine<Data>
where
  Data: Send,
{
  fn state(&self) -> State {
    self.internal.lock().unwrap().state.clone()
  }

  /// Start the engine. Will delegate to all services startup methods. Once service startup is complete the work threads (game, render, ...) will be started.
  fn start(&mut self) -> Result<(), &str> {
    super::logger::init();
    info!("Revving the engine");
    // Start all the services
    self.change_state(State::Starting);
    for service in self.services.iter_mut() {
      info!("Starting: {}", service.name());
      service.start().expect("Failed to start service");
    }
    self.change_state(State::Running);
    info!("Launching simulation");
    let internal = Arc::clone(&self.internal);
    let fixed_time_step = 16_666_000;
    let mut time_frame = TimeFrame::new(fixed_time_step);
    self.threads.push(EngineThread::spawn(move || {
      time_frame = internal.lock().unwrap().engine_tick(time_frame);
      std::thread::sleep(std::time::Duration::from_millis(1));
    }));
    Ok(())
  }


  /// Stop the engine core.
  fn pause(&mut self) -> Result<(), &str> {
    // Pause the Engine Threads
    for thread in self.threads.iter_mut() {
      thread.pause();
    }
    Ok(())
  }

  /// Stop the engine core.
  fn stop(&mut self) -> Result<(), &str> {
    self.change_state(State::Stopping);

    // Kill the Engine Threads
    for thread in self.threads.iter_mut() {
      thread.stop();
    }

    info!("Killing the engine");
    for service in self.services.iter_mut() {
      info!("Stopping: {}", service.name());
      service.stop().expect("Failed to stop service");
    }
    self.change_state(State::Stopped);
    info!("Engine stopped");
    Ok(())
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
