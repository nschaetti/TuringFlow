use std::error::Error;
use std::io::Cursor;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageOutputFormat;

pub fn encode_image_base64(path: impl AsRef<Path>) -> Result<String, Box<dyn Error>> {
    let image = image::open(path)?;
    let mut buffer = Vec::new();
    image.write_to(&mut Cursor::new(&mut buffer), ImageOutputFormat::Png)?;
    Ok(STANDARD.encode(&buffer))
}
