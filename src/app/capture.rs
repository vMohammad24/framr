use anyhow::Result;
use image::ImageFormat;
use libframr::{FramrConnection, LogicalRegion};
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

pub fn capture(cli: &Cli, cfg: Option<&AppConfig>) -> Result<(Vec<u8>, Option<LogicalRegion>)> {
	let conn = FramrConnection::new()?;

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

	let (image, region) = match method {
		Some(DefaultCaptureMethod::Area) => {
			let selection_cfg = cfg.map(|c| c.selection).unwrap_or_default();
			let ui = selection::SelectionUI::new(selection_cfg)?;
			let (r, img) = ui
				.run(true)?
				.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;
			let img = img.ok_or_else(|| anyhow::anyhow!("Failed to capture image"))?;
			(img, Some(r))
		}
		Some(DefaultCaptureMethod::Screen) => {
			let screen_num = screen.unwrap_or(0);
			let output = conn.get_output(screen_num)?;
			(conn.screenshot_output(&output, cli.cursor)?, None)
		}
		_ => (conn.screenshot_all(cli.cursor)?, None),
	};

	let mut buf = Cursor::new(Vec::new());
	image.write_to(&mut buf, ImageFormat::Png)?;
	Ok((buf.into_inner(), region))
}

pub fn get_capture_path(
	cli: &Cli,
	_cfg: Option<&AppConfig>,
	region: Option<LogicalRegion>,
) -> Result<(PathBuf, String)> {
	let windows = get_windows()?;
	let pos = region
		.map(|r| (r.position.x as f64, r.position.y as f64))
		.unwrap_or((0.0, 0.0));
	let active_window = get_window_at_pos(pos, &windows);
	let default = if active_window.is_some() {
		"{window}_%Y-%m-%d_%H-%M-%S.png"
	} else {
		"screenshot_%Y-%m-%d_%H-%M-%S.png"
	};
	let mut filename = resolve_output(cli, default, "png")
		.to_string_lossy()
		.to_string();

	if let Some(i) = active_window {
		let window = &windows[i];
		let title = if cli.upload.is_some() {
			&window.title.replace(" ", "_")
		} else {
			&window.title.replace("/", "_")
		};
		filename = filename.replace("{window}", title);
	}
	let path = match &cli.output {
		Some(dir) => dir.join(&filename),
		None => PathBuf::from(&filename),
	};
	Ok((path, filename))
}
