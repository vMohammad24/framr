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
	let payload: upload::UploadPayload;
	let filename: String;
	let is_image: bool;

	let mut stdin_bytes = Vec::new();

	if let Some(path) = file {
		payload = upload::UploadPayload::File(path.as_path());
		filename = name
			.map(|n| n.to_string())
			.or_else(|| {
				path.file_name()
					.and_then(|n| n.to_str())
					.map(|n| n.to_string())
			})
			.unwrap_or_else(|| "file".to_string());

		let mut f = std::fs::File::open(path)?;
		let mut header = [0; 8192];
		let n = f.read(&mut header).unwrap_or(0);
		is_image = infer::get(&header[..n])
			.map(|m| m.matcher_type() == infer::MatcherType::Image)
			.unwrap_or(false);
	} else {
		if io::stdin().is_terminal() {
			anyhow::bail!("No file specified. Provide a file path or pipe data to stdin.");
		}
		io::stdin().read_to_end(&mut stdin_bytes)?;
		payload = upload::UploadPayload::Bytes(&stdin_bytes);

		is_image = infer::get(&stdin_bytes)
			.map(|m| m.matcher_type() == infer::MatcherType::Image)
			.unwrap_or(false);

		filename = if let Some(n) = name {
			n.to_string()
		} else {
			let ext = infer::get(&stdin_bytes)
				.map(|m| m.extension())
				.unwrap_or("bin");
			let pattern = format!("upload_%Y-%m-%d_%H-%M-%S.{}", ext);
			resolve_output(cli, &pattern, ext)
				.to_string_lossy()
				.into_owned()
		};
	};

	let url = upload::upload(payload, uploader, &filename)?;
	println!("{}", url);

	if !cli.silent {
		let image_data = if is_image {
			match file {
				Some(p) => Some(std::fs::read(p)?),
				None => Some(stdin_bytes.clone()),
			}
		} else {
			None
		};
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

pub fn resolve_action(cli: &Cli, cfg: Option<&AppConfig>) -> DefaultAction {
	if cli.upload.is_some() {
		if cli.copy {
			DefaultAction::UploadAndCopy
		} else {
			DefaultAction::Upload
		}
	} else if cli.copy {
		DefaultAction::Copy
	} else if let Some(c) = cfg {
		if let Some(def_act) = &c.default_action {
			if cli.output.is_none() {
				match def_act {
					config::DefaultAction::Save => DefaultAction::Save,
					config::DefaultAction::Copy => DefaultAction::Copy,
					config::DefaultAction::Upload => DefaultAction::Upload,
					config::DefaultAction::UploadAndCopy => DefaultAction::UploadAndCopy,
				}
			} else {
				DefaultAction::Save
			}
		} else {
			DefaultAction::Save
		}
	} else {
		DefaultAction::Save
	}
}
