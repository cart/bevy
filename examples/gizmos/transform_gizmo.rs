//! Interactive transform gizmo example.
//!
//! Demonstrates translate, rotate, and scale gizmos with click-to-select.
//! - Click an object to select it (primary mouse button)
//! - **1** = Translate, **2** = Rotate, **3** = Scale
//! - **X** = Toggle World/Local space

use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    gizmos::transform_gizmo::{
        TransformGizmoCamera, TransformGizmoFocus, TransformGizmoMode, TransformGizmoPlugin,
        TransformGizmoSettings, TransformGizmoSpace,
    },
    picking::{pointer::PointerButton, Pickable},
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            FreeCameraPlugin,
            MeshPickingPlugin,
            TransformGizmoPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (gizmo_mode_keys, update_instructions))
        .run();
}

fn setup(mut commands: Commands) {
    // Instructions
    commands.spawn((
        Text::new(
            "Click an object to select it\n1: Translate | 2: Rotate | 3: Scale | X: World/Local space",
        ),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
        InstructionsText,
    ));

    // Ground plane (not pickable)
    let plane_mesh = commands.spawn_asset(Mesh::from(Plane3d::default().mesh().size(10.0, 10.0)));
    let plane_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.3, 0.3, 0.3)));
    commands.spawn((
        Mesh3d(plane_mesh),
        MeshMaterial3d(plane_material),
        Pickable::IGNORE,
    ));

    // Table: a parent body with a child part, demonstrating local vs world space.
    // The parent cube is selected by default.
    let tabletop_mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.5, 0.15, 1.0)));
    let leg_mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.5, 0.15, 1.0)));

    let tabletop_material =
        commands.spawn_asset(StandardMaterial::from(Color::srgb(0.8, 0.3, 0.3)));
    let leg_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.6, 0.2, 0.2)));
    commands
        .spawn((
            Mesh3d(tabletop_mesh),
            MeshMaterial3d(tabletop_material),
            Transform::from_xyz(-2.0, 1.0, 0.0),
            TransformGizmoFocus,
        ))
        .observe(on_click_select)
        .with_children(|parent| {
            // Table leg (child)
            parent.spawn((
                Mesh3d(leg_mesh.clone()),
                MeshMaterial3d(leg_material.clone()),
                Transform::from_xyz(-0.6, -0.5, 0.4),
                Pickable::IGNORE,
            ));
            parent.spawn((
                Mesh3d(leg_mesh.clone()),
                MeshMaterial3d(leg_material.clone()),
                Transform::from_xyz(0.6, -0.5, 0.4),
                Pickable::IGNORE,
            ));
            parent.spawn((
                Mesh3d(leg_mesh.clone()),
                MeshMaterial3d(leg_material.clone()),
                Transform::from_xyz(-0.6, -0.5, -0.4),
                Pickable::IGNORE,
            ));
            parent.spawn((
                Mesh3d(leg_mesh.clone()),
                MeshMaterial3d(leg_material.clone()),
                Transform::from_xyz(0.6, -0.5, -0.4),
                Pickable::IGNORE,
            ));
        });

    // Standalone cube
    let cube_mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let cube_material = commands.spawn_asset(StandardMaterial::from(Color::srgb(0.3, 0.8, 0.3)));
    commands
        .spawn((
            Mesh3d(cube_mesh),
            MeshMaterial3d(cube_material),
            Transform::from_xyz(2.0, 0.5, 0.0),
        ))
        .observe(on_click_select);

    // Light
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        FreeCamera::default(),
        TransformGizmoCamera,
    ));
}

fn on_click_select(
    click: On<Pointer<Click>>,
    mut commands: Commands,
    existing: Query<Entity, With<TransformGizmoFocus>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    // Remove focus from all entities
    for e in &existing {
        commands.entity(e).remove::<TransformGizmoFocus>();
    }
    // Add focus to clicked entity
    commands.entity(click.entity).insert(TransformGizmoFocus);
}

// Note: Using 1/2/3 instead of Blender's G/R/S because S conflicts with
// the FreeCameraPlugin's WASD movement controls.
fn gizmo_mode_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if keyboard.just_pressed(KeyCode::Digit1) {
        settings.mode = TransformGizmoMode::Translate;
    }
    if keyboard.just_pressed(KeyCode::Digit2) {
        settings.mode = TransformGizmoMode::Rotate;
    }
    if keyboard.just_pressed(KeyCode::Digit3) {
        settings.mode = TransformGizmoMode::Scale;
    }
    if keyboard.just_pressed(KeyCode::KeyX) {
        settings.space = match settings.space {
            TransformGizmoSpace::World => TransformGizmoSpace::Local,
            TransformGizmoSpace::Local => TransformGizmoSpace::World,
        };
    }
}

#[derive(Component)]
struct InstructionsText;

fn update_instructions(
    settings: Res<TransformGizmoSettings>,
    mut text: Single<&mut Text, With<InstructionsText>>,
) {
    let mode_str = match settings.mode {
        TransformGizmoMode::Translate => "Translate",
        TransformGizmoMode::Rotate => "Rotate",
        TransformGizmoMode::Scale => "Scale",
    };
    let space_str = match settings.space {
        TransformGizmoSpace::World => "World",
        TransformGizmoSpace::Local => "Local",
    };
    text.0 = format!(
        "Click an object to select it\n1: Translate | 2: Rotate | 3: Scale | X: World/Local space\nMode: {mode_str} | Space: {space_str}"
    );
}
