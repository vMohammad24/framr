use anyhow::Result;
use libframr::{FramrConnection, LogicalRegion, OutputImageFormat};
use std::io::Cursor;
use std::path::PathBuf;

use crate::cli::Cli;
use crate::config::{AppConfig, DefaultCaptureMethod};
use crate::selection;
use crate::selection::window::{get_window_at_pos, get_windows};

pub fn resolve_output(cli: &Cli, default_pattern: &str, default_ext: &str) -> PathBuf {
	let pattern = cli.filename.as_deref().unwrap_or(default_pattern);
	let filename = chrono::Local::now().format(pattern).to_string();

	let mut path = PathBuf::from(filename);
	if path.extension().is_none() {
		path.set_extension(default_ext);
	}
	path
}

pub fn capture(
	cli: &Cli,
	cfg: Option<&AppConfig>,
) -> Result<(Vec<u8>, Option<LogicalRegion>, OutputImageFormat)> {
	if let Some(secs) = cli.delay {
		std::thread::sleep(std::time::Duration::from_secs(secs));
	}

	let conn = FramrConnection::new()?;

	let image_format = cfg.and_then(|c| c.image_format).unwrap_or_default();

	let (method, screen) = if cli.area {
		(Some(DefaultCaptureMethod::Area), None)
	} else if let Some(s) = cli.screen {
		(Some(DefaultCaptureMethod::Screen), Some(s))
	} else if let Some(cfg) = cfg {
		(
			cfg.default_capture.or(Some(DefaultCaptureMethod::Full)),
			cfg.default_screen,
		)
	} else {
		(Some(DefaultCaptureMethod::Full), None)
	};

	let (image, region) = if cli.last {
		let r = crate::app::load_last_region()?;
		(conn.screenshot_region(&r, cli.cursor)?, Some(r))
	} else {
		match method {
			Some(DefaultCaptureMethod::Area) => {
				let selection_cfg = cfg.map(|c| c.selection).unwrap_or_default();
				let ui = selection::SelectionUI::new(selection_cfg)?;
				let (r, img) = ui
					.run(true)?
					.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;
				let img = img.ok_or_else(|| anyhow::anyhow!("Failed to capture image"))?;
				crate::app::save_last_region(&r);
				(img, Some(r))
			}
			Some(DefaultCaptureMethod::Screen) => {
				let screen_num = screen.unwrap_or(0);
				let output = conn.get_output(screen_num)?;
				(conn.screenshot_output(&output, cli.cursor)?, None)
			}
			_ => (conn.screenshot_all(cli.cursor)?, None),
		}
	};

	let mut buf = Cursor::new(Vec::new());
	let quality = cfg.and_then(|c| c.image_quality).unwrap_or(90);

	use image::ImageEncoder;
	match image_format {
		OutputImageFormat::Png => {
			image::codecs::png::PngEncoder::new(&mut buf).write_image(
				image.as_raw(),
				image.width(),
				image.height(),
				image::ColorType::Rgba8.into(),
			)?;
		}
		OutputImageFormat::Jpeg => {
			let rgb_image = image::DynamicImage::ImageRgba8(image).to_rgb8();
			image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality).write_image(
				&rgb_image,
				rgb_image.width(),
				rgb_image.height(),
				image::ColorType::Rgb8.into(),
			)?;
		}
		OutputImageFormat::WebP => {
			image::codecs::webp::WebPEncoder::new_lossless(&mut buf).write_image(
				image.as_raw(),
				image.width(),
				image.height(),
				image::ColorType::Rgba8.into(),
			)?;
		}
	}
	Ok((buf.into_inner(), region, image_format))
}

pub fn get_capture_path(
	cli: &Cli,
	_cfg: Option<&AppConfig>,
	region: Option<LogicalRegion>,
	image_format: OutputImageFormat,
) -> Result<(PathBuf, String)> {
	let ext = image_format.extension();
	let windows = get_windows().unwrap_or_default();
	let active_window = region
		.and_then(|r| get_window_at_pos((r.position.x as f64, r.position.y as f64), &windows));
	let default = if active_window.is_some() {
		format!("{{window}}_%Y-%m-%d_%H-%M-%S.{}", ext)
	} else {
		format!("screenshot_%Y-%m-%d_%H-%M-%S.{}", ext)
	};
	let mut filename = resolve_output(cli, &default, ext)
		.to_string_lossy()
		.to_string();

	if let Some(i) = active_window {
		let window = &windows[i];
		let title = &window.title.replace("/", "_");
		filename = filename.replace("{window}", title);
	}
	let path = match &cli.output {
		Some(dir) if dir.as_os_str() == "-" => PathBuf::from("-"),
		Some(dir) => dir.join(&filename),
		None => PathBuf::from(&filename),
	};
	Ok((path, filename))
}
