//! Renders two 3d passes to the same window from different perspectives.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

/// Set up a simple 3D scene
fn setup(mut commands: Commands) {
    // Plane
    let plane_mesh = commands.spawn_asset(Mesh::from(Plane3d::default().mesh().size(5.0, 5.0)));
    let plane_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.3, 0.5, 0.3)));
    commands.spawn((Mesh3d(plane_mesh), MeshMaterial3d(plane_material)));

    // Cube
    let cube_mesh = commands.spawn_asset(Mesh::from(Cuboid::default()));
    let cube_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.8, 0.7, 0.6)));
    commands.spawn((
        Mesh3d(cube_mesh),
        MeshMaterial3d(cube_material),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));

    // Light
    commands.spawn((
        PointLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // camera
    commands.spawn((
        Camera3d::default(),
        Camera {
            // renders after / on top of the main camera
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::from_xyz(10.0, 10., -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
