use anyhow::Result;
use image::GenericImageView;
use notify_rust::Notification;

pub fn send_notification(
	title: &str,
	body: &str,
	image_data: Option<&[u8]>,
	silent: bool,
) -> Result<()> {
	if silent {
		return Ok(());
	}

	let mut n = Notification::new();
	n.summary(title).body(body).appname("framr");

	if let Some(bytes) = image_data
		&& let Ok(img) = image::load_from_memory(bytes)
	{
		let (width, height) = img.dimensions();
		let pixels = img.to_rgba8().into_raw();
		if let Ok(icon) = notify_rust::Image::from_rgba(width as i32, height as i32, pixels) {
			n.image_data(icon);
		}
	}

	let _ = n.show();
	Ok(())
}
