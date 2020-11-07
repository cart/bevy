use bevy::prelude::*;
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use bevy_dylib;

/// This example illustrates how to customize the thread pool used internally (e.g. to only use a
/// certain number of threads).
fn main() {
    App::build()
        .add_resource(DefaultTaskPoolOptions::with_num_threads(4))
        .add_plugins(DefaultPlugins)
        .run();
}
