use embedded_graphics::{
    pixelcolor::Rgb565,
    geometry::Size,
};
use embedded_graphics_simulator::{SimulatorDisplay, OutputSettingsBuilder};

use ::cloud_chamber::ui::screens;

fn main() -> Result<(), core::convert::Infallible> {
    let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(320, 240));

    let main_menu_screen = screens::menu::MainMenuScreen::new();

    main_menu_screen.draw(&mut display)?;

    // SAVE SCREENSHOT
    let output_settings = OutputSettingsBuilder::new()
        .build();

    let path = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| "screenshot.png".into());
    display
        .to_rgb_output_image(&output_settings)
        .save_png(&path)
        .expect("failed to save screenshot");

    Ok(())
}