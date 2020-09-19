use bevy_hecs::*;
pub fn main() {
    let mut world = World::new();
    const N: usize = 100;
    for _ in 0..N {
        world.spawn((42u128,));
    }
    assert_eq!(world.iter().count(), N);
}
