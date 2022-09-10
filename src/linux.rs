// Copyright (C) 2022 The brightness project authors. Distributed under the 0BSD license.

//! Platform-specific implementation for Linux.

use crate::{
    blocking::linux::{
        read_value, SysError, Value, BACKLIGHT_DIR, SESSION_INTERFACE, SESSION_OBJECT_PATH,
        SET_BRIGHTNESS_METHOD, USER_DBUS_NAME,
    },
    Error,
};
use async_trait::async_trait;
use blocking::unblock;
use futures::{future::ready, Stream, StreamExt};

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
        let max = read_value(&self.device, Value::Max)?;
        let actual = read_value(&self.device, Value::Actual)?;
        let percentage = if max == 0 {
            0
        } else {
            (actual * 100 / max) as u32
        };
        Ok(percentage)
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        let percentage = percentage.min(100);
        let max = read_value(&self.device, Value::Max)?;
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
                Some(USER_DBUS_NAME),
                SESSION_OBJECT_PATH,
                Some(SESSION_INTERFACE),
                SET_BRIGHTNESS_METHOD,
                &desired,
            )
            .await;
        match response {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(..)) => {
                // Setting brightness through dbus may not work on older systems that don't have
                // the `SetBrightness` method. Fall back to writing to the brightness file (which
                // requires permission).
                set_value(self.device.clone(), desired_value).await?;
                Ok(())
            }
            Err(e) => Err(Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            }),
        }
    }
}

pub(crate) fn brightness_devices() -> impl Stream<Item = Result<AsyncDeviceImpl, SysError>> {
    match std::fs::read_dir(BACKLIGHT_DIR) {
        Ok(devices) => futures::stream::iter(
            devices
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
                .filter_map(Result::transpose),
        )
        .right_stream(),
        Err(e) => {
            futures::stream::once(ready(Err(SysError::ReadingBacklightDirFailed(e)))).left_stream()
        }
    }
}

async fn set_value(device: String, value: u32) -> Result<(), SysError> {
    unblock(move || crate::blocking::linux::set_value(&device, value)).await
}
