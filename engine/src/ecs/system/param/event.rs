use crate::ecs::{
    event,
    system::{Parameter, command::CommandBuffer},
    world,
};

pub struct Producer<'w, E: event::Event> {
    stream: &'w mut event::Stream<E>,
}

impl<'w, E: event::Event> Producer<'w, E> {
    pub fn send(&mut self, event: E) {
        self.stream.send(event);
    }
}

impl<E: event::Event> Parameter for Producer<'_, E> {
    type Value<'w, 's> = Producer<'w, E>;
    type State = ();

    fn build_state(_world: &mut world::World) -> Self::State {}

    fn required_access(world: &world::World) -> world::AccessRequest {
        // Mutable access to active buffer marker
        world::AccessRequest::to_resources(&[], &[world.resources().register_event::<E>().0])
    }

    unsafe fn extract<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
        _command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's> {
        Producer {
            stream: shard
                .events_mut()
                .stream_mut::<E>()
                .expect("unable to get event stream"),
        }
    }
}

pub struct Consumer<'w, E: event::Event> {
    stream: &'w event::Stream<E>,
}

impl<'w, E: event::Event> Consumer<'w, E> {
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.stream.iter()
    }

    pub fn len(&self) -> usize {
        self.stream.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stream.is_empty()
    }
}

impl<E: event::Event> Parameter for Consumer<'_, E> {
    type Value<'w, 's> = Consumer<'w, E>;
    type State = ();

    fn build_state(_world: &mut world::World) -> Self::State {}

    fn required_access(world: &world::World) -> world::AccessRequest {
        // Immutable access to stable buffer marker
        world::AccessRequest::to_resources(&[world.resources().register_event::<E>().1], &[])
    }

    unsafe fn extract<'w, 's>(
        shard: &'w mut world::Shard<'_>,
        _state: &'s mut Self::State,
        _command_buffer: &'w CommandBuffer,
    ) -> Self::Value<'w, 's> {
        Consumer {
            stream: shard
                .events()
                .stream::<E>()
                .expect("unable to get event stream"),
        }
    }
}

#[cfg(test)]
mod tests {

    use rusty_macros::Event;

    use crate::ecs::system::Parameter;

    use super::*;

    #[derive(Event, Debug, Clone)]
    struct TestEvent;

    #[test]
    fn test_consumer_param_access() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        world.register_event::<TestEvent>();

        // When
        let access = <Consumer<TestEvent>>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(
                &[world.resources().get_event::<TestEvent>().unwrap().1],
                &[]
            )
        );
    }

    #[test]
    fn test_consumer_param_get() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        world.register_event::<TestEvent>();

        #[allow(clippy::let_unit_value)]
        let mut state = <Consumer<TestEvent>>::build_state(&mut world);
        let access = <Consumer<TestEvent>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");
        let command_buffer = CommandBuffer::new();

        // When
        let consumer =
            unsafe { <Consumer<TestEvent>>::extract(&mut shard, &mut state, &command_buffer) };

        // Then
        assert_eq!(consumer.len(), 0);
    }

    #[test]
    fn test_producer_param_access() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        world.register_event::<TestEvent>();

        // When
        let access = <Producer<TestEvent>>::required_access(&world);

        // Then
        assert_eq!(
            access,
            world::AccessRequest::to_resources(
                &[],
                &[world.resources().get_event::<TestEvent>().unwrap().0],
            )
        );
    }

    #[test]
    fn test_producer_aram_get() {
        // Given
        let mut world = world::World::new(world::Id::new(0));
        world.register_event::<TestEvent>();

        #[allow(clippy::let_unit_value)]
        let mut state = <Producer<TestEvent>>::build_state(&mut world);
        let access = <Producer<TestEvent>>::required_access(&world);
        let mut shard = world.shard(&access).expect("Failed to create shard");
        let command_buffer = CommandBuffer::new();

        // When
        let mut producer =
            unsafe { <Producer<TestEvent>>::extract(&mut shard, &mut state, &command_buffer) };

        // Then
        producer.send(TestEvent);
    }

    // NOTE: Full testing of event system parameters is done in the function system tests.
}
