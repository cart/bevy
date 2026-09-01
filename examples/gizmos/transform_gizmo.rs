//! Interactive transform gizmo example.
//!
//! Demonstrates translate, rotate, and scale gizmos with click-to-select.
//! - Click an object to select it (primary mouse button)
//! - Radio buttons switch between Translate/Rotate/Scale modes and toggle World/Local space.

use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    feathers::{controls::FeathersSlider, display::label, theme::UiTheme, FeathersPlugins},
    gizmos::transform_gizmo::{
        TransformGizmoCamera, TransformGizmoFocus, TransformGizmoMode, TransformGizmoPlugin,
        TransformGizmoSettings, TransformGizmoSpace,
    },
    picking::{pointer::PointerButton, Pickable},
    prelude::*,
    ui_widgets::{radio_self_update, SliderPrecision, SliderStep, SliderValue, ValueChange},
};

use radio::{feathers_option_buttons, main_ui_node_scene, RadioButtonOptionValue};

#[path = "../helpers/radio.rs"]
mod radio;

#[path = "../helpers/theme.rs"]
mod theme;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            FreeCameraPlugin,
            MeshPickingPlugin,
            TransformGizmoPlugin,
            FeathersPlugins,
        ))
        .insert_resource(UiTheme(theme::basic_example_theme(Color::WHITE)))
        .add_systems(Startup, setup)
        .add_systems(Update, toggle_scale_slider)
        .add_observer(update_radio_button)
        .add_observer(radio_self_update)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn_scene(bsn! {
        @main_ui_node_scene()
        Children [
            (
                @label("Click an object to select it")
            ),
            (
                @feathers_option_buttons("",
                &[
                    (TransformGizmoMode::Translate, "Translate"),
                    (TransformGizmoMode::Rotate, "Rotate"),
                    (TransformGizmoMode::Scale, "Scale"),
                ], 0)
            ),
            (
                @feathers_option_buttons("",
                &[
                    (TransformGizmoSpace::World, "World"),
                    (TransformGizmoSpace::Local, "Local"),
                ], 0)
            ),
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(4)
                }
                ScaleSensitivitySlider
                Children [
                    (
                        @label("Sensitivity")
                    ),
                    (
                        @FeathersSlider{
                            @max: 2.0,
                            @min: 0.1,
                        }
                        SliderValue(1.0)
                        SliderPrecision(2)
                        SliderStep(0.1)
                        on(slider_update)
                    )
                ]
            )
        ]
    });

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
    click: On<PointerClick>,
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

fn update_radio_button(
    event: On<ValueChange<Entity>>,
    mode_value_query: Query<&RadioButtonOptionValue<TransformGizmoMode>>,
    space_value_query: Query<&RadioButtonOptionValue<TransformGizmoSpace>>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if let Ok(RadioButtonOptionValue(mode)) = mode_value_query.get(event.value) {
        settings.mode = *mode;
    } else if let Ok(RadioButtonOptionValue(space)) = space_value_query.get(event.value) {
        settings.space = *space;
    };
}

fn slider_update(
    value_change: On<ValueChange<f32>>,
    mut commands: Commands,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    commands
        .entity(value_change.source)
        .insert(SliderValue(value_change.value));

    settings.scale_sensitivity = value_change.value;
}

#[derive(Component, Clone, Default)]
struct ScaleSensitivitySlider;

fn toggle_scale_slider(
    settings: Res<TransformGizmoSettings>,
    mut slider_node: Single<&mut Node, With<ScaleSensitivitySlider>>,
) {
    slider_node.display = if settings.mode == TransformGizmoMode::Scale {
        Display::Flex
    } else {
        Display::None
    };
}
