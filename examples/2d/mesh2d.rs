//! Shows how to render a polygonal [`Mesh`], generated from a [`Rectangle`] primitive, in a 2D scene.

use bevy::{color::palettes::basic::PURPLE, prelude::*};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let mesh = commands.spawn_asset(Mesh::from(Rectangle::default()));
    let material = commands.spawn_asset(ColorMaterial::from(Color::from(PURPLE)));
    commands.spawn((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::default().with_scale(Vec3::splat(128.)),
    ));
}
