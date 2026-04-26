use std::io::Cursor;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use image::ImageFormat;
use libframr::{FramrConnection, LogicalRegion};
use slurp_rs::SelectOptions;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

mod config;

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
}

#[derive(Subcommand)]
enum Commands {
	/// Configure uploaders
	Config {
		#[command(subcommand)]
		action: Option<ConfigAction>,
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

fn capture(cli: &Cli) -> Result<Vec<u8>> {
	let conn = FramrConnection::new()?;
	let image = if cli.area {
		let selection = slurp_rs::select_region(SelectOptions::default())?;
		let rect = &selection.rect;
		let region = LogicalRegion::new(rect.x, rect.y, rect.width as u32, rect.height as u32);

		let outputs = conn.get_all_outputs();
		let output = outputs
			.iter()
			.find(|o| {
				let ox = o.logical_position.x;
				let oy = o.logical_position.y;
				let ow = o.logical_size.width as i32;
				let oh = o.logical_size.height as i32;
				rect.x >= ox
					&& rect.y >= oy && rect.x + rect.width as i32 <= ox + ow
					&& rect.y + rect.height as i32 <= oy + oh
			})
			.or_else(|| outputs.first())
			.ok_or_else(|| anyhow::anyhow!("no output found for region"))?;

		conn.screenshot_region(output, &region, cli.cursor)?
	} else if let Some(screen_num) = cli.screen {
		let output = conn.get_output(screen_num)?;
		conn.screenshot_output(output, cli.cursor)?
	} else {
		conn.screenshot_all(cli.cursor)?
	};

	let mut buf = Cursor::new(Vec::new());
	image.write_to(&mut buf, ImageFormat::Png)?;
	Ok(buf.into_inner())
}

fn main() -> Result<()> {
	let cli = Cli::parse();

	match cli.command {
		Some(Commands::Config { action }) => {
			return match action {
				Some(ConfigAction::Import { source }) => config::import_uploader(&source),
				Some(ConfigAction::List) => config::list_uploaders(),
				Some(ConfigAction::Show { uploader }) => config::show_uploader(&uploader),
				Some(ConfigAction::Create) => config::create_uploader(),
				Some(ConfigAction::Edit { uploader }) => {
					config::edit_uploader(uploader.as_deref())
				}
				Some(ConfigAction::Delete { uploader }) => {
					config::delete_uploader(uploader.as_deref())
				}
				Some(ConfigAction::Default { uploader }) => {
					config::set_default_uploader(uploader.as_deref())
				}
				None => config::run_config_wizard(),
			};
		}
		None => {}
	}

	if cli.screens {
		let conn = FramrConnection::new()?;
		for (i, output) in conn.get_all_outputs().iter().enumerate() {
			println!("{}: {}", i, output);
		}
		return Ok(());
	}

	let png_bytes = capture(&cli)?;

	if cli.copy {
		copy_to_clipboard_image(png_bytes)?;
		return Ok(());
	}

	let path = match &cli.output {
		Some(dir) => {
			std::fs::create_dir_all(dir)?;
			let filename = resolve_output(&cli, "screenshot_%Y-%m-%d_%H-%M-%S.png", "png");
			dir.join(&filename)
		}
		None => resolve_output(&cli, "screenshot_%Y-%m-%d_%H-%M-%S.png", "png"),
	};

	std::fs::write(&path, &png_bytes)?;

	Ok(())
}
