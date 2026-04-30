use std::io::{self, Cursor, IsTerminal, Read};
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use image::{GenericImageView, ImageFormat};
use libframr::FramrConnection;
use notify_rust::Notification;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

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

fn capture(cli: &Cli, cfg: Option<&config::AppConfig>) -> Result<Vec<u8>> {
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

	let image = match method {
		Some(config::DefaultCaptureMethod::Area) => {
			let ui = selection::SelectionUI::new()?;
			ui.run()?
				.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?
		}
		Some(config::DefaultCaptureMethod::Screen) => {
			let screen_num = screen.unwrap_or(0);
			let output = conn.get_output(screen_num)?;
			conn.screenshot_output(&output, cli.cursor)?
		}
		_ => conn.screenshot_all(cli.cursor)?,
	};

	let mut buf = Cursor::new(Vec::new());
	image.write_to(&mut buf, ImageFormat::Png)?;
	Ok(buf.into_inner())
}

fn handle_upload(
	cli: &Cli,
	file: Option<&PathBuf>,
	uploader: Option<&str>,
	name: Option<&str>,
) -> Result<()> {
	let (bytes, filename) = if let Some(path) = file {
		let bytes = std::fs::read(path)?;
		let filename = name
			.map(|n| n.to_string())
			.or_else(|| {
				path.file_name()
					.and_then(|n| n.to_str())
					.map(|n| n.to_string())
			})
			.unwrap_or_else(|| "file".to_string());
		(bytes, filename)
	} else {
		if io::stdin().is_terminal() {
			anyhow::bail!("No file specified. Provide a file path or pipe data to stdin.");
		}
		let mut bytes = Vec::new();
		io::stdin().read_to_end(&mut bytes)?;

		let filename = if let Some(n) = name {
			n.to_string()
		} else {
			let ext = infer::get(&bytes).map(|m| m.extension()).unwrap_or("bin");
			let pattern = format!("upload_%Y-%m-%d_%H-%M-%S.{}", ext);

			resolve_output(cli, &pattern, ext)
				.to_string_lossy()
				.into_owned()
		};
		(bytes, filename)
	};

	let url = upload::upload(&bytes, uploader, &filename)?;
	println!("{}", url);

	let is_image = infer::get(&bytes)
		.map(|m| m.matcher_type() == infer::MatcherType::Image)
		.unwrap_or(false);
	if is_image {
		notify("Upload Successful", &url, &bytes, cli.silent)?;
	} else {
		if !cli.silent {
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

	let png_bytes = capture(&cli, cfg.as_ref())?;
	let filename = resolve_output(&cli, "screenshot_%Y-%m-%d_%H-%M-%S.png", "png")
		.to_string_lossy()
		.to_string();

	if let Some(ref uploader_name) = cli.upload {
		let name = if uploader_name.is_empty() {
			None
		} else {
			Some(uploader_name.as_str())
		};
		let url = upload::upload(&png_bytes, name, &filename)?;
		println!("{}", url);
		notify("Upload Successful", &url, &png_bytes, cli.silent)?;
		if cli.copy {
			copy_to_clipboard_text(&url)?;
		}
		return Ok(());
	}

	if cli.copy {
		copy_to_clipboard_image(png_bytes.clone())?;
		notify(
			"Copied to Clipboard",
			"Screenshot copied to clipboard",
			&png_bytes,
			cli.silent,
		)?;
		return Ok(());
	}

	if let Some(ref cfg) = cfg
		&& let Some(action) = cfg.default_action
		&& cli.output.is_none()
	{
		use config::DefaultAction;
		match action {
			DefaultAction::Save => {}
			DefaultAction::Copy => {
				copy_to_clipboard_image(png_bytes.clone())?;
				notify(
					"Copied to Clipboard",
					"Screenshot copied to clipboard",
					&png_bytes,
					cli.silent,
				)?;
				return Ok(());
			}
			DefaultAction::Upload => {
				let url = upload::upload(&png_bytes, None, &filename)?;
				println!("{}", url);
				notify("Upload Successful", &url, &png_bytes, cli.silent)?;
				return Ok(());
			}
			DefaultAction::UploadAndCopy => {
				let url = upload::upload(&png_bytes, None, &filename)?;
				println!("{}", url);
				notify("Upload Successful", &url, &png_bytes, cli.silent)?;
				copy_to_clipboard_text(&url)?;
				return Ok(());
			}
		}
	}

	let path = match &cli.output {
		Some(dir) => {
			std::fs::create_dir_all(dir)?;
			dir.join(&filename)
		}
		None => PathBuf::from(&filename),
	};

	std::fs::write(&path, &png_bytes)?;
	println!("{}", path.display());
	notify(
		"Screenshot Saved",
		&path.to_string_lossy(),
		&png_bytes,
		cli.silent,
	)?;
	Ok(())
}
