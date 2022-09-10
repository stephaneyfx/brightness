// Copyright (C) 2022 The brightness project authors. Distributed under the 0BSD license.

//! Platform-specific implementation for Windows.

pub use crate::blocking::windows::BrightnessExt;

use crate::{
    blocking::{
        windows::{BlockingDeviceImpl, SysError},
        Brightness,
    },
    BrightnessDevice, Error,
};
use async_trait::async_trait;
use blocking::unblock;
use futures::{stream, FutureExt, Stream, StreamExt};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct AsyncDeviceImpl(Arc<BlockingDeviceImpl>);

// Windows doesn't have an async C API for monitors, so we will instead spawn the blocking tasks on
// background threads.
#[async_trait]
impl crate::Brightness for AsyncDeviceImpl {
    async fn device_name(&self) -> Result<String, Error> {
        self.0.device_name()
    }

    async fn get(&self) -> Result<u32, Error> {
        let cloned = Arc::clone(&self.0);
        unblock(move || cloned.get()).await
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        let cloned = Arc::clone(&self.0);
        unblock(move || cloned.set(percentage)).await
    }
}

pub(crate) fn brightness_devices() -> impl Stream<Item = Result<AsyncDeviceImpl, SysError>> {
    unblock(crate::blocking::windows::brightness_devices)
        .into_stream()
        .map(stream::iter)
        .flatten()
        .map(|d| d.map(|d| AsyncDeviceImpl(Arc::new(d))).map_err(Into::into))
}

impl BrightnessExt for BrightnessDevice {
    fn device_description(&self) -> Result<String, Error> {
        Ok(self.0 .0.device_description.clone())
    }

    fn device_registry_key(&self) -> Result<String, Error> {
        Ok(self.0 .0.device_key.clone())
    }

    fn device_path(&self) -> Result<String, Error> {
        Ok(self.0 .0.device_path.clone())
    }
}
