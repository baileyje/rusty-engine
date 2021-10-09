use engine::core::Core;
use engine::core::service::Service;

fn main() {
  
  let core = Core::new();
  let service_one = Service::new(String::from("First Service"));
  core.add(service_one);
  core.start().expect("Failed to start core");
  
}