use super::Texture;
use anyhow::Result;
use bevy_asset::{AssetLoader, AssetSerializer, LoadContext, LoadedAsset};
use bevy_type_registry::TypeUuid;

#[derive(Default, TypeUuid)]
#[uuid = "a8d20e9c-a8b0-4d1b-9899-f40ad05ff5d5"]
pub struct BinaryTextureLoader;

const BINARY_TEXTURE_EXTENSION: &str = "texture";

impl AssetLoader for BinaryTextureLoader {
    fn load(&self, bytes: &[u8], load_context: &mut LoadContext) -> Result<()> {
        let texture = bincode::deserialize::<Texture>(bytes)?;
        load_context.set_default_asset(LoadedAsset::new(texture));
        Ok(())
    }

    fn extensions(&self) -> &[&str] {
        &[BINARY_TEXTURE_EXTENSION]
    }
}

#[derive(Default, TypeUuid)]
#[uuid = "a0294291-14d8-4663-a1d6-59067aecfb4d"]
pub struct BinaryTextureSerializer;

impl AssetSerializer for BinaryTextureSerializer {
    type Asset = Texture;

    fn serialize(&self, asset: &Self::Asset) -> Result<Vec<u8>, anyhow::Error> {
        // let texture = Texture::new_fill(asset.size, &[255, 0, 0, 0], asset.format);
        let mut texture = asset.clone();
        for (i, x) in texture.data.iter_mut().enumerate() {
            if i % 3 == 0 {
                *x = 20;
            }
        }
        Ok(bincode::serialize(&texture)?)
    }

    fn extension(&self) -> &str {
        BINARY_TEXTURE_EXTENSION
    }
}
