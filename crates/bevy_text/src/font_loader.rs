use crate::Font;
use anyhow::Result;
use bevy_asset::{AssetLoader, LoadContext, LoadedAsset};
use bevy_type_registry::TypeUuid;

#[derive(Default, TypeUuid)]
#[uuid = "22f6a521-651e-42f0-8a91-bbf85a0a7c4d"]
pub struct FontLoader;

impl AssetLoader for FontLoader {
    fn load(&self, bytes: &[u8], load_context: &mut LoadContext) -> Result<()> {
        let font = Font::try_from_bytes(bytes.into())?;
        load_context.set_default_asset(LoadedAsset::new(font));
        Ok(())
    }

    fn extensions(&self) -> &[&str] {
        static EXTENSIONS: &[&str] = &["ttf"];
        EXTENSIONS
    }
}
