pub mod file;
pub mod gated;
pub mod memory;
pub mod processor_gated;

pub use futures_lite::{AsyncReadExt, AsyncWriteExt};

use bevy_utils::BoxedFuture;
use futures_io::{AsyncRead, AsyncWrite};
use futures_lite::Stream;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that occur while loading assets.
#[derive(Error, Debug)]
pub enum AssetReaderError {
    /// Path not found.
    #[error("path not found: {0}")]
    NotFound(PathBuf),

    /// Encountered an I/O error while loading an asset.
    #[error("encountered an io error while loading asset: {0}")]
    Io(#[from] std::io::Error),
}

pub type Reader = dyn AsyncRead + Unpin + Send + Sync;

pub trait AssetReader: Send + Sync + 'static {
    /// Returns a future to load the full file data at the provided path.
    fn read<'a>(&'a self, path: &'a Path)
        -> BoxedFuture<'a, Result<Box<Reader>, AssetReaderError>>;
    /// Returns a future to load the full file data at the provided path.
    fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader>, AssetReaderError>>;
    /// Returns an iterator of directory entry names at the provided path.
    fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<PathStream>, AssetReaderError>>;
}

pub type Writer = dyn AsyncWrite + Unpin + Send + Sync;

pub type PathStream = dyn Stream<Item = PathBuf> + Unpin + Send;

/// Errors that occur while loading assets.
#[derive(Error, Debug)]
pub enum AssetWriterError {
    /// Encountered an I/O error while loading an asset.
    #[error("encountered an io error while loading asset: {0}")]
    Io(#[from] std::io::Error),
}
pub trait AssetWriter: Send + Sync + 'static {
    /// Returns a future to load the full file data at the provided path.
    fn write<'a>(&'a self, path: &'a Path) -> BoxedFuture<'a, Result<Box<Writer>, ()>>;
    fn write_meta<'a>(&'a self, path: &'a Path) -> BoxedFuture<'a, Result<Box<Writer>, ()>>;
}
