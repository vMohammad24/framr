use anyhow::Result;
use image::{GenericImageView, RgbaImage};
use notify_rust::Notification;

#[derive(Clone, Copy)]
pub enum NotificationImage<'a> {
	Rgba(&'a RgbaImage),
	Encoded(&'a [u8]),
}

fn thumbnail_dimensions(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
	if width <= max_width && height <= max_height {
		return (width.max(1), height.max(1));
	}
	if width * max_height >= height * max_width {
		let h = (height as u64 * max_width as u64 / width.max(1) as u64).max(1) as u32;
		(max_width, h)
	} else {
		let w = (width as u64 * max_height as u64 / height.max(1) as u64).max(1) as u32;
		(w, max_height)
	}
}

pub fn send_notification(
	title: &str,
	body: &str,
	image_data: Option<NotificationImage<'_>>,
	silent: bool,
) -> Result<()> {
	if silent {
		return Ok(());
	}

	let mut n = Notification::new();
	n.summary(title).body(body).appname("framr");

	match image_data {
		Some(NotificationImage::Rgba(img)) => {
			let (w, h) = img.dimensions();
			let (tw, th) = thumbnail_dimensions(w, h, 256, 256);
			let pixels = if tw == w && th == h {
				img.as_raw().clone()
			} else {
				image::imageops::thumbnail(img, tw, th).into_raw()
			};
			if let Ok(icon) = notify_rust::Image::from_rgba(tw as i32, th as i32, pixels) {
				n.image_data(icon);
			}
		}
		Some(NotificationImage::Encoded(bytes)) => {
			if let Ok(img) = image::load_from_memory(bytes) {
				let img = img.thumbnail(256, 256);
				let (width, height) = img.dimensions();
				let pixels = img.to_rgba8().into_raw();
				if let Ok(icon) = notify_rust::Image::from_rgba(width as i32, height as i32, pixels)
				{
					n.image_data(icon);
				}
			}
		}
		None => {}
	}

	let _ = n.show();
	Ok(())
}
