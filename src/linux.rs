// Copyright (C) 2021 Stephane Raux. Distributed under the 0BSD license.

use crate::Error;
use async_trait::async_trait;
use either::{Left, Right};
use futures::Stream;
use std::{fs, io, iter, path::PathBuf};

const BACKLIGHT_DIR: &str = "/sys/class/backlight";

#[derive(Debug)]
pub struct Brightness {
    device: String,
}

#[async_trait]
impl crate::Brightness for Brightness {
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
        let desired = (
            "backlight",
            &self.device,
            (u64::from(percentage) * max / 100) as u32,
        );
        let mut bus = zbus::azync::Connection::new_system().await.map_err(|e| {
            Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            }
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
        response
            .map(|_| ())
            .map_err(|e| Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            })
    }
}

pub fn brightness_devices() -> impl Stream<Item = Result<Brightness, SysError>> {
    let devices = match fs::read_dir(BACKLIGHT_DIR) {
        Ok(devices) => Right(
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
                        .map(|device| Brightness { device })
                        .filter(|_| keep))
                })
                .filter_map(Result::transpose),
        ),
        Err(e) => Left(iter::once(Err(SysError::ReadingBacklightDirFailed(e)))),
    };
    futures::stream::iter(devices)
}

#[derive(Debug, Error)]
pub enum SysError {
    #[error("Failed to read {} directory", BACKLIGHT_DIR)]
    ReadingBacklightDirFailed(#[source] io::Error),
    #[error("Failed to read backlight device info {}", .path.display())]
    ReadingBacklightDeviceFailed { path: PathBuf, source: io::Error },
    #[error("Failed to parse backlight info in {}: {reason}", .path.display())]
    ParsingBacklightInfoFailed { path: PathBuf, reason: String },
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        match &e {
            SysError::ReadingBacklightDirFailed(_) => Error::ListingDevicesFailed(e.into()),
            SysError::ReadingBacklightDeviceFailed { path, .. } => Error::GettingDeviceInfoFailed {
                device: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .into(),
                source: e.into(),
            },
            SysError::ParsingBacklightInfoFailed { path, .. } => Error::GettingDeviceInfoFailed {
                device: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .into(),
                source: e.into(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Value {
    Actual,
    Max,
}

impl Value {
    fn as_str(&self) -> &str {
        match self {
            Value::Actual => "actual_brightness",
            Value::Max => "max_brightness",
        }
    }
}

fn read_value(device: &str, name: Value) -> Result<u64, SysError> {
    let path = [BACKLIGHT_DIR, device, name.as_str()]
        .iter()
        .collect::<PathBuf>();
    fs::read_to_string(&path)
        .map_err(|source| SysError::ReadingBacklightDeviceFailed {
            path: path.clone(),
            source,
        })?
        .trim()
        .parse::<u64>()
        .map_err(|e| SysError::ParsingBacklightInfoFailed {
            path,
            reason: e.to_string(),
        })
}
