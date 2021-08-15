<!-- cargo-sync-readme start -->

[![Build status](https://github.com/stephaneyfx/brightness/actions/workflows/rust.yml/badge.svg)](https://github.com/stephaneyfx/brightness/actions)
[![crates.io](https://img.shields.io/crates/v/brightness.svg)](https://crates.io/crates/brightness)
[![docs.rs](https://docs.rs/brightness/badge.svg)](https://docs.rs/brightness)

# Overview

[⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)

# Example

```rust
use brightness::Brightness;
use futures::TryStreamExt;

async fn show_brightness() -> Result<(), brightness::Error> {
    brightness::brightness_devices().try_for_each(|dev| async move {
        let name = dev.device_name().await?;
        let value = dev.get().await?;
        println!("Brightness of device {} is {}%", name, value);
        Ok(())
    }).await
}
```

## Linux

This library interacts with the displays found at `/sys/class/backlight`. Note this means that in order to control 
external displays (via the DDC/CI) protocol, you need to have the 
[ddcci-backlight](https://gitlab.com/ddcci-driver-linux/ddcci-driver-linux#ddcci-backlight-monitor-backlight-driver)
kernel driver loaded first.

When the `zbus` cargo feature is enabled the library will try to use `D-Bus` to write the backlight values via 
`systemd-logind`, however this requires 
[systemd 243 or later](https://github.com/systemd/systemd/blob/877aa0bdcc2900712b02dac90856f181b93c4e40/NEWS#L262).
You can disable this with the `--no-default-features` flag, and the library will instead write directly to 
`/sys/class/backlight/$DEVICE/brightness` - however this requires you to set the permissions on these paths 
appropriately for non-root users (look at using `udev` rules).

# Contribute

All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

<!-- cargo-sync-readme end -->
