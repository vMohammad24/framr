use anyhow::Result;
use clap::{Parser, Subcommand};
use libframr::{EncoderSpeed, H264Tune, VideoEncoder};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "framr")]
pub struct Cli {
	#[command(subcommand)]
	pub command: Option<Commands>,

	/// Screen to capture
	#[arg(short, long)]
	pub screen: Option<usize>,

	/// Version
	#[arg(short, long)]
	pub version: bool,

	/// List available screens
	#[arg(long)]
	pub screens: bool,

	/// Select an area to capture
	#[arg(short, long)]
	pub area: bool,

	/// Copy to clipboard
	#[arg(short, long)]
	pub copy: bool,

	/// Output directory (defaults to current directory)
	#[arg(short, long)]
	pub output: Option<PathBuf>,

	/// Filename (e.g. "screenshot_%Y-%m-%d.png")
	#[arg(long)]
	pub filename: Option<String>,

	/// Include cursor in capture
	#[arg(long)]
	pub cursor: bool,

	/// Record video
	#[arg(short, long)]
	pub record: bool,

	/// Video encoder (h264/x264, av1/rav1)
	#[arg(long)]
	pub encoder: Option<VideoEncoder>,

	/// Container format (mp4, mkv/matroska)
	#[arg(long)]
	pub container: Option<libframr::ContainerFormat>,

	/// Preferred hardware encoder element using gstreamer (e.g. nvh264enc, vaapiav1enc)
	#[arg(long)]
	pub hw_encoder: Option<String>,

	/// Recording bitrate in kbps
	#[arg(long)]
	pub bitrate: Option<u32>,

	/// Recording FPS
	#[arg(long)]
	pub fps: Option<u32>,

	/// Recording keyframe interval in frames
	#[arg(long)]
	pub keyframe_interval: Option<u32>,

	/// Recording threads (0 for auto)
	#[arg(long)]
	pub threads: Option<u32>,

	/// H.264 tune preset (zerolatency, film, animation, grain, stillimage, fastdecode)
	#[arg(long)]
	pub tune: Option<H264Tune>,

	/// Encoder speed preset (ultrafast, superfast, veryfast, faster, fast, medium, slow, slower, veryslow, placebo)
	#[arg(long)]
	pub speed: Option<EncoderSpeed>,

	/// Upload screenshot (uses default uploader, or specify with -u <name>)
	#[arg(short = 'u', long, num_args = 0..=1, default_missing_value = "")]
	pub upload: Option<String>,

	/// Deeplink URI (framr://...)
	#[arg(value_parser = parse_framr_uri)]
	pub uri: Option<String>,

	/// Silent mode (no notifications)
	#[arg(long, global = true)]
	pub silent: bool,
}

pub fn parse_framr_uri(arg: &str) -> Result<String, String> {
	if arg.starts_with("framr://") {
		Ok(arg.to_string())
	} else {
		Err("Positional arguments must be a valid framr:// URI".to_string())
	}
}

#[derive(Subcommand)]
pub enum Commands {
	/// Configure uploaders
	Config {
		#[command(subcommand)]
		action: Option<ConfigAction>,
	},
	/// Generate shell completions
	Completions {
		/// Shell to generate the completions for
		shell: clap_complete::Shell,
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
pub enum ConfigAction {
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
	/// Set the default upload sound path
	Sound {
		/// Path to the sound file (omitting prompts for selection)
		path: Option<String>,
	},
	/// Set the default screenshot image format (png, jpeg, webp)
	Format {
		/// Image format name (omitting prompts for selection)
		format: Option<String>,
	},
	/// Set the default screenshot image quality (1-100, only for jpeg)
	Quality {
		/// Image quality (1-100)
		quality: Option<u8>,
	},
	/// Register the framr:// protocol handler
	Protocol,
}
