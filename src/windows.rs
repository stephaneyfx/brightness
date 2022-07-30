// Copyright (C) 2022 Stephane Raux & Contributors. Distributed under the 0BSD license.

//! Platform-specific implementation for Windows.

use crate::blocking::windows::{BlockingDeviceImpl, SysError};
use crate::blocking::Brightness;
use crate::{BrightnessDevice, Error};
use async_trait::async_trait;
use futures::{stream, Stream, StreamExt};
use std::future::ready;
use std::sync::Arc;
use tokio::task;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub(crate) struct AsyncDeviceImpl(Arc<BlockingDeviceImpl>);

// Windows doesn't have an async C API for monitors, so we will instead spawn the blocking tasks on
// background threads.
#[async_trait]
impl crate::Brightness for AsyncDeviceImpl {
    async fn device_name(&self) -> Result<String, Error> {
        let cloned = Arc::clone(&self.0);
        join_or_panic(task::spawn_blocking(move || cloned.device_name())).await
    }

    async fn get(&self) -> Result<u32, Error> {
        let cloned = Arc::clone(&self.0);
        join_or_panic(task::spawn_blocking(move || cloned.get())).await
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        let cloned = Arc::clone(&self.0);
        join_or_panic(task::spawn_blocking(move || cloned.set(percentage))).await
    }
}

pub(crate) async fn brightness_devices() -> impl Stream<Item = Result<AsyncDeviceImpl, SysError>> {
    let devices = join_or_panic(task::spawn_blocking(|| {
        crate::blocking::windows::brightness_devices()
    }))
    .await;
    match devices {
        Ok(devices) => stream::iter(devices)
            .map(|d| Ok(AsyncDeviceImpl(Arc::new(d))))
            .left_stream(),
        Err(err) => stream::once(ready(Err(err))).right_stream(),
    }
}

async fn join_or_panic<T>(handle: JoinHandle<T>) -> T {
    match handle.await {
        Ok(ok) => ok,
        Err(e) => {
            if e.is_panic() {
                std::panic::resume_unwind(e.into_panic());
            } else {
                unreachable!("Task was unexpectedly aborted: {:?}", e);
            }
        }
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
