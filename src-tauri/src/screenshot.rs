//! Desktop-Screenshot fuer die Bild-Analyse (Phase 2.5/3): primaeren
//! Monitor aufnehmen, auf handliche Breite verkleinern (das VLM braucht
//! keine 4K-Pixel, und Base64 ueber den WebSocket bleibt klein), als PNG
//! kodieren und Base64 zurueckgeben.

use base64::Engine;
use image::codecs::png::PngEncoder;
use image::ImageEncoder;

const MAX_WIDTH: u32 = 1600;

pub fn capture_primary() -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or("kein Monitor gefunden")?;

    let mut img = monitor.capture_image().map_err(|e| e.to_string())?;

    if img.width() > MAX_WIDTH {
        let height = (img.height() as f64 * MAX_WIDTH as f64 / img.width() as f64) as u32;
        img = image::imageops::resize(&img, MAX_WIDTH, height, image::imageops::FilterType::Triangle);
    }

    let mut png: Vec<u8> = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;

    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}
