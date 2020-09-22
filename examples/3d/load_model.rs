use bevy::prelude::*;
use bevy_asset::HandleId;
use bevy_type_registry::TypeRegistry;

fn main() {
    App::build()
        .add_resource(Msaa { samples: 4 })
        .add_default_plugins()
        .add_startup_system(setup.system())
        // .add_system(save_sys.system())
        // .add_system(print_world_system.thread_local_system())
        .run();
}

#[allow(unused)]
fn save_sys(asset_server: Res<AssetServer>) {
    asset_server.save_meta().unwrap();
}

#[allow(unused)]
fn print_world_system(world: &mut World, resources: &mut Resources) {
    let registry = resources.get::<TypeRegistry>().unwrap();
    let dc = DynamicScene::from_world(world, &registry.component.read());
    println!("WORLD");
    println!("{}", dc.serialize_ron(&registry.property.read()).unwrap());
    println!();
    println!();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut scene_spawner: ResMut<SceneSpawner>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // let handle: Handle<Mesh> = asset_server.load("assets/models/scene/scene.gltf#Mesh0/Primitive0").unwrap();
    let scene_handle: Handle<Scene> = asset_server.load("assets/models/scene/scene.gltf").unwrap();
    // scene_spawner.spawn(scene_handle);
    materials.set_untracked(
        HandleId::default::<StandardMaterial>(),
        StandardMaterial {
            albedo: Color::RED,
            ..Default::default()
        },
    );

    // add entities to the world
    commands
        .spawn_scene(scene_handle)
        // .spawn(PbrComponents {
        //     mesh: asset_server
        //         .get_handle("assets/models/scene/scene.gltf#Mesh0/Primitive0")
        //         .unwrap(),
        //     ..Default::default()
        // })
        .spawn(LightComponents {
            transform: Transform::from_translation(Vec3::new(4.0, 5.0, 4.0)),
            ..Default::default()
        })
        // camera
        .spawn(Camera3dComponents {
            transform: Transform::new(Mat4::face_toward(
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            )),
            ..Default::default()
        });
}
