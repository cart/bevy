//! Shows how to create a 3D orthographic view (for isometric-look games or CAD applications).

use bevy::{camera::ScalingMode, prelude::*};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

/// set up a simple 3D scene
fn setup(mut commands: Commands) {
    // camera
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            // 6 world units per pixel of window height.
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 6.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(5.0, 5.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // plane
    let plane_mesh = commands.spawn_asset(Mesh::from(Plane3d::default().mesh().size(5.0, 5.0)));
    let plane_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.3, 0.5, 0.3)));
    commands.spawn((Mesh3d(plane_mesh), MeshMaterial3d(plane_material)));
    // cubes
    let cube_mesh = commands.spawn_asset(Mesh::from(Cuboid::default()));
    let cube_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.8, 0.7, 0.6)));
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(cube_material.clone()),
        Transform::from_xyz(1.5, 0.5, 1.5),
    ));
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(cube_material.clone()),
        Transform::from_xyz(1.5, 0.5, -1.5),
    ));
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(cube_material.clone()),
        Transform::from_xyz(-1.5, 0.5, 1.5),
    ));
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(cube_material.clone()),
        Transform::from_xyz(-1.5, 0.5, -1.5),
    ));
    // light
    commands.spawn((PointLight::default(), Transform::from_xyz(3.0, 8.0, 5.0)));
}
