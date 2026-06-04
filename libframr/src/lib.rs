pub mod backend;
mod buffer;
mod connection;
mod convert;
mod encoding;
mod error;
mod output;
mod transform;

use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(default)]
pub struct RecordingConfig {
	pub encoder: VideoEncoder,
	pub bitrate: u32,
	pub keyframe_interval: u32,
	pub threads: Option<u32>,
	pub tune: H264Tune,
	pub speed: EncoderSpeed,
}

impl Default for RecordingConfig {
	fn default() -> Self {
		Self {
			encoder: VideoEncoder::H264,
			bitrate: 4000,
			keyframe_interval: 60,
			threads: None,
			tune: H264Tune::Zerolatency,
			speed: EncoderSpeed::Ultrafast,
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy, strum::EnumIter, strum::AsRefStr, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum VideoEncoder {
	#[default]
	H264,
	AV1,
}

impl VideoEncoder {
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::H264 => "h264",
			Self::AV1 => "av1",
		}
	}
}

impl FromStr for VideoEncoder {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"h264" | "x264" => Ok(Self::H264),
			"av1" | "rav1" => Ok(Self::AV1),
			_ => Err(format!("Invalid video encoder: {}", s)),
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy, strum::EnumIter, strum::AsRefStr, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
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
		match self {
			Self::Film | Self::Animation | Self::Grain => true,
			_ => false,
		}
	}

	pub fn to_gst_value(&self) -> i32 {
		match self {
			Self::Zerolatency => 4,
			Self::Stillimage => 1,
			Self::Fastdecode => 2,
			Self::Film => 1,
			Self::Animation => 2,
			Self::Grain => 3,
		}
	}

	pub fn as_str(&self) -> &'static str {
		match self {
			Self::Zerolatency => "zerolatency",
			Self::Film => "film",
			Self::Animation => "animation",
			Self::Grain => "grain",
			Self::Stillimage => "stillimage",
			Self::Fastdecode => "fastdecode",
		}
	}
}

impl FromStr for H264Tune {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"zerolatency" => Ok(Self::Zerolatency),
			"film" => Ok(Self::Film),
			"animation" => Ok(Self::Animation),
			"grain" => Ok(Self::Grain),
			"stillimage" => Ok(Self::Stillimage),
			"fastdecode" => Ok(Self::Fastdecode),
			_ => Err(format!("Invalid H.264 tune: {}", s)),
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy, strum::EnumIter, strum::AsRefStr, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
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
	pub fn to_gst_value(&self) -> i32 {
		match self {
			Self::Ultrafast => 1,
			Self::Superfast => 2,
			Self::Veryfast => 3,
			Self::Faster => 4,
			Self::Fast => 5,
			Self::Medium => 6,
			Self::Slow => 7,
			Self::Slower => 8,
			Self::Veryslow => 9,
			Self::Placebo => 10,
		}
	}

	pub fn as_str(&self) -> &'static str {
		match self {
			Self::Ultrafast => "ultrafast",
			Self::Superfast => "superfast",
			Self::Veryfast => "veryfast",
			Self::Faster => "faster",
			Self::Fast => "fast",
			Self::Medium => "medium",
			Self::Slow => "slow",
			Self::Slower => "slower",
			Self::Veryslow => "veryslow",
			Self::Placebo => "placebo",
		}
	}
}

impl FromStr for EncoderSpeed {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"ultrafast" => Ok(Self::Ultrafast),
			"superfast" => Ok(Self::Superfast),
			"veryfast" => Ok(Self::Veryfast),
			"faster" => Ok(Self::Faster),
			"fast" => Ok(Self::Fast),
			"medium" => Ok(Self::Medium),
			"slow" => Ok(Self::Slow),
			"slower" => Ok(Self::Slower),
			"veryslow" => Ok(Self::Veryslow),
			"placebo" => Ok(Self::Placebo),
			_ => Err(format!("Invalid encoder speed preset: {}", s)),
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy, strum::EnumIter, strum::AsRefStr, strum::IntoStaticStr)]
pub enum OutputImageFormat {
	#[default]
	Png,
	Jpeg,
	WebP,
}

impl OutputImageFormat {
	pub fn all_formats() -> &'static [Self] {
		&[Self::Png, Self::Jpeg, Self::WebP]
	}
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

pub use connection::FramrConnection;
pub use error::FramrError;
pub use output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Position, Size, Transform};
