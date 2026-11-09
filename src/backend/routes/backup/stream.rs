use axum::body::{Body, Bytes};
use futures::{Stream, StreamExt};
use std::pin::Pin;

use crate::error::{AppError, AppResult};

/// One bounded reader is shared by ordinary chunks and the length-framed batch body.
pub(super) struct BackupBodyReader {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, axum::Error>> + Send>>,
    pending: Bytes,
    pub position: u64,
}

impl BackupBodyReader {
    pub fn new(body: Body) -> Self {
        Self {
            stream: Box::pin(body.into_data_stream().fuse()),
            pending: Bytes::new(),
            position: 0,
        }
    }

    pub async fn read(&mut self, maximum: usize) -> AppResult<Option<Bytes>> {
        while self.pending.is_empty() {
            match self.stream.next().await {
                Some(Ok(bytes)) => self.pending = bytes,
                Some(Err(error)) => {
                    return Err(AppError::BadRequest(format!(
                        "invalid backup body: {error}"
                    )))
                }
                None => return Ok(None),
            }
        }
        let bytes = self.pending.split_to(maximum.min(self.pending.len()));
        self.position += bytes.len() as u64;
        Ok(Some(bytes))
    }

    pub async fn exact(&mut self, length: usize) -> AppResult<Vec<u8>> {
        let mut result = Vec::with_capacity(length);
        while result.len() < length {
            let bytes = self.read(length - result.len()).await?.ok_or_else(|| {
                AppError::BadRequest("backup body ended before its declared length".to_string())
            })?;
            result.extend_from_slice(&bytes);
        }
        Ok(result)
    }

    pub async fn discard(&mut self, mut length: u64) -> AppResult<()> {
        while length > 0 {
            let bytes = self
                .read(length.min(crate::runtime::FILE_IO_CHUNK_BYTES) as usize)
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest("backup body ended before its declared length".to_string())
                })?;
            length -= bytes.len() as u64;
        }
        Ok(())
    }

    pub async fn ensure_eof(&mut self) -> AppResult<()> {
        if self.read(1).await?.is_some() {
            return Err(AppError::BadRequest(
                "backup body exceeds its declared length".to_string(),
            ));
        }
        Ok(())
    }
}
