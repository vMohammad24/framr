use anyhow::Result;
use libframr::LogicalRegion;
#[cfg(feature = "upload")]
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

pub mod capture;
pub mod record;

#[cfg(feature = "upload")]
use crate::app::capture::resolve_output;
use crate::cli::Cli;
#[cfg(feature = "upload")]
use crate::config::NotificationKind;
use crate::config::{AppConfig, DefaultAction};
#[cfg(feature = "upload")]
use crate::utils::clipboard::copy_to_clipboard;
#[cfg(feature = "upload")]
use crate::utils::notify::{NotificationImage, send_notification};
#[cfg(feature = "upload")]
use crate::{config, sound, upload};

#[cfg(feature = "upload")]
pub fn handle_upload(
	cli: &Cli,
	file: Option<&PathBuf>,
	uploader: Option<&str>,
	name: Option<&str>,
) -> Result<()> {
	let mut stdin_bytes = Vec::new();

	let (payload, filename, is_image) = if let Some(path) = file {
		let filename = name
			.map(|n| n.to_string())
			.or_else(|| {
				path.file_name()
					.and_then(|n| n.to_str())
					.map(|n| n.to_string())
			})
			.unwrap_or_else(|| "file".to_string());

		let is_image = std::fs::File::open(path)
			.map(|mut f| {
				let mut header = [0; 8192];
				let n = f.read(&mut header).unwrap_or(0);
				is_image_data(&header[..n])
			})
			.unwrap_or(false);

		(
			upload::UploadPayload::File(path.as_path()),
			filename,
			is_image,
		)
	} else {
		if io::stdin().is_terminal() {
			anyhow::bail!("No file specified. Provide a file path or pipe data to stdin.");
		}
		io::stdin().read_to_end(&mut stdin_bytes)?;
		let info = infer::get(&stdin_bytes);
		let is_image = info
			.map(|m| m.matcher_type() == infer::MatcherType::Image)
			.unwrap_or(false);

		let filename = if let Some(n) = name {
			n.to_string()
		} else {
			let ext = info.map(|m| m.extension()).unwrap_or("bin");
			let pattern = format!("upload_%Y-%m-%d_%H-%M-%S.{}", ext);
			resolve_output(cli, &pattern, ext)
				.to_string_lossy()
				.into_owned()
		};

		(
			upload::UploadPayload::Bytes {
				bytes: &stdin_bytes,
				mime_type: info
					.map(|m| m.mime_type())
					.unwrap_or("application/octet-stream"),
			},
			filename,
			is_image,
		)
	};

	let url = upload::upload(payload, uploader, &filename)?;
	println!("{}", url);

	if cli.copy {
		copy_to_clipboard(url.as_bytes(), "text/plain;charset=utf-8")?;
	}

	let cfg = config::load_config().ok();
	let upload_silent = cli.silent
		|| cfg
			.as_ref()
			.is_some_and(|cfg| cfg.notification_is_silent(NotificationKind::Upload));

	if !upload_silent {
		let file_bytes;
		let image_data = if is_image {
			match file {
				Some(p) => {
					file_bytes = std::fs::read(p).unwrap_or_default();
					Some(NotificationImage::Encoded(file_bytes.as_slice()))
				}
				None => Some(NotificationImage::Encoded(stdin_bytes.as_slice())),
			}
		} else {
			None
		};
		send_notification("Upload Successful", &url, image_data, false)?;
	}

	if let Some(cfg) = cfg {
		let _ = sound::play_sound(&cfg.upload_sound);
	}
	Ok(())
}

fn last_region_path() -> PathBuf {
	dirs::runtime_dir()
		.unwrap_or_else(std::env::temp_dir)
		.join("framr-last-region")
}

#[cfg(feature = "annotations")]
pub fn save_last_region(region: &LogicalRegion) {
	let _ = std::fs::write(
		last_region_path(),
		format!(
			"{} {} {} {}",
			region.position.x, region.position.y, region.size.width, region.size.height
		),
	);
}

fn last_region_error() -> anyhow::Error {
	#[cfg(feature = "annotations")]
	let hint = "Select an area with -a first.";
	#[cfg(not(feature = "annotations"))]
	let hint = "Select an area with an annotations-enabled build first.";
	anyhow::anyhow!("No previous region saved. {hint}")
}

fn last_region_corrupt() -> anyhow::Error {
	#[cfg(feature = "annotations")]
	let hint = "Select an area with -a to rewrite it.";
	#[cfg(not(feature = "annotations"))]
	let hint = "Select an area with an annotations-enabled build to rewrite it.";
	anyhow::anyhow!("Invalid last-region file. {hint}")
}

pub fn load_last_region() -> Result<LogicalRegion> {
	let content = std::fs::read_to_string(last_region_path()).map_err(|_| last_region_error())?;
	let parts: Vec<&str> = content.split_whitespace().collect();
	if parts.len() != 4 {
		return Err(last_region_corrupt());
	}
	Ok(LogicalRegion::new(
		parts[0].parse()?,
		parts[1].parse()?,
		parts[2].parse()?,
		parts[3].parse()?,
	))
}

#[cfg(feature = "upload")]
fn is_image_data(bytes: &[u8]) -> bool {
	infer::get(bytes)
		.map(|m| m.matcher_type() == infer::MatcherType::Image)
		.unwrap_or(false)
}

pub fn resolve_action(cli: &Cli, cfg: Option<&AppConfig>) -> DefaultAction {
	#[cfg(feature = "upload")]
	if cli.upload.is_some() {
		return if cli.copy {
			DefaultAction::UploadAndCopy
		} else {
			DefaultAction::Upload
		};
	}
	if cli.copy {
		return if cli.output.is_some() {
			DefaultAction::SaveAndCopy
		} else {
			DefaultAction::Copy
		};
	}
	if cli.output.is_some() {
		return DefaultAction::Save;
	}
	cfg.and_then(|c| c.default_action)
		.unwrap_or(DefaultAction::Save)
}
