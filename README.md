<!-- cargo-sync-readme start -->

# Overview
- [📦 crates.io](https://crates.io/crates/brightness)
- [📖 Documentation](https://docs.rs/brightness)
- [⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)

This crate provides definitions to get and set display brightness.

Linux and Windows are supported.

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

# Linux

This crate interacts with devices found at `/sys/class/backlight`. This means that the
[ddcci-backlight](https://gitlab.com/ddcci-driver-linux/ddcci-driver-linux#ddcci-backlight-monitor-backlight-driver)
kernel driver is required to control external displays (via DDC/CI).

Setting brightness is attempted using D-Bus and logind, which requires
[systemd 243 or newer](https://github.com/systemd/systemd/blob/877aa0bdcc2900712b02dac90856f181b93c4e40/NEWS#L262).
If this fails because the method is not available, the desired brightness is written to
`/sys/class/backlight/$DEVICE/brightness`, which requires permission (`udev` rules can help with
that).

# Contribute

All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

<!-- cargo-sync-readme end -->
