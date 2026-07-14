//! By default Bevy loads images to textures that clamps the image to the edges
//! This example shows how to configure it to repeat the image instead.

use bevy::{
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    math::Affine2,
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let image_with_default_sampler =
        asset_server.load("textures/fantasy_ui_borders/panel-border-010.png");

    // central cube with not repeated texture
    let mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let material = commands.spawn_asset(StandardMaterial {
        base_color_texture: Some(image_with_default_sampler.clone()),
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(Vec3::ZERO),
    ));

    // left cube with repeated texture
    let mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let material = commands.spawn_asset(StandardMaterial {
        base_color_texture: Some(
            asset_server
                .load_builder()
                .with_settings(|s: &mut _| {
                    *s = ImageLoaderSettings {
                        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
                            // rewriting mode to repeat image,
                            address_mode_u: ImageAddressMode::Repeat,
                            address_mode_v: ImageAddressMode::Repeat,
                            ..default()
                        }),
                        ..default()
                    }
                })
                .load("textures/fantasy_ui_borders/panel-border-010-repeated.png"),
        ),

        // uv_transform used here for proportions only, but it is full Affine2
        // that's why you can use rotation and shift also
        uv_transform: Affine2::from_scale(Vec2::new(2., 3.)),
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(-1.5, 0.0, 0.0),
    ));

    // right cube with scaled texture, because with default sampler
    let mesh = commands.spawn_asset(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let material = commands.spawn_asset(StandardMaterial {
        // there is no sampler set, that's why
        // by default you see only one small image in a row/column
        // and other space is filled by image edge
        base_color_texture: Some(image_with_default_sampler),

        // uv_transform used here for proportions only, but it is full Affine2
        // that's why you can use rotation and shift also
        uv_transform: Affine2::from_scale(Vec2::new(2., 3.)),
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(1.5, 0.0, 0.0),
    ));

    // light
    commands.spawn((
        PointLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
    // camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.5, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
