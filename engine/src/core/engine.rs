use std::vec::Vec;

use crossbeam::channel::Receiver;
use log::{info, warn};

use crate::core::context::Context;
use crate::core::control::{Control, EngineCommand};
use crate::core::service::NoOpService;
use crate::core::time::ONE_FPS;
use crate::core::{Logic, Time};

use super::runner::{no_op, Runner};
use super::{service::Service, state::State};

/// The engine's core structure. This structure holds all the services required for the engine to run.
pub struct Engine {
    state: State,
    services: Vec<Box<dyn Service>>,
    runner: Runner,
    time: Time,
    // Optional receiver for control commands.
    control_receiver: Option<Receiver<EngineCommand>>,
    // Not sure this belongs in the engine, track the usage to see if
    // there is a more sensical place.
    pub logic: Box<dyn Logic>,
}

impl Engine {
    /// Construct a new Core instance with default parameters.
    pub fn new(runner: Runner, logic: Box<dyn Logic>) -> Self {
        Self {
            state: State::Dead,
            services: Vec::<Box<dyn Service>>::new(),
            runner,
            // TODO: How would we want to configure this?
            time: Time::new(ONE_FPS),
            control_receiver: None,
            logic,
        }
    }

    /// Add a new service to the engine.
    pub fn add(&mut self, service: Box<dyn Service>) -> &mut Engine {
        self.services.push(service);
        self
    }

    /// Get the state of the engine.
    pub fn state(&self) -> State {
        self.state
    }

    /// Start the engine. Will delegate to all services startup methods before invoking the runner.
    pub fn start(&mut self) -> Result<(), &str> {
        info!("Revving the engine");
        // Start all the services. Temporarily replace them with NoOpService to get mutable access.
        self.state = State::Starting;
        for i in 0..self.services.len() {
            let mut service = std::mem::replace(&mut self.services[i], Box::new(NoOpService));
            info!("Starting Service: {}", service.name());
            service.start(self).expect("Failed to start service");
            self.services[i] = service;
        }
        self.state = State::Running;
        // Invoke the runner. This will likely block the thread until some outside influence stops
        // the service. For now this may not be possible since the runner will own the engine.
        let runner = std::mem::replace(&mut self.runner, Box::new(no_op));
        runner(self);
        Ok(())
    }

    /// Update the engine core. This will advance the time frame and invoke the logic updates.
    pub fn update(&mut self) -> bool {
        // If we have a control receiver, check for any commands.
        if let Some(receiver) = &self.control_receiver {
            // Process any pending control commands.
            while let Ok(cmd) = receiver.try_recv() {
                info!("Received engine command: {:?}", cmd);
                if let EngineCommand::Stop = cmd {
                    self.stop().expect("Failed to stop engine");
                    return false;
                } else {
                    warn!("Unknown engine command received: {:?}", cmd);
                }
            }
        }

        // Make sure we are still in a running state.
        if self.state != State::Running {
            return false;
        }
        // Advance the engine time.
        self.time = self.time.next();
        // If there are fixed updates to process, do so now.
        while self.time.has_fixed() {
            self.time.increment_fixed();
            self.logic.on_fixed_update(Context::new(self.time));
        }
        // Run th per-frame update.
        self.logic.on_update(Context::new(self.time));
        true
    }

    /// Stop the engine core.
    pub fn stop(&mut self) -> Result<(), &str> {
        if self.state == State::Stopped {
            return Ok(());
        }
        self.state = State::Stopping;

        info!("Killing the engine");
        for i in 0..self.services.len() {
            let mut service = std::mem::replace(&mut self.services[i], Box::new(NoOpService));
            info!("Stopping Service: {}", service.name());
            service.stop(self).expect("Failed to stop service");
            self.services[i] = service;
        }
        self.state = State::Stopped;
        info!("Engine stopped");
        Ok(())
    }

    pub fn control(&mut self) -> Control {
        let (send, recv) = crossbeam::channel::unbounded();
        self.control_receiver = Some(recv);
        Control::new(send)
    }
}
