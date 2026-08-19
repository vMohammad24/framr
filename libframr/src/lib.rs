pub use connection::FramrConnection;
pub use error::FramrError;
pub use frame::{DmaBuf, DmaBufPlane, Frame, FrameBytes, FrameData};
pub use output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Position, Size, Transform};
pub mod backend;
mod buffer;
mod connection;
mod convert;
mod encoding;
mod error;
mod frame;
mod gpu;
mod output;
mod transform;

use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct RecordingConfig {
	pub encoder: VideoEncoder,
	pub container: ContainerFormat,
	pub bitrate: u32,
	pub fps: u32,
	pub keyframe_interval: u32,
	pub threads: Option<u32>,
	pub tune: H264Tune,
	pub speed: EncoderSpeed,
	pub backend: EncoderBackend,
	pub vaapi_device: Option<std::path::PathBuf>,
}

impl Default for RecordingConfig {
	fn default() -> Self {
		Self {
			encoder: VideoEncoder::H264,
			container: ContainerFormat::Mp4,
			bitrate: 4000,
			fps: 30,
			keyframe_interval: 60,
			threads: None,
			tune: H264Tune::Zerolatency,
			speed: EncoderSpeed::Ultrafast,
			backend: EncoderBackend::Auto,
			vaapi_device: None,
		}
	}
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::EnumIter,
	strum::AsRefStr,
	strum::Display,
	strum::IntoStaticStr,
	strum::EnumString,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum ContainerFormat {
	#[default]
	Mp4,
	#[strum(to_string = "matroska", serialize = "mkv")]
	Matroska,
	WebM,
}

impl ContainerFormat {
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::Mp4 => "mp4",
			Self::Matroska => "mkv",
			Self::WebM => "webm",
		}
	}
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::AsRefStr,
	strum::Display,
	strum::EnumString,
	strum::EnumIter,
	strum::IntoStaticStr,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum EncoderBackend {
	#[default]
	Auto,
	Software,
	Vaapi,
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::EnumIter,
	strum::AsRefStr,
	strum::Display,
	strum::IntoStaticStr,
	strum::EnumString,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum VideoEncoder {
	#[default]
	#[strum(to_string = "h264", serialize = "x264")]
	H264,
	#[strum(to_string = "av1", serialize = "rav1")]
	AV1,
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::EnumIter,
	strum::AsRefStr,
	strum::Display,
	strum::IntoStaticStr,
	strum::EnumString,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum H264Tune {
	#[default]
	Zerolatency,
	Film,
	Animation,
	Grain,
	Stillimage,
	Fastdecode,
}

impl H264Tune {
	pub fn is_psy_tune(&self) -> bool {
		matches!(self, Self::Film | Self::Animation | Self::Grain)
	}
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::EnumIter,
	strum::AsRefStr,
	strum::Display,
	strum::IntoStaticStr,
	strum::EnumString,
)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum EncoderSpeed {
	#[default]
	Ultrafast,
	Superfast,
	Veryfast,
	Faster,
	Fast,
	Medium,
	Slow,
	Slower,
	Veryslow,
	Placebo,
}

impl EncoderSpeed {
	pub fn software_preset(self) -> &'static str {
		self.into()
	}

	pub fn av1_preset(self) -> u8 {
		match self {
			Self::Ultrafast => 13,
			Self::Superfast => 12,
			Self::Veryfast => 10,
			Self::Faster => 9,
			Self::Fast => 8,
			Self::Medium => 6,
			Self::Slow => 4,
			Self::Slower => 3,
			Self::Veryslow => 2,
			Self::Placebo => 0,
		}
	}
}

#[derive(
	Debug,
	Serialize,
	Deserialize,
	Default,
	PartialEq,
	Eq,
	Clone,
	Copy,
	strum::EnumIter,
	strum::AsRefStr,
	strum::IntoStaticStr,
)]
pub enum OutputImageFormat {
	#[default]
	Png,
	Jpeg,
	WebP,
}

impl OutputImageFormat {
	pub fn to_image_format(self) -> image::ImageFormat {
		match self {
			Self::Png => image::ImageFormat::Png,
			Self::Jpeg => image::ImageFormat::Jpeg,
			Self::WebP => image::ImageFormat::WebP,
		}
	}

	pub fn extension(self) -> &'static str {
		match self {
			Self::Png => "png",
			Self::Jpeg => "jpg",
			Self::WebP => "webp",
		}
	}

	pub fn mime_type(self) -> &'static str {
		match self {
			Self::Png => "image/png",
			Self::Jpeg => "image/jpeg",
			Self::WebP => "image/webp",
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Png => "PNG",
			Self::Jpeg => "JPEG",
			Self::WebP => "WebP",
		}
	}
}

impl std::fmt::Display for OutputImageFormat {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

impl FromStr for OutputImageFormat {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"png" => Ok(Self::Png),
			"jpeg" | "jpg" => Ok(Self::Jpeg),
			"webp" => Ok(Self::WebP),
			_ => Err(format!(
				"Invalid image format: {}. Valid options: png, jpeg, webp",
				s
			)),
		}
	}
}
