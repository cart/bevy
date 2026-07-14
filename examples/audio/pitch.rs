//! This example illustrates how to play a single-frequency sound (aka a pitch)

use bevy::prelude::*;
use std::time::Duration;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_message::<PlayPitch>()
        .add_systems(Startup, setup)
        .add_systems(Update, (play_pitch, keyboard_input_system))
        .run();
}

#[derive(Message, Default)]
struct PlayPitch;

#[derive(Resource)]
struct PitchFrequency(f32);

fn setup(mut commands: Commands) {
    commands.insert_resource(PitchFrequency(220.0));
}

fn play_pitch(
    frequency: Res<PitchFrequency>,
    pitch_assets: Query<&Pitch>,
    mut play_pitch_reader: MessageReader<PlayPitch>,
    mut commands: Commands,
) {
    let mut current_assets = pitch_assets.count();
    for _ in play_pitch_reader.read() {
        info!("playing pitch with frequency: {}", frequency.0);
        let pitch = commands.spawn_asset(Pitch::new(frequency.0, Duration::new(1, 0)));
        commands.spawn((AudioPlayer(pitch), PlaybackSettings::DESPAWN));

        current_assets += 1;
        info!("number of pitch assets: {}", current_assets);
    }
}

fn keyboard_input_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut frequency: ResMut<PitchFrequency>,
    mut play_pitch_writer: MessageWriter<PlayPitch>,
) {
    if keyboard_input.just_pressed(KeyCode::ArrowUp) {
        frequency.0 *= ops::powf(2.0f32, 1.0 / 12.0);
    }
    if keyboard_input.just_pressed(KeyCode::ArrowDown) {
        frequency.0 /= ops::powf(2.0f32, 1.0 / 12.0);
    }
    if keyboard_input.just_pressed(KeyCode::Space) {
        play_pitch_writer.write(PlayPitch);
    }
}
