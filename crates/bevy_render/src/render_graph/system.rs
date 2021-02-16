use super::RenderGraph;
use bevy_ecs::{core::World, schedule::Stage};

pub fn render_graph_schedule_executor_system(world: &mut World) {
    // run render graph systems
    let (mut system_schedule, mut commands) = {
        let mut render_graph = world.get_resource_mut::<RenderGraph>().unwrap();
        (render_graph.take_schedule(), render_graph.take_commands())
    };

    commands.apply(world);
    if let Some(schedule) = system_schedule.as_mut() {
        schedule.run(world);
    }
    let mut render_graph = world.get_resource_mut::<RenderGraph>().unwrap();
    if let Some(schedule) = system_schedule.take() {
        render_graph.set_schedule(schedule);
    }
}
