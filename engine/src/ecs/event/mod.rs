pub mod broker;
pub mod stream;

pub use broker::Broker;
pub use stream::Stream;

/// Marker trait for event types.
///
/// Events must be:
/// - `'static`: No borrowed data
/// - `Send + Sync`: Safe to share across threads
/// - `Clone`: Events may be read by multiple consumers
/// - `Debug`: For diagnostics and logging
pub trait Event: 'static + Send + Sync + Clone {}
