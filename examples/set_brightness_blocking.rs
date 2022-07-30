use brightness::blocking::{Brightness, BrightnessDevice};
use std::env;

fn main() {
    let percentage = env::args()
        .skip(1)
        .next()
        .and_then(|a| a.parse().ok())
        .expect("Desired brightness percentage must be given as parameter");
    run(percentage);
}

fn run(percentage: u32) {
    brightness::blocking::brightness_devices()
        .unwrap()
        .iter()
        .try_for_each(|dev| {
            show_brightness(&dev)?;
            dev.set(percentage)?;
            show_brightness(&dev)
        })
        .unwrap()
}

fn show_brightness(dev: &BrightnessDevice) -> Result<(), brightness::Error> {
    println!(
        "Brightness of device {} is {}%",
        dev.device_name()?,
        dev.get()?
    );
    Ok(())
}
