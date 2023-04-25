use bevy_app::{App, Plugin, ScheduleRunnerPlugin, ScheduleRunnerSettings, Startup, Update};
use bevy_asset::{
    io::{Reader, Writer},
    processor::AssetProcessor,
    saver::AssetSaver,
    Asset, AssetApp, AssetLoader, AssetPlugin, AssetServer, Assets, Handle, LoadContext,
};
use bevy_core::TaskPoolPlugin;
use bevy_ecs::prelude::*;
use bevy_log::{Level, LogPlugin};
use bevy_utils::Duration;
use futures_lite::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};

fn main() {
    App::new()
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs(2)))
        .add_plugin(TaskPoolPlugin::default())
        .add_plugin(LogPlugin {
            level: Level::TRACE,
            ..Default::default()
        })
        .add_plugin(ScheduleRunnerPlugin::default())
        .add_plugin(AssetPlugin::processed_dev())
        .add_plugin(TextPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            ((print_text, print_and_despawn).chain(), print_cool),
        )
        .run();
}

pub struct TextPlugin;

impl Plugin for TextPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Text>()
            .init_asset::<CoolText>()
            .register_asset_loader(TextLoader)
            .register_asset_loader(TextRonLoader)
            .register_asset_loader(CoolTextLoader);

        if let Some(processor) = app.world.get_resource::<AssetProcessor>() {
            processor
                .register_process_plan::<TextLoader, TextRonSaver, TextRonLoader>(TextRonSaver);
        }
    }
}

#[derive(Asset, Debug)]
struct Text(String);

impl Drop for Text {
    fn drop(&mut self) {
        println!("text dropped: {}", self.0);
    }
}

#[derive(Default)]
struct TextLoader;

#[derive(Resource)]
struct MyHandle(Option<Handle<Text>>);

#[derive(Resource)]
struct CoolHandle(Option<Handle<CoolText>>);

#[derive(Default, Serialize, Deserialize)]
struct TextSettings {
    blah: bool,
}

impl AssetLoader for TextLoader {
    type Asset = Text;
    type Settings = TextSettings;
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        settings: &'a TextSettings,
        _load_context: &'a mut LoadContext,
    ) -> bevy_utils::BoxedFuture<'a, Result<Text, anyhow::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            let value = if settings.blah {
                "blah".to_string()
            } else {
                String::from_utf8(bytes).unwrap()
            };
            Ok(Text(value))
        })
    }

    fn extensions(&self) -> &[&str] {
        &["txt"]
    }
}

struct TextRonSaver;

#[derive(Serialize, Deserialize)]
pub struct RonText {
    text: String,
}

#[derive(Serialize, Deserialize)]
pub struct CoolTextRon {
    text: String,
    dependencies: Vec<String>,
    embedded_dependencies: Vec<String>,
}

#[derive(Asset, Debug)]
pub struct CoolText {
    text: String,
    dependencies: Vec<Handle<CoolText>>,
}

#[derive(Default)]
struct CoolTextLoader;

impl AssetLoader for CoolTextLoader {
    type Asset = CoolText;

    type Settings = ();

    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a Self::Settings,
        load_context: &'a mut LoadContext,
    ) -> bevy_utils::BoxedFuture<'a, Result<CoolText, anyhow::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            let ron: CoolTextRon = ron::de::from_bytes(&bytes)?;
            let mut base_text = ron.text;
            for embedded in ron.embedded_dependencies {
                let loaded = load_context.load_direct_async(&embedded).await?;
                let cool = loaded.get::<CoolText>().unwrap();
                base_text.push_str(&cool.text);
            }
            Ok(CoolText {
                text: base_text,
                dependencies: ron
                    .dependencies
                    .iter()
                    .map(|p| load_context.load(p))
                    .collect(),
            })
        })
    }

    fn extensions(&self) -> &[&str] {
        &["cool.ron"]
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct RonSaverSettings {
    appended: String,
}

impl AssetSaver for TextRonSaver {
    type Asset = Text;
    type Settings = RonSaverSettings;

    fn save<'a>(
        &'a self,
        writer: &'a mut Writer,
        asset: &'a Text,
        settings: &'a RonSaverSettings,
    ) -> bevy_utils::BoxedFuture<'a, Result<(), anyhow::Error>> {
        Box::pin(async move {
            let ron = ron::to_string(&RonText {
                text: format!("{}{}", asset.0.clone(), settings.appended),
            })
            .unwrap();
            writer.write_all(ron.as_bytes()).await?;
            Ok(())
        })
    }

    fn extension(&self) -> &'static str {
        "txt.ron"
    }
}

#[derive(Default)]
struct TextRonLoader;

impl AssetLoader for TextRonLoader {
    type Asset = Text;
    type Settings = ();
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a (),
        _load_context: &'a mut LoadContext,
    ) -> bevy_utils::BoxedFuture<'a, Result<Text, anyhow::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            let text: RonText = ron::from_str(&String::from_utf8(bytes)?)?;
            Ok(Text(text.text))
        })
    }

    fn extensions(&self) -> &[&str] {
        &["txt.ron"]
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let hello: Handle<Text> = assets.load("hello_world.txt");
    commands.insert_resource(MyHandle(Some(hello)));
    commands.insert_resource(CoolHandle(Some(assets.load("a.cool.ron"))));
}

fn print_text(handle: Res<MyHandle>, texts: Res<Assets<Text>>) {
    if let Some(handle) = &handle.0 {
        println!("{:?}", texts.get(handle));
    } else {
        println!(
            "text handle is gone. there are {} text instances",
            texts.len()
        )
    }
}

fn print_cool(handle: Res<CoolHandle>, cool_texts: Res<Assets<CoolText>>) {
    if let Some(handle) = &handle.0 {
        if let Some(text) = cool_texts.get(handle) {
            println!("{:?}", text);
            for handle in text.dependencies.iter() {
                println!("dep {:?}", cool_texts.get(handle));
            }
        }
    } else {
        println!(
            "text handle is gone. there are {} text instances",
            cool_texts.len()
        )
    }
}

fn print_and_despawn(mut iterations: Local<usize>, mut handle: ResMut<MyHandle>) {
    *iterations += 1;

    if *iterations == 20 {
        println!("removed handle");
        handle.0 = None;
    }
}
