use anyhow::Result;
use image::RgbaImage;
use libframr::{FramrConnection, LogicalRegion, OutputImageFormat};
use std::io::Cursor;
use std::path::PathBuf;

use crate::cli::Cli;
use crate::config::{AppConfig, DefaultCaptureMethod};
use crate::selection;
use crate::selection::window::{Window, get_window_at_pos, get_windows};

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
) -> Result<(
	Vec<u8>,
	RgbaImage,
	Option<LogicalRegion>,
	Option<Vec<Window>>,
	OutputImageFormat,
)> {
	if let Some(secs) = cli.delay {
		std::thread::sleep(std::time::Duration::from_secs(secs));
	}

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

	let (image, region, cached_windows) = if cli.last {
		let conn = FramrConnection::new()?;
		let r = crate::app::load_last_region()?;
		(conn.screenshot_region(&r, cli.cursor)?, Some(r), None)
	} else {
		match method {
			Some(DefaultCaptureMethod::Area) => {
				let selection_cfg = cfg.map(|c| c.selection).unwrap_or_default();
				let ui = selection::SelectionUI::new(selection_cfg)?;
				let (r, img, windows) = ui
					.run(true)?
					.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;
				let img = img.ok_or_else(|| anyhow::anyhow!("Failed to capture image"))?;
				crate::app::save_last_region(&r);
				(img, Some(r), Some(windows))
			}
			Some(DefaultCaptureMethod::Screen) => {
				let conn = FramrConnection::new()?;
				let screen_num = screen.unwrap_or(0);
				let output = conn.get_output(screen_num)?;
				(conn.screenshot_output(&output, cli.cursor)?, None, None)
			}
			_ => {
				let conn = FramrConnection::new()?;
				(conn.screenshot_all(cli.cursor)?, None, None)
			}
		}
	};

	let mut buf = Cursor::new(Vec::with_capacity(
		(image.width() as usize * image.height() as usize).max(65536),
	));
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
			let raw = image.as_raw();
			let mut rgb = Vec::with_capacity((raw.len() / 4) * 3);
			for px in raw.chunks_exact(4) {
				rgb.extend_from_slice(&px[..3]);
			}
			image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality).write_image(
				&rgb,
				image.width(),
				image.height(),
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
	Ok((buf.into_inner(), image, region, cached_windows, image_format))
}

pub fn get_capture_path(
	cli: &Cli,
	_cfg: Option<&AppConfig>,
	region: Option<LogicalRegion>,
	cached_windows: Option<Vec<Window>>,
	image_format: OutputImageFormat,
) -> Result<(PathBuf, String)> {
	let ext = image_format.extension();
	let needs_windows = region.is_some()
		&& cli.output.as_deref() != Some(std::path::Path::new("-"))
		&& cli
			.filename
			.as_deref()
			.is_none_or(|p| p.contains("{window}"));
	let windows = if !needs_windows {
		Vec::new()
	} else if let Some(w) = cached_windows {
		w
	} else {
		get_windows().unwrap_or_default()
	};
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
