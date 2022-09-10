// Copyright (C) 2022 The brightness project authors. Distributed under the 0BSD license.

//! Platform-specific implementation for Linux.

use crate::Error;
use itertools::Either;
use std::{fs, io, iter::once, path::PathBuf};

pub(crate) const BACKLIGHT_DIR: &str = "/sys/class/backlight";
pub(crate) const USER_DBUS_NAME: &str = "org.freedesktop.login1";
pub(crate) const SESSION_OBJECT_PATH: &str = "/org/freedesktop/login1/session/auto";
pub(crate) const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
pub(crate) const SET_BRIGHTNESS_METHOD: &str = "SetBrightness";

#[derive(Debug)]
pub(crate) struct BlockingDeviceImpl {
    device: String,
}

impl crate::blocking::Brightness for BlockingDeviceImpl {
    fn device_name(&self) -> Result<String, Error> {
        Ok(self.device.clone())
    }

    fn get(&self) -> Result<u32, Error> {
        let max = read_value(&self.device, Value::Max)?;
        let actual = read_value(&self.device, Value::Actual)?;
        let percentage = if max == 0 {
            0
        } else {
            (actual * 100 / max) as u32
        };
        Ok(percentage)
    }

    fn set(&self, percentage: u32) -> Result<(), Error> {
        let percentage = percentage.min(100);
        let max = read_value(&self.device, Value::Max)?;
        let desired_value = (u64::from(percentage) * u64::from(max) / 100) as u32;
        let desired = ("backlight", &self.device, desired_value);
        let bus =
            zbus::blocking::Connection::system().map_err(|e| Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            })?;
        let response = bus.call_method(
            Some(USER_DBUS_NAME),
            SESSION_OBJECT_PATH,
            Some(SESSION_INTERFACE),
            SET_BRIGHTNESS_METHOD,
            &desired,
        );
        match response {
            Ok(_) => Ok(()),
            Err(zbus::Error::MethodError(..)) => {
                // Setting brightness through dbus may not work on older systems that don't have
                // the `SetBrightness` method. Fall back to writing to the brightness file (which
                // requires permission).
                set_value(&self.device, desired_value)?;
                Ok(())
            }
            Err(e) => Err(Error::SettingBrightnessFailed {
                device: self.device.clone(),
                source: e.into(),
            }),
        }
    }
}

pub(crate) fn brightness_devices() -> impl Iterator<Item = Result<BlockingDeviceImpl, SysError>> {
    match fs::read_dir(BACKLIGHT_DIR) {
        Ok(devices) => Either::Left(
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
                        .map(|device| BlockingDeviceImpl { device })
                        .filter(|_| keep))
                })
                .filter_map(Result::transpose),
        ),
        Err(e) => Either::Right(once(Err(SysError::ReadingBacklightDirFailed(e)))),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Value {
    Actual,
    Max,
}

impl Value {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Value::Actual => "actual_brightness",
            Value::Max => "max_brightness",
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum SysError {
    #[error("Failed to read {} directory", BACKLIGHT_DIR)]
    ReadingBacklightDirFailed(#[source] io::Error),
    #[error("Failed to read backlight device info {}", .path.display())]
    ReadingBacklightDeviceFailed {
        device: String,
        path: PathBuf,
        source: io::Error,
    },
    #[error("Failed to parse backlight info in {}: {reason}", .path.display())]
    ParsingBacklightInfoFailed {
        device: String,
        path: PathBuf,
        reason: String,
    },
    #[error("Failed to write brightness to {}", .path.display())]
    WritingBrightnessFailed {
        device: String,
        path: PathBuf,
        source: io::Error,
    },
}

impl From<SysError> for Error {
    fn from(e: SysError) -> Self {
        match &e {
            SysError::ReadingBacklightDirFailed(_) => Error::ListingDevicesFailed(e.into()),
            SysError::ReadingBacklightDeviceFailed { device, .. }
            | SysError::ParsingBacklightInfoFailed { device, .. } => {
                Error::GettingDeviceInfoFailed {
                    device: device.clone(),
                    source: e.into(),
                }
            }
            SysError::WritingBrightnessFailed { device, .. } => Error::SettingBrightnessFailed {
                device: device.clone(),
                source: e.into(),
            },
        }
    }
}

/// Reads a backlight device brightness value from the filesystem.
///
/// Note: Even though this makes a call to `std::fs`, we are communicating with a kernel pseudo file
/// system so it is safe to call from an async context.
pub(crate) fn read_value(device: &str, name: Value) -> Result<u32, SysError> {
    let path = [BACKLIGHT_DIR, device, name.as_str()]
        .iter()
        .collect::<PathBuf>();
    fs::read_to_string(&path)
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

/// Sets the brightness for a backlight device via the filesystem.
///
/// This is a blocking operation that can take approximately 10-100ms depending on the device.
pub(crate) fn set_value(device: &str, value: u32) -> Result<(), SysError> {
    let path = [BACKLIGHT_DIR, device, "brightness"]
        .iter()
        .collect::<PathBuf>();
    fs::write(&path, value.to_string()).map_err(|source| SysError::WritingBrightnessFailed {
        device: device.into(),
        path: path.clone(),
        source,
    })?;
    Ok(())
}
