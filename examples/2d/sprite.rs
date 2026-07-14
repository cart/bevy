//! Displays a single [`Sprite`], created from an image.

use bevy::{prelude::*, sprite::SpriteTemplate};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn_template((
        SpriteTemplate {
            image: "branding/bevy_bird_dark.png".into(),
            ..default()
        },
        Transform::default(),
    ));
}
