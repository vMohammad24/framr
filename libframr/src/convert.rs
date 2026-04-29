use crate::output::PixelFormat;
use image::ColorType;

pub fn convert_to_rgba(data: &mut [u8], format: PixelFormat) -> Option<ColorType> {
	match format {
		PixelFormat::Xrgb8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk.swap(0, 2);
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		PixelFormat::Argb8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk.swap(0, 2);
			}
			Some(ColorType::Rgba8)
		}
		PixelFormat::Xbgr8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		PixelFormat::Abgr8888 => Some(ColorType::Rgba8),
		PixelFormat::Xbgr2101010 => {
			for chunk in data.chunks_exact_mut(4) {
				let pixel = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);

				chunk[0] = ((pixel & 0x3FF) >> 2) as u8;
				chunk[1] = (((pixel >> 10) & 0x3FF) >> 2) as u8;
				chunk[2] = (((pixel >> 20) & 0x3FF) >> 2) as u8;
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		PixelFormat::Abgr2101010 => {
			for chunk in data.chunks_exact_mut(4) {
				let pixel = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);

				chunk[0] = ((pixel & 0x3FF) >> 2) as u8;
				chunk[1] = (((pixel >> 10) & 0x3FF) >> 2) as u8;
				chunk[2] = (((pixel >> 20) & 0x3FF) >> 2) as u8;

				let a = (pixel >> 30) & 0x3;
				chunk[3] = (a * 85) as u8;
			}
			Some(ColorType::Rgba8)
		}
	}
}
