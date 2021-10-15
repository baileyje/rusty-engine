use engine::core::{Engine, Service};

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


fn main() {
  let service_one = TestService {};
  let mut data = String::from("foo");
  Engine::new()
    .add(Box::new(service_one))
    .start(
      &mut data,
      |f| { println!("Hmm: {}", f.data)},
      |f| {}
    )
    .expect("Failed to start core");
}
