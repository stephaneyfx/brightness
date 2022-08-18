// Copyright (C) 2022 Stephane Raux & Contributors. Distributed under the 0BSD license.

//! Platform-specific implementation for Windows.

use crate::blocking::windows::{BlockingDeviceImpl, SysError};
use crate::blocking::Brightness;
use crate::{BrightnessDevice, Error};
use async_trait::async_trait;
use blocking_crate::unblock;
use futures::{stream, Stream, StreamExt};
use std::future::ready;
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

pub(crate) async fn brightness_devices() -> impl Stream<Item = Result<AsyncDeviceImpl, SysError>> {
    let devices = unblock(crate::blocking::windows::brightness_devices).await;
    match devices {
        Ok(devices) => stream::iter(devices)
            .map(|d| Ok(AsyncDeviceImpl(Arc::new(d))))
            .left_stream(),
        Err(err) => stream::once(ready(Err(err))).right_stream(),
    }
}

pub use crate::blocking::windows::BrightnessExt;

impl BrightnessExt for BrightnessDevice {
    fn device_description(&self) -> &str {
        &self.0 .0.device_description
    }

    fn device_registry_key(&self) -> &str {
        &self.0 .0.device_key
    }

    fn device_path(&self) -> &str {
        &self.0 .0.device_path
    }
}
