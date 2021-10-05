use std::vec::Vec;

#[derive(PartialEq)]
#[derive(Debug)]
pub enum State {
	Dead,
	Starting,
	Running,
	// Paused,
	Stopping,
	Stopped,
}

#[derive(Debug)]
pub struct Service {
	pub name: String,
	pub state: State,
}

#[derive(Debug)]
pub struct Core {
	pub state: State,
	services: Vec<Service>,
}

impl Service {
	pub fn new(name: String) -> Service {
		return Service { name, state: State::Dead };
	}
	pub fn start(&mut self) {
		// self.state = State::Starting;
		// TODO: Other things....		
		// self.state = State::Running;
	}
}

impl Core {
	pub fn new() -> Core {
		return Core { state: State::Dead, services: Vec::new() };
	}
	pub fn start(&mut self) -> bool {
		self.state = State::Starting;
		// TODO: Other things....
		println!("Staring Core....");
		for service in self.services.iter_mut() {
			service.state = State::Starting;
			println!("Starting {}...", service.name);
			service.start();
			println!("Hmmm: {:?}", service);
			service.state = State::Running;
		}
		self.state = State::Running;
		self.state == State::Running
	}
	pub fn add(&mut self, service: Service) {
		self.services.push(service);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_start() {
		let mut core = Core::new();
		assert!(core.start());
	}

	#[test]
	fn test_start_with_service() {
		let mut core = Core::new();
		let service = Service::new(String::from("Some Service"));
		core.add(service);
		assert!(core.start());
		assert!(core.services[0].state == State::Running);
	}
}
