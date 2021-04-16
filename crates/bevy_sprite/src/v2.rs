use bevy_app::{AppBuilder, Plugin};

use crate::Sprite;


#[derive(Default)]
pub struct PipelinedSpritePlugin;

impl Plugin for PipelinedSpritePlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<Sprite>();

        app.sub_app_mut(0);

        let world = app.world_mut();
        world
            .register_component(ComponentDescriptor::new::<OutsideFrustum>(
                StorageType::SparseSet,
            ))
            .unwrap();

        let world_cell = world.cell();
        let mut render_graph = world_cell.get_resource_mut::<RenderGraph>().unwrap();
        let mut pipelines = world_cell
            .get_resource_mut::<Assets<PipelineDescriptor>>()
            .unwrap();
        let mut shaders = world_cell.get_resource_mut::<Assets<Shader>>().unwrap();
        crate::render::add_sprite_graph(&mut render_graph, &mut pipelines, &mut shaders);

        let mut meshes = world_cell.get_resource_mut::<Assets<Mesh>>().unwrap();
        let mut color_materials = world_cell
            .get_resource_mut::<Assets<ColorMaterial>>()
            .unwrap();
        color_materials.set_untracked(Handle::<ColorMaterial>::default(), ColorMaterial::default());
        meshes.set_untracked(
            QUAD_HANDLE,
            // Use a flipped quad because the camera is facing "forward" but quads should face
            // backward
            Mesh::from(shape::Quad::new(Vec2::new(1.0, 1.0))),
        )
    }
}
