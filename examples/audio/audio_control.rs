//! This example illustrates how to load and play an audio file, and control how it's played.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, (update_speed, pause, volume))
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, audio: Res<Audio>) {
    let music = asset_server.load("sounds/Windless Slopes.ogg");
    let handle = audio.play(music);
    commands.insert_resource(MusicController(handle));
}

#[derive(Resource)]
struct MusicController(Handle<AudioSink>);

fn update_speed(
    audio_sinks: Res<Assets<AudioSink>>,
    music_controller: Res<MusicController>,
    time: Res<Time>,
) {
    if let Some(sink) = audio_sinks.get(&music_controller.0) {
        sink.set_speed(((time.elapsed_seconds() / 5.0).sin() + 1.0).max(0.1));
    }
}

fn pause(
    keyboard_input: Res<Input<KeyCode>>,
    audio_sinks: Res<Assets<AudioSink>>,
    music_controller: Res<MusicController>,
) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        if let Some(sink) = audio_sinks.get(&music_controller.0) {
            sink.toggle();
        }
    }
}

fn volume(
    keyboard_input: Res<Input<KeyCode>>,
    audio_sinks: Res<Assets<AudioSink>>,
    music_controller: Res<MusicController>,
) {
    if let Some(sink) = audio_sinks.get(&music_controller.0) {
        if keyboard_input.just_pressed(KeyCode::Plus) {
            sink.set_volume(sink.volume() + 0.1);
        } else if keyboard_input.just_pressed(KeyCode::Minus) {
            sink.set_volume(sink.volume() - 0.1);
        }
    }
}
