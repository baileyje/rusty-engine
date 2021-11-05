use std::{
  sync::{
    mpsc::{channel, Sender},
    Arc, Mutex,
  },
  vec::Vec,
};

use super::{
  context::Context,
  control::Controllable,
  frame::{Frame, TimeFrame},
  logger::{LogMessage, Logger, ChannelLogger},
  service::Service,
  state::State,
  thread::EngineThread,
};

/// Primary Logic for the engine. Broken into `on_update` and `on_fixed_update` . The `on_update` function will be called on every frame of the engin and has non-deterministic timing.
/// The `on_fixed_update` is called on a fixed interval for time sensitive functionality. Depending on the work performed in each phase there may be multiple updates per fixed update
/// or vice versa. There is no strong correlation between the two.
pub trait Logic {
  type Data;

  /// Called on every frame of the engine.
  fn on_update(&mut self, frame: Frame<Self::Data>, context: Context);

  /// Called on a fixed frame based on the engine's fixed update interval.
  fn on_fixed_update(&mut self, frame: Frame<Self::Data>, context: Context);
}

/// Internal protected state of the engine.
struct EngineInternal<Data> {
  state: State,
  data: Data,
  logic: Box<dyn Logic<Data = Data> + Send + Sync>,
}

impl<Data> EngineInternal<Data> {
  /// Called on every tick (loop) of the engine's primary logic thread.
  fn engine_tick<'a>(&mut self, time_frame: TimeFrame, logger: &'a dyn Logger) -> TimeFrame {
    let mut time_frame = time_frame.next();
    while time_frame.has_fixed() {
      time_frame.increment_fixed();
      self.logic.on_fixed_update(
        Frame::new(time_frame, &mut self.data),
        Context::new(logger),
      );
    }
    self.logic.on_update(
      Frame::new(time_frame, &mut self.data),
      Context::new(logger),
    );
    time_frame
  }
}

/// The engine's core structure. This structure holds all the services required for the engine to run.
pub struct Engine<Data> {
  internal: Arc<Mutex<EngineInternal<Data>>>,
  threads: Vec<EngineThread>,
  services: Vec<Box<dyn Service>>,
  logger: Box<dyn Logger>
}

impl<'a, Data: 'static> Engine<Data>
where
  Data: Send,
{
  /// Construct a new Core instance with default parameters.
  pub fn new(
    data: Data,
    logic: Box<dyn Logic<Data = Data> + Send + Sync>,
    logger: Box<dyn Logger>,
  ) -> Self {
    return Self {
      internal: Arc::new(Mutex::new(EngineInternal {
        data,
        state: State::Dead,
        logic,
      })),
      threads: Vec::<EngineThread>::new(),
      services: Vec::<Box<dyn Service>>::new(),
      logger
    };
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
    // super::logger::init();
    let mut internal = self.internal.lock().unwrap();
    self.logger.info("Revving the engine".into());
    // Start all the services
    internal.state = State::Starting;
    for service in self.services.iter_mut() {
      self
        .logger
        .info(format!("Starting Service: {}", service.name()));
      service.start().expect("Failed to start service");
    }
    internal.state = State::Running;
    self.logger.info("Launching simulation".into());
    let internal = Arc::clone(&self.internal);
    let fixed_time_step = 16_666_000;
    let mut time_frame = TimeFrame::new(fixed_time_step);

    // let thread_log_send_clone = thread_log_send.clone();
    self.threads.push(EngineThread::spawn(move |logger| {
      time_frame = internal
        .lock()
        .unwrap()
        .engine_tick(time_frame, logger);
      std::thread::sleep(std::time::Duration::from_millis(1));
    }));
    Ok(())
  }

  /// Stop the engine core.
  fn pause(&mut self) -> Result<(), &str> {
    let mut internal = self.internal.lock().unwrap();
    internal.state = State::Pausing;
    self.logger.info("Pausing the engine".into());
    // Pause the Engine Threads
    for thread in self.threads.iter_mut() {
      thread.pause();
    }
    internal.state = State::Paused;
    Ok(())
  }

  /// Stop the engine core.
  fn unpause(&mut self) -> Result<(), &str> {
    let mut internal = self.internal.lock().unwrap();
    internal.state = State::Unpausing;
    self.logger.info("Unpausing the engine".into());
    // Pause the Engine Threads
    for thread in self.threads.iter_mut() {
      thread.unpause();
    }
    internal.state = State::Running;
    Ok(())
  }

  /// Stop the engine core.
  fn stop(&mut self) -> Result<(), &str> {
    let mut internal = self.internal.lock().unwrap();
    if internal.state == State::Stopped {
      return Ok(());
    }
    internal.state = State::Stopping;

    // Kill the Engine Threads
    for thread in self.threads.iter_mut() {
      thread.stop();
    }

    self.logger.info("Killing the engine".into());
    for service in self.services.iter_mut() {
      self
        .logger
        .info(format!("Stopping service: {}", service.name()));
      service.stop().expect("Failed to stop service");
    }
    internal.state = State::Stopped;
    self.logger.info("Engine stopped".into());
    self.threads.clear();
    Ok(())
  }

  fn flush(&self) {
    for thread in self.threads.iter() {
      while let Ok(msg) = thread.log_receiver.try_recv() {
        self.logger.info(format!("{:?}", msg));
      }
    }
  }
}
