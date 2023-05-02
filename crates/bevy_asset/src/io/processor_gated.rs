use crate::{
    io::{AssetReader, AssetReaderError, PathStream, Reader},
    processor::{AssetProcessorData, ProcessStatus},
};
use anyhow::Result;
use bevy_log::trace;
use bevy_utils::BoxedFuture;
use std::{path::Path, sync::Arc};

pub struct ProcessorGatedReader {
    reader: Box<dyn AssetReader>,
    processor_data: Arc<AssetProcessorData>,
}

impl ProcessorGatedReader {
    pub fn new(reader: Box<dyn AssetReader>, processor_data: Arc<AssetProcessorData>) -> Self {
        Self {
            processor_data,
            reader,
        }
    }
}

impl AssetReader for ProcessorGatedReader {
    fn read<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader<'static>>, AssetReaderError>> {
        Box::pin(async move {
            trace!("Waiting for processing to finish before reading {:?}", path);
            // TODO: handle the response here
            let process_result = self.processor_data.wait_until_processed(path).await;
            match process_result {
                ProcessStatus::Processed => {}
                ProcessStatus::Failed | ProcessStatus::NonExistent => {
                    return Err(AssetReaderError::NotFound(path.to_owned()))
                }
            }
            trace!(
                "Processing finished with {:?}, reading {:?}",
                process_result,
                path
            );
            let result = self.reader.read(path).await?;
            Ok(result)
        })
    }

    fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader<'static>>, AssetReaderError>> {
        Box::pin(async move {
            trace!(
                "Waiting for processing to finish before reading meta {:?}",
                path
            );
            // TODO: handle the response here
            let process_result = self.processor_data.wait_until_processed(path).await;
            match process_result {
                ProcessStatus::Processed => {}
                ProcessStatus::Failed | ProcessStatus::NonExistent => {
                    return Err(AssetReaderError::NotFound(path.to_owned()));
                }
            }
            trace!(
                "Processing finished with {:?}, reading meta {:?}",
                process_result,
                path
            );
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
            self.processor_data.wait_until_finished().await;
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
            self.processor_data.wait_until_finished().await;
            trace!("Processing finished, getting directory status {:?}", path);
            let result = self.reader.is_directory(path).await?;
            Ok(result)
        })
    }
}
