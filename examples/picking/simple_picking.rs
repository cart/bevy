//! A simple scene to demonstrate picking events for UI and mesh entities.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, MeshPickingPlugin))
        .add_systems(Startup, setup_scene)
        .run();
}

fn setup_scene(mut commands: Commands) {
    commands
        .spawn((
            Text::new("Click Me to get a box\nDrag cubes to rotate"),
            Node {
                position_type: PositionType::Absolute,
                top: percent(12),
                left: percent(12),
                ..default()
            },
        ))
        .observe(on_click_spawn_cube)
        .observe(|out: On<PointerOut>, mut texts: Query<&mut TextColor>| {
            let mut text_color = texts.get_mut(out.entity).unwrap();
            text_color.0 = Color::WHITE;
        })
        .observe(|over: On<PointerOver>, mut texts: Query<&mut TextColor>| {
            let mut color = texts.get_mut(over.entity).unwrap();
            color.0 = bevy::color::palettes::tailwind::CYAN_400.into();
        });

    // Base
    let base_mesh = commands.spawn_asset(Mesh::from(Circle::new(4.0)));
    let base_material = commands.spawn_asset(StandardMaterial::from(Color::WHITE));
    commands.spawn((
        Mesh3d(base_mesh),
        MeshMaterial3d(base_material),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
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
        Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn on_click_spawn_cube(_click: On<PointerClick>, mut commands: Commands, mut num: Local<usize>) {
    let mesh = commands.spawn_asset(Mesh::from(Cuboid::new(0.5, 0.5, 0.5)));
    let material = commands.spawn_asset(StandardMaterial::from(Color::srgb_u8(124, 144, 255)));
    commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.25 + 0.55 * *num as f32, 0.0),
        ))
        // With the MeshPickingPlugin added, you can add pointer event observers to meshes:
        .observe(on_drag_rotate);
    *num += 1;
}

fn on_drag_rotate(drag: On<PointerDrag>, mut transforms: Query<&mut Transform>) {
    if let Ok(mut transform) = transforms.get_mut(drag.entity) {
        transform.rotate_y(drag.delta.x * 0.02);
        transform.rotate_x(drag.delta.y * 0.02);
    }
}
