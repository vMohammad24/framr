use anyhow::Result;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;

pub mod capture;
pub mod record;

use crate::app::capture::resolve_output;
use crate::cli::Cli;
use crate::config::{AppConfig, DefaultAction};
use crate::utils::clipboard::copy_to_clipboard;
use crate::utils::notify::send_notification;
use crate::{config, sound, upload};

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

	if !cli.silent {
		let image_data = is_image.then(|| match file {
			Some(p) => std::fs::read(p).unwrap_or_default(),
			None => stdin_bytes.clone(),
		});
		send_notification("Upload Successful", &url, image_data.as_deref(), cli.silent)?;
	}

	if let Ok(cfg) = config::load_config() {
		let _ = sound::play_sound(&cfg.upload_sound);
	}

	if cli.copy {
		copy_to_clipboard(url.as_bytes().to_vec(), "text/plain;charset=utf-8")?;
	}
	Ok(())
}

fn is_image_data(bytes: &[u8]) -> bool {
	infer::get(bytes)
		.map(|m| m.matcher_type() == infer::MatcherType::Image)
		.unwrap_or(false)
}

pub fn resolve_action(cli: &Cli, cfg: Option<&AppConfig>) -> DefaultAction {
	if cli.upload.is_some() {
		return if cli.copy {
			DefaultAction::UploadAndCopy
		} else {
			DefaultAction::Upload
		};
	}
	if cli.copy {
		return DefaultAction::Copy;
	}
	if cli.output.is_some() {
		return DefaultAction::Save;
	}
	cfg.and_then(|c| c.default_action)
		.unwrap_or(DefaultAction::Save)
}
