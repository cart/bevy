use bevy::{prelude::*, sprite::entity::PipelinedSpriteBundle};
use bevy::PipelinedDefaultPlugins;

fn main() {
    App::new()
        .add_plugins(PipelinedDefaultPlugins)
        .add_startup_system(setup.system())
        .run();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    // mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // let texture_handle = asset_server.load("branding/icon.png");
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());
    commands.spawn_bundle(PipelinedSpriteBundle {
        // material: materials.add(texture_handle.into()),
        sprite: Sprite {
            size: Vec2::new(50.0, 50.0),
            ..Default::default()
        },
        transform: Transform::from_xyz(100.0, 0.0, 0.0),
        ..Default::default()
    });
    commands.spawn_bundle(PipelinedSpriteBundle {
        // material: materials.add(texture_handle.into()),
        sprite: Sprite {
            size: Vec2::new(50.0, 50.0),
            ..Default::default()
        },
        transform: Transform::from_xyz(-100.0, 0.0, 0.0),
        ..Default::default()
    });
    commands.spawn_bundle(PipelinedSpriteBundle {
        // material: materials.add(texture_handle.into()),
        sprite: Sprite {
            size: Vec2::new(50.0, 50.0),
            ..Default::default()
        },
        transform: Transform::from_xyz(0.0, 100.0, 0.0),
        ..Default::default()
    });
}
