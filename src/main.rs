use anyhow::Result;
use clap::Parser;
use libframr::FramrConnection;

mod app;
mod cli;
mod config;
mod pidfile;
mod selection;
mod sound;
mod upload;
mod utils;

use crate::app::capture::{capture, get_capture_path};
use crate::app::record::record;
use crate::app::{handle_upload, resolve_action};
use crate::cli::{Cli, Commands, ConfigAction};
use crate::config::DefaultAction;
use crate::utils::clipboard::copy_to_clipboard;
use crate::utils::notify::send_notification;

fn main() -> Result<()> {
	sound::init_sound();
	let cli = Cli::parse();

	if let Some(ref uri) = cli.uri {
		return config::import_uploader(uri);
	}

	if cli.version {
		println!("framr v{}", env!("CARGO_PKG_VERSION"));
		return Ok(());
	}

	let cfg = config::load_config().ok();

	match cli.command {
		Some(Commands::Config { action }) => {
			return match action {
				Some(ConfigAction::Import { source }) => config::import_uploader(&source),
				Some(ConfigAction::List) => config::list_uploaders(),
				Some(ConfigAction::Show { uploader }) => config::show_uploader(&uploader),
				Some(ConfigAction::Create) => config::create_uploader(),
				Some(ConfigAction::Edit { uploader }) => config::edit_uploader(uploader.as_deref()),
				Some(ConfigAction::Delete { uploader }) => {
					config::delete_uploader(uploader.as_deref())
				}
				Some(ConfigAction::Default { uploader }) => {
					config::set_default_uploader(uploader.as_deref())
				}
				Some(ConfigAction::Action { action }) => {
					config::set_default_action(action.as_deref())
				}
				Some(ConfigAction::Capture { method }) => {
					config::set_default_capture(method.as_deref())
				}
				Some(ConfigAction::Sound { path }) => config::set_default_sound(path.as_deref()),
				Some(ConfigAction::Format { format }) => {
					config::set_default_format(format.as_deref())
				}
				Some(ConfigAction::Quality { quality }) => config::set_image_quality(quality),
				Some(ConfigAction::Protocol) => config::register_protocol_handler(),
				None => config::run_config_wizard(),
			};
		}
		Some(Commands::Upload {
			ref file,
			ref uploader,
			ref filename,
		}) => {
			return handle_upload(
				&cli,
				file.as_ref(),
				uploader.as_deref(),
				filename.as_deref(),
			);
		}
		None => {}
	}

	if cli.screens {
		let conn = FramrConnection::new()?;
		for (i, output) in conn.get_all_outputs()?.iter().enumerate() {
			println!("{}: {}", i, output);
		}
		return Ok(());
	}

	let action = resolve_action(&cli, cfg.as_ref());
	let is_upload_action =
		action == DefaultAction::Upload || action == DefaultAction::UploadAndCopy;

	let (bytes_opt, path, filename, is_image, mime_type) = if cli.record {
		let Some((path, filename)) = record(&cli, cfg.as_ref(), is_upload_action)? else {
			return Ok(());
		};
		(None, path, filename, false, "application/octet-stream")
	} else {
		let (img_bytes, region, img_format) = capture(&cli, cfg.as_ref())?;
		let (path, filename) = get_capture_path(&cli, cfg.as_ref(), region, img_format)?;
		(
			Some(img_bytes),
			path,
			filename,
			true,
			img_format.mime_type(),
		)
	};

	let payload = match &bytes_opt {
		Some(b) => upload::UploadPayload::Bytes {
			bytes: b,
			mime_type,
		},
		None => upload::UploadPayload::File(&path),
	};

	match action {
		DefaultAction::Upload | DefaultAction::UploadAndCopy => {
			let uploader_name = cli.upload.as_deref().filter(|s| !s.is_empty());
			let url = upload::upload(payload, uploader_name, &filename)?;
			println!("{}", url);

			send_notification("Upload Successful", &url, bytes_opt.as_deref(), cli.silent)?;

			if action == DefaultAction::UploadAndCopy {
				copy_to_clipboard(url.as_bytes().to_vec(), "text/plain;charset=utf-8")?;
			}

			if let Some(cfg) = &cfg {
				let _ = sound::play_sound(&cfg.upload_sound);
			}

			if !is_image && cli.output.is_none() {
				let _ = std::fs::remove_file(&path);
			}
		}
		DefaultAction::Copy => {
			if is_image {
				if let Some(ref bytes) = bytes_opt {
					copy_to_clipboard(bytes.clone(), mime_type)?;
					send_notification(
						"Copied to Clipboard",
						"Screenshot copied to clipboard",
						Some(bytes),
						cli.silent,
					)?;
				}
			} else {
				let p_str = path.to_string_lossy();
				copy_to_clipboard(p_str.as_bytes().to_vec(), "text/plain;charset=utf-8")?;
				send_notification(
					"Video Path Copied",
					"The path to the recording was copied to your clipboard",
					None,
					cli.silent,
				)?;
			}
		}
		DefaultAction::Save => {
			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent)?;
			}
			if is_image && let Some(ref bytes) = bytes_opt {
				std::fs::write(&path, bytes)?;
			}

			println!("{}", path.display());

			let title = if is_image {
				"Screenshot Saved"
			} else {
				"Recording Saved"
			};
			send_notification(
				title,
				&path.to_string_lossy(),
				bytes_opt.as_deref(),
				cli.silent,
			)?;
		}
	}

	Ok(())
}
