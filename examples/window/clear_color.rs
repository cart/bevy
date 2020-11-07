use bevy::{prelude::*, render::pass::ClearColor};
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use bevy_dylib;

fn main() {
    App::build()
        .add_resource(ClearColor(Color::rgb(0.5, 0.5, 0.9)))
        .add_plugins(DefaultPlugins)
        .run();
}
