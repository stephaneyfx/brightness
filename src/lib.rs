// Copyright (C) 2021 Stephane Raux. Distributed under the 0BSD license.

//! # Overview
//! - [📦 crates.io](https://crates.io/crates/brightness)
//! - [📖 Documentation](https://docs.rs/brightness)
//! - [⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)
//!
//! Definitions to get and set brightness on Linux. This relies on D-Bus and logind.
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
//! # Contribute
//!
//! All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

#![cfg(target_os = "linux")]
#![deny(warnings)]
#![deny(missing_docs)]

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::error::Error as StdError;
use thiserror::Error;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

use platform::Brightness as Inner;

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

#[cfg(test)]
mod tests {
    use crate::brightness_devices;
    use futures::executor::block_on_stream;

    #[test]
    fn it_works() {
        for i in block_on_stream(brightness_devices()) {
            println!("{:?}", i);
        }
    }
}
