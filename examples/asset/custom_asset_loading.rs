use bevy::{
    asset::{Asset, AssetLoader, LoadContext, LoadedAsset},
    prelude::*,
    type_registry::TypeUuid,
};
use bevy_type_registry::{TypeUuidDynamic, Uuid};
use ron::de::from_bytes;
use serde::{export::PhantomData, Deserialize};

#[derive(Deserialize, TypeUuid)]
#[uuid = "39cadc56-aa9c-4543-8640-a018b74b5052"]
pub struct MyCustomData {
    pub num: i32,
}

#[derive(Deserialize, TypeUuid)]
#[uuid = "9e08c542-ab71-44f2-ac5b-bc1c75e2a319"]
pub struct MySecondCustomData {
    pub is_set: bool,
}

// create a custom loader for data files
#[derive(Default)]
pub struct DataFileLoader<TAsset> {
    matching_extensions: Vec<&'static str>,
    type_uuid: Uuid,
    marker: PhantomData<TAsset>,
}

impl<TAsset> TypeUuidDynamic for DataFileLoader<TAsset> {
    fn type_uuid(&self) -> Uuid {
        self.type_uuid
    }
}

impl<TAsset> DataFileLoader<TAsset> {
    pub fn from_extensions(type_uuid: Uuid, matching_extensions: Vec<&'static str>) -> Self {
        DataFileLoader {
            type_uuid,
            matching_extensions,
            marker: PhantomData::default(),
        }
    }
}

impl<TAsset: Asset> AssetLoader for DataFileLoader<TAsset>
where
    for<'de> TAsset: Deserialize<'de>,
{
    fn load(&self, bytes: Vec<u8>, load_context: &mut LoadContext) -> Result<(), anyhow::Error> {
        load_context.set_default_asset(LoadedAsset::new(from_bytes::<TAsset>(bytes.as_slice())?));
        Ok(())
    }

    fn extensions(&self) -> &[&str] {
        self.matching_extensions.as_slice()
    }
}

/// This example illustrates various ways to load assets
fn main() {
    App::build()
        .add_default_plugins()
        .add_asset::<MyCustomData>()
        .add_asset_loader(DataFileLoader::<MyCustomData>::from_extensions(
            Uuid::parse_str("46131f2b-cad9-4e08-8212-a7124f5f18e3").unwrap(),
            vec!["data1"],
        ))
        .add_asset::<MySecondCustomData>()
        .add_asset_loader(DataFileLoader::<MySecondCustomData>::from_extensions(
            Uuid::parse_str("afacfc44-d90a-4e9d-be84-874a2238b96e").unwrap(),
            vec!["data2"],
        ))
        .add_startup_system(setup.system())
        .run();
}

fn setup(
    _asset_server: Res<AssetServer>,
    mut _data1s: ResMut<Assets<MyCustomData>>,
    mut _data2s: ResMut<Assets<MySecondCustomData>>,
) {
    panic!("this example was written to use load_sync, which is no longer available");
    // let data1_handle = asset_server
    //     .load_sync(&mut data1s, "data/test_data.data1")
    //     .unwrap();
    // let data2_handle = asset_server
    //     .load_sync(&mut data2s, "data/test_data.data2")
    //     .unwrap();

    // let data1 = data1s.get(&data1_handle).unwrap();
    // println!("Data 1 loaded with value {}", data1.num);

    // let data2 = data2s.get(&data2_handle).unwrap();
    // println!("Data 2 loaded with value {}", data2.is_set);
}
