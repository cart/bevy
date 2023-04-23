use anyhow::Result;
use bevy_asset::{io::Reader, AssetLoader, AsyncReadExt, LoadContext};
use bevy_ecs::prelude::{FromWorld, World};
use thiserror::Error;

use crate::{
    renderer::RenderDevice,
    texture::{Image, ImageType, TextureError},
};

use super::CompressedImageFormats;

/// Loader for images that can be read by the `image` crate.
#[derive(Clone)]
pub struct ImageTextureLoader {
    supported_compressed_formats: CompressedImageFormats,
}

const FILE_EXTENSIONS: &[&str] = &[
    #[cfg(feature = "basis-universal")]
    "basis",
    #[cfg(feature = "bmp")]
    "bmp",
    #[cfg(feature = "png")]
    "png",
    #[cfg(feature = "dds")]
    "dds",
    #[cfg(feature = "tga")]
    "tga",
    #[cfg(feature = "jpeg")]
    "jpg",
    #[cfg(feature = "jpeg")]
    "jpeg",
    #[cfg(feature = "ktx2")]
    "ktx2",
    #[cfg(feature = "webp")]
    "webp",
];

impl AssetLoader for ImageTextureLoader {
    type Asset = Image;
    type Settings = ();
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a (),
        load_context: &'a mut LoadContext,
    ) -> bevy_utils::BoxedFuture<'a, Result<Image, anyhow::Error>> {
        Box::pin(async move {
            // use the file extension for the image type
            let ext = load_context.path().extension().unwrap().to_str().unwrap();

            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            Ok(Image::from_buffer(
                &bytes,
                ImageType::Extension(ext),
                self.supported_compressed_formats,
                true,
            )
            .map_err(|err| FileTextureError {
                error: err,
                path: format!("{}", load_context.path().display()),
            })?)
        })
    }

    fn extensions(&self) -> &[&str] {
        FILE_EXTENSIONS
    }
}

impl FromWorld for ImageTextureLoader {
    fn from_world(world: &mut World) -> Self {
        let supported_compressed_formats = match world.get_resource::<RenderDevice>() {
            Some(render_device) => CompressedImageFormats::from_features(render_device.features()),

            None => CompressedImageFormats::all(),
        };
        Self {
            supported_compressed_formats,
        }
    }
}

/// An error that occurs when loading a texture from a file.
#[derive(Error, Debug)]
pub struct FileTextureError {
    error: TextureError,
    path: String,
}
impl std::fmt::Display for FileTextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(
            f,
            "Error reading image file {}: {}, this is an error in `bevy_render`.",
            self.path, self.error
        )
    }
}
