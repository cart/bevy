use bevy::prelude::*;
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use bevy_dylib;

fn main() {
    App::build()
        .add_startup_system(startup_system.system())
        .add_system(normal_system.system())
        .run();
}

/// Startup systems are run exactly once when the app starts up.
/// They run right before "normal" systems run.
fn startup_system() {
    println!("startup system ran first");
}

fn normal_system() {
    println!("normal system ran second");
}
