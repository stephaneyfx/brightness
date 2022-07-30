// Copyright (C) 2022 Stephane Raux & Contributors. Distributed under the 0BSD license.

//! Platform-specific implementation for Linux.

use crate::blocking::linux::{SysError, Value, BACKLIGHT_DIR};
use crate::Error;
use async_trait::async_trait;
use futures::{future::ready, Stream, StreamExt};
use std::path::PathBuf;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

#[derive(Debug)]
pub(crate) struct AsyncDeviceImpl {
    device: String,
}

#[async_trait]
impl crate::Brightness for AsyncDeviceImpl {
    async fn device_name(&self) -> Result<String, Error> {
        Ok(self.device.clone())
    }

    async fn get(&self) -> Result<u32, Error> {
        let max = read_value(&self.device, Value::Max).await?;
        let actual = read_value(&self.device, Value::Actual).await?;
        let percentage = if max == 0 {
            0
        } else {
            (actual * 100 / max) as u32
        };
        Ok(percentage)
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        let percentage = percentage.min(100);
        let max = read_value(&self.device, Value::Max).await?;
        let desired_value = (u64::from(percentage) * u64::from(max) / 100) as u32;
        let desired = ("backlight", &self.device, desired_value);
        let bus = zbus::Connection::system()
            .await
            .map_err(|e| Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            })?;
        let response = bus
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1/session/auto",
                Some("org.freedesktop.login1.Session"),
                "SetBrightness",
                &desired,
            )
            .await;
        match response {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(..)) => {
                // Setting brightness through dbus may not work on older systems that don't have
                // the `SetBrightness` method. Fall back to writing to the brightness file (which
                // requires permission).
                set_value(&self.device, desired_value).await?;
                Ok(())
            }
            Err(e) => Err(Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            }),
        }
    }
}

pub(crate) async fn brightness_devices() -> impl Stream<Item = Result<AsyncDeviceImpl, SysError>> {
    match fs::read_dir(BACKLIGHT_DIR).await {
        Ok(devices) => ReadDirStream::new(devices)
            .map(|device| {
                let device = device.map_err(SysError::ReadingBacklightDirFailed)?;
                let path = device.path();
                let keep = path.join(Value::Actual.as_str()).exists()
                    && path.join(Value::Max.as_str()).exists();
                Ok(device
                    .file_name()
                    .into_string()
                    .ok()
                    .map(|device| AsyncDeviceImpl { device })
                    .filter(|_| keep))
            })
            .filter_map(|d| async move { d.transpose() })
            .right_stream(),
        Err(e) => {
            futures::stream::once(ready(Err(SysError::ReadingBacklightDirFailed(e)))).left_stream()
        }
    }
}

async fn read_value(device: &str, name: Value) -> Result<u32, SysError> {
    let path = [BACKLIGHT_DIR, device, name.as_str()]
        .iter()
        .collect::<PathBuf>();
    fs::read_to_string(&path)
        .await
        .map_err(|source| SysError::ReadingBacklightDeviceFailed {
            device: device.into(),
            path: path.clone(),
            source,
        })?
        .trim()
        .parse::<u32>()
        .map_err(|e| SysError::ParsingBacklightInfoFailed {
            device: device.into(),
            path,
            reason: e.to_string(),
        })
}

async fn set_value(device: &str, value: u32) -> Result<(), SysError> {
    let path = [BACKLIGHT_DIR, device, "brightness"]
        .iter()
        .collect::<PathBuf>();
    fs::write(&path, value.to_string())
        .await
        .map_err(|source| SysError::WritingBrightnessFailed {
            device: device.into(),
            path: path.clone(),
            source,
        })?;
    Ok(())
}
