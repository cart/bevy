use crate::Font;
use anyhow::Result;
use bevy_asset::{LoadedAsset, AssetLoader, LoadContext};

#[derive(Default)]
pub struct FontLoader;

impl AssetLoader for FontLoader {
    fn load(&self, bytes: Vec<u8>, load_context: &mut LoadContext) -> Result<()> {
        let font = Font::try_from_bytes(bytes)?;
        load_context.set_default_asset(LoadedAsset::new(font));
        Ok(())
    }

    fn extensions(&self) -> &[&str] {
        static EXTENSIONS: &[&str] = &["ttf"];
        EXTENSIONS
    }
}
