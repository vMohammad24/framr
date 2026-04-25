use std::io::Cursor;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use image::ImageFormat;
use libwayshot::region::{Position, Region};
use libwayshot::{LogicalRegion, WayshotConnection};
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

	/// Copy screenshot to clipboard without saving
	#[arg(short, long)]
	copy: bool,

	/// Output directory (defaults to current directory)
	#[arg(short, long)]
	output: Option<PathBuf>,

	/// Filename (e.g. "screenshot_%Y-%m-%d.png")
	#[arg(long)]
	filename: Option<String>,
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

fn capture(cli: &Cli) -> Result<Vec<u8>> {
	let conn = WayshotConnection::new()?;
	let image = if cli.area {
		let selection = slurp_rs::select_region(SelectOptions::default())?;
		let rect = &selection.rect;
		conn.screenshot(
			LogicalRegion {
				inner: Region {
					position: Position {
						x: rect.x,
						y: rect.y,
					},
					size: libwayshot::Size {
						width: rect.width as u32,
						height: rect.height as u32,
					},
				},
			},
			true,
		)?
	} else if let Some(screen_num) = cli.screen {
		let outputs = conn.get_all_outputs();
		let output = outputs.get(screen_num).ok_or_else(|| {
			anyhow::anyhow!(
				"screen {}: only {} screens available",
				screen_num,
				outputs.len()
			)
		})?;
		conn.screenshot_single_output(output, true)?
	} else {
		conn.screenshot_all(true)?
	};

	let mut buf = Cursor::new(Vec::new());
	image.write_to(&mut buf, ImageFormat::Png)?;
	Ok(buf.into_inner())
}

#[tokio::main]
async fn main() -> Result<()> {
	let cli = Cli::parse();

	if let Some(Commands::Config { action }) = cli.command {
		return match action {
			Some(ConfigAction::Import { source }) => config::import_uploader(&source).await,
			Some(ConfigAction::List) => config::list_uploaders().await,
			Some(ConfigAction::Show { uploader }) => config::show_uploader(&uploader).await,
			Some(ConfigAction::Create) => config::create_uploader().await,
			Some(ConfigAction::Edit { uploader }) => {
				config::edit_uploader(uploader.as_deref()).await
			}
			Some(ConfigAction::Delete { uploader }) => {
				config::delete_uploader(uploader.as_deref()).await
			}
			Some(ConfigAction::Default { uploader }) => {
				config::set_default_uploader(uploader.as_deref()).await
			}
			None => config::run_config_wizard().await,
		};
	}

	if cli.screens {
		let conn = WayshotConnection::new()?;
		for (i, output) in conn.get_all_outputs().iter().enumerate() {
			let pos = output.logical_position();
			let size = output.logical_size();
			println!(
				"{}: {} ({}x{}+{}+{})",
				i, output.name, size.width, size.height, pos.x, pos.y
			);
		}
		return Ok(());
	}

	let png_bytes = capture(&cli)?;

	if cli.copy {
		match unsafe { libc::fork() } {
			-1 => bail!("fork failed"),
			0 => {
				let mut clipboard_opts = ClipboardOptions::new();
				clipboard_opts.foreground(true).seat(Seat::All);
				let _ = clipboard_opts.copy(
					Source::Bytes(png_bytes.into()),
					MimeType::Specific("image/png".into()),
				);
				std::process::exit(0);
			}
			_ => return Ok(()),
		}
	}

	let filename = chrono::Local::now()
		.format(
			&cli.filename
				.unwrap_or_else(|| "screenshot_%Y-%m-%d_%H-%M-%S.png".to_string()),
		)
		.to_string();

	let path = match &cli.output {
		Some(dir) => dir.join(&filename),
		None => PathBuf::from(&filename),
	};

	std::fs::write(&path, &png_bytes)?;

	Ok(())
}
