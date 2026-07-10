pub use connection::FramrConnection;
pub use error::FramrError;
pub use output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Position, Size, Transform};
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
	pub hw_encoder: Option<String>,
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
			hw_encoder: None,
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

	pub fn gst_muxer(&self) -> &'static str {
		match self {
			Self::Mp4 => "mp4mux",
			Self::Matroska => "matroskamux",
			Self::WebM => "webmmux",
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

pub fn find_hardware_encoder(
	encoder_type: VideoEncoder,
	preferred: Option<&str>,
) -> Option<String> {
	if let Some(p) = preferred {
		if gstreamer::ElementFactory::find(p).is_some() {
			return Some(p.to_string());
		}
	}

	let candidates = match encoder_type {
		VideoEncoder::H264 => vec![
			"amfh264enc",   // AMF
			"vah264enc",    // VA
			"nvh264enc",    // NVIDIA
			"msdkh264enc",  // Intel
			"vaapih264enc", // VA (Old)
		],
		VideoEncoder::AV1 => vec![
			"amfav1enc",   // AMF
			"vaav1enc",    // VAAPI
			"nvav1enc",    // NVIDIA
			"msdkav1enc",  // Intel
			"vaapiav1enc", // VA (Old)
		],
	};

	for name in candidates {
		if gstreamer::ElementFactory::find(name).is_some() {
			return Some(name.to_string());
		}
	}
	None
}
