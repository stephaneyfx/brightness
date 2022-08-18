// Copyright (C) 2021 Stephane Raux. Distributed under the 0BSD license.

//! # Overview
//! - [📦 crates.io](https://crates.io/crates/brightness)
//! - [📖 Documentation](https://docs.rs/brightness)
//! - [⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)
//!
//! This crate provides definitions to get and set display brightness.
//!
//! Linux and Windows are supported.
//!
//! # Example
//!
//! ```rust
//! use brightness::Brightness;
//! use futures::TryStreamExt;
//!
//! async fn show_brightness() -> Result<(), brightness::Error> {
//!     brightness::brightness_devices().try_for_each(|dev| async move {
//!         let name = dev.device_name().await?;
//!         let value = dev.get().await?;
//!         println!("Brightness of device {} is {}%", name, value);
//!         Ok(())
//!     }).await
//! }
//! ```
//!
//! # Linux
//!
//! This crate interacts with devices found at `/sys/class/backlight`. This means that the
//! [ddcci-backlight](https://gitlab.com/ddcci-driver-linux/ddcci-driver-linux#ddcci-backlight-monitor-backlight-driver)
//! kernel driver is required to control external displays (via DDC/CI).
//!
//! Setting brightness is attempted using D-Bus and logind, which requires
//! [systemd 243 or newer](https://github.com/systemd/systemd/blob/877aa0bdcc2900712b02dac90856f181b93c4e40/NEWS#L262).
//! If this fails because the method is not available, the desired brightness is written to
//! `/sys/class/backlight/$DEVICE/brightness`, which requires permission (`udev` rules can help with
//! that).
//!
//! # Contribute
//!
//! All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

#![deny(warnings)]
#![deny(missing_docs)]

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::error::Error as StdError;
use thiserror::Error;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(windows)]
#[path = "blocking/windows.rs"]
mod platform;

use platform::Brightness as Inner;

#[cfg(windows)]
pub use platform::BrightnessExt;

/// Interface to get and set brightness
#[async_trait]
pub trait Brightness {
    /// Returns the device name
    async fn device_name(&self) -> Result<String, Error>;

    /// Returns the current brightness as a percentage
    async fn get(&self) -> Result<u32, Error>;

    /// Sets the brightness as a percentage
    async fn set(&mut self, percentage: u32) -> Result<(), Error>;
}

/// Brightness device
#[derive(Debug)]
pub struct BrightnessDevice(Inner);

#[async_trait]
impl Brightness for BrightnessDevice {
    async fn device_name(&self) -> Result<String, Error> {
        self.0.device_name().await
    }

    async fn get(&self) -> Result<u32, Error> {
        self.0.get().await
    }

    async fn set(&mut self, percentage: u32) -> Result<(), Error> {
        self.0.set(percentage).await
    }
}

/// Returns all brightness devices on the running system
pub fn brightness_devices() -> impl Stream<Item = Result<BrightnessDevice, Error>> {
    platform::brightness_devices().map(|r| r.map(BrightnessDevice).map_err(Into::into))
}

/// Errors used in this API
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Getting a list of brightness devices failed
    #[error("Failed to list brightness devices")]
    ListingDevicesFailed(#[source] Box<dyn StdError + Send + Sync>),

    /// Getting device information failed
    #[error("Failed to get brightness device {device} information")]
    GettingDeviceInfoFailed {
        /// Device name
        device: String,
        /// Cause
        source: Box<dyn StdError + Send + Sync>,
    },

    /// Setting brightness failed
    #[error("Setting brightness failed for device {device}")]
    SettingBrightnessFailed {
        /// Device name
        device: String,
        /// Cause
        source: Box<dyn StdError + Send + Sync>,
    },
}
