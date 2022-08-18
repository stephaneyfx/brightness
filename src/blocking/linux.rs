// Copyright (C) 2022 Stephane Raux & Contributors. Distributed under the 0BSD license.

//! Platform-specific implementation for Linux.

use crate::Error;
use std::path::PathBuf;
use std::{fs, io};

pub(crate) const BACKLIGHT_DIR: &str = "/sys/class/backlight";

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
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
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

pub(crate) fn brightness_devices() -> Result<Vec<BlockingDeviceImpl>, SysError> {
    match fs::read_dir(BACKLIGHT_DIR) {
        Ok(devices) => devices
            .filter_map(|device| {
                let device = match device {
                    Ok(d) => d,
                    Err(e) => return Some(Err(SysError::ReadingBacklightDirFailed(e))),
                };
                let path = device.path();
                let keep = path.join(Value::Actual.as_str()).exists()
                    && path.join(Value::Max.as_str()).exists();
                keep.then(|| device.file_name().into_string().ok())
                    .flatten()
                    .map(|device| Ok(BlockingDeviceImpl { device }))
            })
            .collect::<Result<Vec<_>, _>>(),
        Err(e) => Err(SysError::ReadingBacklightDirFailed(e)),
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

#[allow(clippy::enum_variant_names)]
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
/// Note: Even though this makes a call to std::fs, we are communicating with a kernel pseudo file
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
