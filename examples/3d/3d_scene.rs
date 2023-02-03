//! A simple 3D scene with light shining over a cube sitting on a plane.

// use bevy::prelude::*;

use bevy::{ecs::schedule_v3::Schedule, prelude::World};

fn main() {
    let mut x = Schedule::default();
    x.add_system(hi);
    let mut world = World::default();
    x.run(&mut world);
}

fn hi() {
    panic!("aahhh");
}

// /// set up a simple 3D scene
// fn setup(
//     mut commands: Commands,
//     mut meshes: ResMut<Assets<Mesh>>,
//     mut materials: ResMut<Assets<StandardMaterial>>,
// ) {
//     // plane
//     commands.spawn(PbrBundle {
//         mesh: meshes.add(Mesh::from(shape::Plane { size: 5.0 })),
//         material: materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
//         ..default()
//     });
//     // cube
//     commands.spawn(PbrBundle {
//         mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
//         material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
//         transform: Transform::from_xyz(0.0, 0.5, 0.0),
//         ..default()
//     });
//     // light
//     commands.spawn(PointLightBundle {
//         point_light: PointLight {
//             intensity: 1500.0,
//             shadows_enabled: true,
//             ..default()
//         },
//         transform: Transform::from_xyz(4.0, 8.0, 4.0),
//         ..default()
//     });
//     // camera
//     commands.spawn(Camera3dBundle {
//         transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
//         ..default()
//     });
//     panic!("aahhhh");
// }
