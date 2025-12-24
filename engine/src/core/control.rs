use crossbeam::channel::Sender;

#[derive(Debug, Clone, Copy)]
pub enum EngineCommand {
    Start,
    Stop,
    Pause,
    Unpause,
}

pub struct Control {
    sender: Sender<EngineCommand>,
}

impl Control {
    pub fn new(sender: Sender<EngineCommand>) -> Self {
        Self { sender }
    }

    pub fn start(&self) {
        let _ = self.sender.send(EngineCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.sender.send(EngineCommand::Stop);
    }

    pub fn pause(&self) {
        let _ = self.sender.send(EngineCommand::Pause);
    }

    pub fn unpause(&self) {
        let _ = self.sender.send(EngineCommand::Unpause);
    }
}
