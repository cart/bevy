use bevy::{input::touch::*, prelude::*};
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use bevy_dylib;

fn main() {
    App::build()
        .add_plugins(DefaultPlugins)
        .add_system(touch_event_system.system())
        .run();
}

#[derive(Default)]
struct State {
    event_reader: EventReader<TouchInput>,
}

fn touch_event_system(mut state: Local<State>, touch_events: Res<Events<TouchInput>>) {
    for event in state.event_reader.iter(&touch_events) {
        println!("{:?}", event);
    }
}
