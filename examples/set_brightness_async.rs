use brightness::{Brightness, BrightnessDevice};
use futures::{executor::block_on, TryStreamExt};
use std::env;

fn main() {
    let percentage = env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .expect("Desired brightness percentage must be given as parameter");
    block_on(run(percentage));
}

async fn run(percentage: u32) {
    brightness::brightness_devices()
        .try_for_each(|mut dev| async move {
            show_brightness(&dev).await?;
            dev.set(percentage).await?;
            show_brightness(&dev).await
        })
        .await
        .unwrap()
}

async fn show_brightness(dev: &BrightnessDevice) -> Result<(), brightness::Error> {
    println!(
        "Brightness of device {} is {}%",
        dev.device_name().await?,
        dev.get().await?
    );
    Ok(())
}
