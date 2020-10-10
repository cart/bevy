use bevy::{asset::AssetServerSettings, prelude::*};

/// This example illustrates various ways to load assets
fn main() {
    App::build()
        .add_resource(AssetServerSettings::importer())
        // .add_resource(AssetServerSettings::imported_app())
        .add_startup_system(setup.system())
        .run();
}

fn setup(asset_server: Res<AssetServer>) {
    asset_server.load_untyped("branding/icon.png");
}
