use crate::v2::draw_state::TrackedRenderPass;
use bevy_ecs::world::World;
use parking_lot::Mutex;

pub trait Draw: Send + Sync + 'static {
    fn draw(&mut self, world: &World, pass: &mut TrackedRenderPass, draw_key: usize);
}

#[derive(Default)]
pub struct DrawFunctions {
    pub draw_function: Mutex<Vec<Box<dyn Draw>>>,
}

impl DrawFunctions {
    pub fn add<D: Draw>(&self, draw_function: D) {
        self.draw_function.lock().push(Box::new(draw_function));
    }
}
