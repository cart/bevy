use bevy::{asset::AssetServerSettings, prelude::*};

/// This example illustrates various ways to load assets
fn main() {
    App::build()
         // run this first, then comment it out. this imports the asset when loaded
        .add_resource(AssetServerSettings::importer())
         // then uncomment this and run this second. this uses the imported asset
        // .add_resource(AssetServerSettings::imported_app())
        .add_default_plugins()
        .add_startup_system(setup.system())
        .add_system(test_cleanup.system())
        .run();
}

// despawn sprite after 2 seconds. the ColorMaterial and icon texture should be freed
fn test_cleanup(mut commands: Commands, time: Res<Time>, mut query: Query<(Entity, &Sprite)>) {
    if time.seconds_since_startup < 2.0 {
        return;
    }
    for (e, _) in &mut query.iter() {
        commands.despawn(e);
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands
        .spawn(Camera2dComponents::default())
        .spawn(SpriteComponents {
            material: materials.add(asset_server.load("branding/icon.png").into()),
            ..Default::default()
        });
}
