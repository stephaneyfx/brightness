use brightness::{Brightness, BrightnessDevice};
use futures::{executor::block_on, TryStreamExt};

fn main() {
    block_on(run());
}

async fn run() {
    let count = brightness::brightness_devices()
        .try_fold(0, |count, dev| async move {
            show_brightness(&dev).await?;
            Ok(count + 1)
        })
        .await
        .unwrap();
    println!("Found {} displays", count);
}

async fn show_brightness(dev: &BrightnessDevice) -> Result<(), brightness::Error> {
    println!("Display {}", dev.device_name().await?);
    println!("\tBrightness = {}%", dev.get().await?);
    show_platform_specific_info(dev).await?;
    Ok(())
}

#[cfg(windows)]
async fn show_platform_specific_info(dev: &BrightnessDevice) -> Result<(), brightness::Error> {
    use brightness::windows::BrightnessExt;
    println!("\tDevice description = {}", dev.device_description()?);
    println!("\tDevice registry key = {}", dev.device_registry_key()?);
    Ok(())
}

#[cfg(not(windows))]
async fn show_platform_specific_info(_: &BrightnessDevice) -> Result<(), brightness::Error> {
    Ok(())
}
