use bevy::prelude::*;
#[allow(unused_imports)]
#[allow(clippy::single_component_path_imports)]
use bevy_dylib;

fn main() {
    App::build().add_system(hello_world_system.system()).run();
}

fn hello_world_system() {
    println!("hello world");
}
