use std::io::{self, Cursor, IsTerminal, Read};
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use image::{GenericImageView, ImageFormat};
use libframr::{FramrConnection, LogicalRegion};
use notify_rust::Notification;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

use crate::config::DefaultAction;
use crate::selection::window::{get_window_at_pos, get_windows};

mod config;
mod selection;
mod upload;

#[derive(Parser)]
#[command(name = "framr")]
struct Cli {
	#[command(subcommand)]
	command: Option<Commands>,

	/// Screen to capture
	#[arg(short, long)]
	screen: Option<usize>,

	/// Version
	#[arg(short, long)]
	version: bool,

	/// List available screens
	#[arg(long)]
	screens: bool,

	/// Select an area to capture
	#[arg(short, long)]
	area: bool,

	/// Copy to clipboard
	#[arg(short, long)]
	copy: bool,

	/// Output directory (defaults to current directory)
	#[arg(short, long)]
	output: Option<PathBuf>,

	/// Filename (e.g. "screenshot_%Y-%m-%d.png")
	#[arg(long)]
	filename: Option<String>,

	/// Include cursor in capture
	#[arg(long)]
	cursor: bool,

	/// Record video
	#[arg(short, long)]
	record: bool,

	/// Upload screenshot (uses default uploader, or specify with -u <name>)
	#[arg(short = 'u', long, num_args = 0..=1, default_missing_value = "")]
	upload: Option<String>,

	/// Deeplink URI (framr://...)
	#[arg(value_parser = parse_framr_uri)]
	uri: Option<String>,

	/// Silent mode (no notifications)
	#[arg(long, global = true)]
	silent: bool,
}

fn parse_framr_uri(arg: &str) -> Result<String, String> {
	if arg.starts_with("framr://") {
		Ok(arg.to_string())
	} else {
		Err("Positional arguments must be a valid framr:// URI".to_string())
	}
}

#[derive(Subcommand)]
enum Commands {
	/// Configure uploaders
	Config {
		#[command(subcommand)]
		action: Option<ConfigAction>,
	},
	/// Upload a file or byte data from stdin
	Upload {
		/// Path to the file to upload (omit to read from stdin)
		file: Option<PathBuf>,

		/// Uploader to use (uses default if not specified)
		#[arg(short = 'u', long)]
		uploader: Option<String>,

		/// Filename to use for the upload
		#[arg(short = 'n', long)]
		filename: Option<String>,
	},
}

#[derive(Subcommand)]
enum ConfigAction {
	/// Import uploader from .sxcu/.iscu file or URL
	Import {
		/// Path to .sxcu/.iscu file or URL
		source: String,
	},
	/// List all configured uploaders
	List,
	/// Show detailed info about an uploader
	Show {
		/// Name or number of the uploader
		uploader: String,
	},
	/// Create a new uploader interactively
	Create,
	/// Edit an existing uploader interactively
	Edit {
		/// Name or number of the uploader (omitting prompts for selection)
		uploader: Option<String>,
	},
	/// Delete an uploader
	Delete {
		/// Name or number of the uploader (omitting prompts for selection)
		uploader: Option<String>,
	},
	/// Set the default uploader
	Default {
		/// Name or number of the uploader (omitting prompts for selection)
		uploader: Option<String>,
	},
	/// Set the default action
	Action {
		/// Name of the action (omitting prompts for selection)
		action: Option<String>,
	},
	/// Set the default capture method
	Capture {
		/// Name of the method (omitting prompts for selection)
		method: Option<String>,
	},
	/// Register the framr:// protocol handler
	Protocol,
}

fn resolve_output(cli: &Cli, default_pattern: &str, default_ext: &str) -> PathBuf {
	let pattern = cli.filename.as_deref().unwrap_or(default_pattern);
	let filename = chrono::Local::now().format(pattern).to_string();

	let mut path = PathBuf::from(filename);
	if path.extension().is_none() {
		path.set_extension(default_ext);
	}
	path
}

fn copy_to_clipboard_image(png_bytes: Vec<u8>) -> Result<()> {
	match unsafe { libc::fork() } {
		-1 => anyhow::bail!("fork failed"),
		0 => {
			let mut clipboard_opts = ClipboardOptions::new();
			clipboard_opts.foreground(true).seat(Seat::All);
			let _ = clipboard_opts.copy(
				Source::Bytes(png_bytes.into()),
				MimeType::Specific("image/png".into()),
			);
			std::process::exit(0);
		}
		_ => {}
	}
	Ok(())
}

fn copy_to_clipboard_text(text: &str) -> Result<()> {
	match unsafe { libc::fork() } {
		-1 => anyhow::bail!("fork failed"),
		0 => {
			let mut clipboard_opts = ClipboardOptions::new();
			clipboard_opts.foreground(true).seat(Seat::All);
			let _ = clipboard_opts.copy(
				Source::Bytes(text.as_bytes().to_vec().into()),
				MimeType::Specific("text/plain;charset=utf-8".into()),
			);
			std::process::exit(0);
		}
		_ => {}
	}
	Ok(())
}

fn capture(cli: &Cli, cfg: Option<&config::AppConfig>) -> Result<(Vec<u8>, Option<LogicalRegion>)> {
	let conn = FramrConnection::new()?;

	let (method, screen) = if cli.area {
		(Some(config::DefaultCaptureMethod::Area), None)
	} else if let Some(s) = cli.screen {
		(Some(config::DefaultCaptureMethod::Screen), Some(s))
	} else if let Some(cfg) = cfg {
		(
			cfg.default_capture
				.or(Some(config::DefaultCaptureMethod::Full)),
			cfg.default_screen,
		)
	} else {
		(Some(config::DefaultCaptureMethod::Full), None)
	};

	let (image, region) = match method {
		Some(config::DefaultCaptureMethod::Area) => {
			let selection_cfg = cfg.map(|c| c.selection).unwrap_or_default();
			let ui = selection::SelectionUI::new(selection_cfg)?;
			let (r, img) = ui
				.run(true)?
				.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;
			let img = img.ok_or_else(|| anyhow::anyhow!("Failed to capture image"))?;
			(img, Some(r))
		}
		Some(config::DefaultCaptureMethod::Screen) => {
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

fn handle_upload(
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
		if is_image {
			let image_data = match file {
				Some(p) => std::fs::read(p)?,
				None => stdin_bytes.clone(),
			};
			notify("Upload Successful", &url, &image_data, cli.silent)?;
		} else {
			let _ = Notification::new()
				.summary("Upload Successful")
				.body(&url)
				.appname("framr")
				.show();
		}
	}

	if cli.copy {
		copy_to_clipboard_text(&url)?;
	}
	Ok(())
}

fn notify(title: &str, body: &str, bytes: &[u8], silent: bool) -> Result<()> {
	if silent {
		return Ok(());
	}

	let _ = (|| -> Result<()> {
		let img = image::load_from_memory(bytes)?;
		let (width, height) = img.dimensions();
		let pixels = img.to_rgba8().into_raw();

		Notification::new()
			.summary(title)
			.body(body)
			.appname("framr")
			.image_data(notify_rust::Image::from_rgba(
				width as i32,
				height as i32,
				pixels,
			)?)
			.show()?;
		Ok(())
	})();

	Ok(())
}

fn main() -> Result<()> {
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

	let action = if cli.upload.is_some() {
		if cli.copy {
			DefaultAction::UploadAndCopy
		} else {
			DefaultAction::Upload
		}
	} else if cli.copy {
		DefaultAction::Copy
	} else if let Some(ref c) = cfg {
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
	};

	let is_upload_action =
		action == DefaultAction::Upload || action == DefaultAction::UploadAndCopy;

	let (bytes_opt, path, filename, is_image) = if cli.record {
		let conn = FramrConnection::new()?;

		let filename = resolve_output(&cli, "recording_%Y-%m-%d_%H-%M-%S.mp4", "mp4")
			.to_string_lossy()
			.to_string();

		let path = if is_upload_action && cli.output.is_none() {
			std::env::temp_dir().join(&filename)
		} else {
			let p = match &cli.output {
				Some(dir) => dir.join(&filename),
				None => PathBuf::from(&filename),
			};
			if let Some(parent) = p.parent() {
				std::fs::create_dir_all(parent)?;
			}
			p
		};

		let handle = if let Some(screen_num) = cli.screen {
			let output = conn.get_output(screen_num)?;
			conn.start_recording(&output, None, cli.cursor, path.clone())?
		} else {
			let selection_cfg = cfg.as_ref().map(|c| c.selection).unwrap_or_default();
			let ui = selection::SelectionUI::new(selection_cfg)?;
			let (region, _) = ui
				.run(false)?
				.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;

			conn.start_recording_region(&region, cli.cursor, path.clone())?
		};

		println!("Recording to {}... Press Ctrl+C to stop.", path.display());

		let (tx, rx) = std::sync::mpsc::channel();
		ctrlc::set_handler(move || {
			let _ = tx.send(());
		})?;

		loop {
			if rx
				.recv_timeout(std::time::Duration::from_millis(100))
				.is_ok()
			{
				println!("\nStopping recording...");
				break;
			}
			if handle.pipeline_thread.is_finished() {
				println!("\nRecording stopped unexpectedly.");
				break;
			}
		}

		let _ = handle.stop_sender.send(());
		handle
			.pipeline_thread
			.join()
			.map_err(|_| anyhow::anyhow!("Pipeline thread panicked"))??;

		(None, path, filename, false)
	} else {
		let (png_bytes, region) = capture(&cli, cfg.as_ref())?;
		let windows = get_windows()?;
		let pos = region
			.map(|r| (r.position.x as f64, r.position.y as f64))
			.unwrap_or((0.0, 0.0));
		let active_window = get_window_at_pos(pos, &windows);
		let mut filename = resolve_output(&cli, "{window}_%Y-%m-%d_%H-%M-%S.png", "png")
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
		(Some(png_bytes), path, filename, true)
	};

	let payload = match &bytes_opt {
		Some(b) => upload::UploadPayload::Bytes(b),
		None => upload::UploadPayload::File(&path),
	};

	match action {
		DefaultAction::Upload | DefaultAction::UploadAndCopy => {
			let uploader_name = cli.upload.as_deref().filter(|s| !s.is_empty());
			let url = upload::upload(payload, uploader_name, &filename)?;
			println!("{}", url);

			if is_image {
				if let Some(ref b) = bytes_opt {
					notify("Upload Successful", &url, b, cli.silent)?;
				} else {
					if !cli.silent {
						let _ = Notification::new()
							.summary("Upload Successful")
							.body(&url)
							.appname("framr")
							.show();
					}
				}
			} else {
				if !cli.silent {
					let _ = Notification::new()
						.summary("Upload Successful")
						.body(&url)
						.appname("framr")
						.show();
				}
			}

			if action == DefaultAction::UploadAndCopy {
				copy_to_clipboard_text(&url)?;
			}

			if !is_image && cli.output.is_none() {
				let _ = std::fs::remove_file(&path);
			}
		}
		DefaultAction::Copy => {
			if is_image {
				if let Some(ref bytes) = bytes_opt {
					copy_to_clipboard_image(bytes.clone())?;
				}
				if let Some(ref b) = bytes_opt {
					notify(
						"Copied to Clipboard",
						"Screenshot copied to clipboard",
						b,
						cli.silent,
					)?;
				}
			} else {
				copy_to_clipboard_text(&path.to_string_lossy())?;
				if !cli.silent {
					let _ = Notification::new()
						.summary("Video Path Copied")
						.body("The path to the recording was copied to your clipboard")
						.appname("framr")
						.show();
				}
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

			if is_image {
				if let Some(ref b) = bytes_opt {
					notify("Screenshot Saved", &path.to_string_lossy(), b, cli.silent)?;
				} else {
					if !cli.silent {
						let _ = Notification::new()
							.summary("Recording Saved")
							.body(&path.to_string_lossy())
							.appname("framr")
							.show();
					}
				}
			} else {
				if !cli.silent {
					let _ = Notification::new()
						.summary("Recording Saved")
						.body(&path.to_string_lossy())
						.appname("framr")
						.show();
				}
			}
		}
	}

	Ok(())
}
