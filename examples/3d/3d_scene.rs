//! A simple 3D scene with light shining over a cube sitting on a plane.

use bevy::{
    color::palettes::css::{BLACK, RED},
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, (setup, scene.spawn()))
        // .add_systems(
        //     Update,
        //     |mut commands: Commands, time: Res<Time>, query: Query<Entity, With<Mesh3d>>| {
        //         if time.elapsed_secs() > 2.0 {
        //             for entity in &query {
        //                 commands.entity(entity).despawn();
        //             }
        //         }
        //     },
        // )
        .run();
}

fn setup(world: &mut World) -> Result {
    world.spawn_scene(bsn! {
        shared (
            #Material
            StandardMaterial {
                base_color: Color::srgb_u8(124, 144, 255)
            }
            on(|add: On<Add, StandardMaterial>| {
                println!("Spawned material {}", add.entity);
            })
            on(|add: On<Despawn, StandardMaterial>| {
                println!("Despawned material {}", add.entity);
            })
        )
        #CircularBase
        Mesh3d(asset_value(Circle::new(4.0)))
        MeshMaterial3d::<StandardMaterial>(#Material)
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2))
    })?;
    Ok(())
}

/// set up a simple 3D scene
fn scene() -> impl SceneList {
    bsn_list! [
        (
            #Cube
            Mesh3d(asset_value(Cuboid::new(1.0, 1.0, 1.0)))
            MeshMaterial3d::<StandardMaterial>(asset_value(Color::srgb_u8(124, 144, 255)))
            Transform::from_xyz(0.0, 0.5, 0.0)
        ),
        (
            PointLight {
                shadow_maps_enabled: true,
            }
            Transform::from_xyz(4.0, 8.0, 4.0)
        ),
        (
            Camera3d
            template_value(Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y))
        )
    ]
}
