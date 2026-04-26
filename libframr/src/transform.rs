use image::{ImageBuffer, Rgba};
use wayland_client::protocol::wl_output::Transform;

pub fn apply_transform(
    image: ImageBuffer<Rgba<u8>, Vec<u8>>,
    transform: Transform,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    match transform {
        Transform::_90 => image::imageops::rotate90(&image),
        Transform::_180 => image::imageops::rotate180(&image),
        Transform::_270 => image::imageops::rotate270(&image),
        Transform::Flipped => image::imageops::flip_horizontal(&image),
        Transform::Flipped90 => {
            let flipped = image::imageops::flip_horizontal(&image);
            image::imageops::rotate90(&flipped)
        }
        Transform::Flipped180 => {
            let flipped = image::imageops::flip_horizontal(&image);
            image::imageops::rotate180(&flipped)
        }
        Transform::Flipped270 => {
            let flipped = image::imageops::flip_horizontal(&image);
            image::imageops::rotate270(&flipped)
        }
        _ => image,
    }
}

#[allow(dead_code)]
pub fn transformed_size(width: u32, height: u32, transform: Transform) -> (u32, u32) {
    match transform {
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
            (height, width)
        }
        _ => (width, height),
    }
}
