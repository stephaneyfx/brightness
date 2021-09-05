use brightness::Brightness;
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

async fn show_brightness<T>(dev: &T) -> Result<(), brightness::Error>
where
    T: Brightness,
{
    println!("Display {}", dev.device_name().await?);
    println!("\tBrightness = {}%", dev.get().await?);
    Ok(())
}
