use bevy_hecs::*;

pub fn main() {
    let mut world = World::new();
    // world.spawn_batch_new(SoaBatch::new((
    //     vec![1u8; 10000],
    //     vec![2u16; 10000],
    //     vec![3u32; 10000],
    //     vec![4u64; 10000],
    // )));

    world.spawn_batch_new(SoaBatch::new((
        vec!["hi".to_string(), "world".to_string()],
        vec![111u32, 222u32],
        vec![111u128, 222u128],
        vec![111u16, 222u16],
    )));

    for (a, b) in &mut world.query::<(&String, &u32)>() {
        println!("{} {}", a, b);
    }
}
