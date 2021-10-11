use engine::core::{Engine, Service};

fn main() {
  let service_one = Service::new("First Service".into());
  Engine::new()
    .add(service_one)
    .start(
      String::from("foo"),
      |frame, data| {
        println!("Update: {} -> {}", frame.time.as_millis(), data);
      },
      |frame, data| {
        println!("Fixed Update: {} -> {}", frame.fixed_time.as_millis(), data);
      },
    )
    .expect("Failed to start core");
}
