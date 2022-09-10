// Copyright (C) 2022 The brightness project authors. Distributed under the 0BSD license.

//! The blocking API.

use crate::Error;

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        pub(crate) mod linux;
        use self::linux as platform;
    } else if #[cfg(windows)] {
        pub mod windows;
        use self::windows as platform;
    } else {
        compile_error!("unsupported platform");
    }
}

/// Blocking brightness device.
#[derive(Debug)]
pub struct BrightnessDevice(platform::BlockingDeviceImpl);

/// Blocking interface to get and set brightness.
pub trait Brightness {
    /// Returns the device name.
    fn device_name(&self) -> Result<String, Error>;

    /// Returns the current brightness as a percentage.
    fn get(&self) -> Result<u32, Error>;

    /// Sets the brightness as a percentage.
    fn set(&self, percentage: u32) -> Result<(), Error>;
}

impl Brightness for BrightnessDevice {
    fn device_name(&self) -> Result<String, Error> {
        self.0.device_name()
    }

    fn get(&self) -> Result<u32, Error> {
        self.0.get()
    }

    fn set(&self, percentage: u32) -> Result<(), Error> {
        self.0.set(percentage)
    }
}

/// Blocking function that returns all brightness devices on the running system.
pub fn brightness_devices() -> impl Iterator<Item = Result<BrightnessDevice, Error>> {
    platform::brightness_devices().map(|r| r.map(BrightnessDevice).map_err(Into::into))
}
