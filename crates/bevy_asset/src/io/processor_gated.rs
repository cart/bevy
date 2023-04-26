use crate::{
    io::{AssetReader, AssetReaderError, PathStream, Reader},
    processor::AssetProcessor,
};
use anyhow::Result;
use bevy_log::trace;
use bevy_utils::BoxedFuture;
use std::path::Path;

pub struct ProcessorGatedReader {
    reader: Box<dyn AssetReader>,
    processor: AssetProcessor,
}

impl ProcessorGatedReader {
    pub fn new(reader: Box<dyn AssetReader>, processor: AssetProcessor) -> Self {
        Self { processor, reader }
    }
}

impl AssetReader for ProcessorGatedReader {
    fn read<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader>, AssetReaderError>> {
        Box::pin(async move {
            trace!("Waiting for processing to finish before reading {:?}", path);
            self.processor.wait_until_finished().await;
            trace!("Processing finished, reading {:?}", path);
            let result = self.reader.read(path).await?;
            Ok(result)
        })
    }

    fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader>, AssetReaderError>> {
        Box::pin(async move {
            trace!(
                "Waiting for processing to finish before reading meta {:?}",
                path
            );
            self.processor.wait_until_finished().await;
            trace!("Processing finished, reading meta {:?}", path);
            let result = self.reader.read_meta(path).await?;
            Ok(result)
        })
    }

    fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<PathStream>, AssetReaderError>> {
        Box::pin(async move {
            trace!(
                "Waiting for processing to finish before reading directory {:?}",
                path
            );
            self.processor.wait_until_finished().await;
            trace!("Processing finished, reading directory {:?}", path);
            let result = self.reader.read_directory(path).await?;
            Ok(result)
        })
    }

    fn is_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, std::result::Result<bool, AssetReaderError>> {
        Box::pin(async move {
            trace!(
                "Waiting for processing to finish before reading directory {:?}",
                path
            );
            self.processor.wait_until_finished().await;
            trace!("Processing finished, getting directory status {:?}", path);
            let result = self.reader.is_directory(path).await?;
            Ok(result)
        })
    }
}
