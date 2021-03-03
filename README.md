<!-- cargo-sync-readme start -->

# Overview
- [📦 crates.io](https://crates.io/crates/brightness)
- [📖 Documentation](https://docs.rs/brightness)
- [⚖ 0BSD license](https://spdx.org/licenses/0BSD.html)

Definitions to get and set brightness on Linux. This relies on D-Bus and logind.

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

# Contribute

All contributions shall be licensed under the [0BSD license](https://spdx.org/licenses/0BSD.html).

<!-- cargo-sync-readme end -->
