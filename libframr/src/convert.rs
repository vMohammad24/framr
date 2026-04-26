use image::ColorType;
use wayland_client::protocol::wl_shm::Format;

pub fn convert_to_rgba(data: &mut [u8], format: Format) -> Option<ColorType> {
	match format {
		Format::Xrgb8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk.swap(0, 2);
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		Format::Argb8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk.swap(0, 2);
			}
			Some(ColorType::Rgba8)
		}
		Format::Xbgr8888 => {
			for chunk in data.chunks_exact_mut(4) {
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		Format::Abgr8888 => Some(ColorType::Rgba8),
		Format::Xbgr2101010 => {
			for chunk in data.chunks_exact_mut(4) {
				let pixel = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);

				chunk[0] = ((pixel & 0x3FF) >> 2) as u8;
				chunk[1] = (((pixel >> 10) & 0x3FF) >> 2) as u8;
				chunk[2] = (((pixel >> 20) & 0x3FF) >> 2) as u8;
				chunk[3] = 255;
			}
			Some(ColorType::Rgba8)
		}
		Format::Abgr2101010 => {
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
		_ => None,
	}
}
#[allow(dead_code)]
pub fn is_format_supported(format: Format) -> bool {
	matches!(
		format,
		Format::Xrgb8888
			| Format::Argb8888
			| Format::Xbgr8888
			| Format::Abgr8888
			| Format::Xbgr2101010
			| Format::Abgr2101010
	)
}
