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
pub struct RecordingConfig {
	pub bitrate: u32,
	pub keyframe_interval: u32,
	pub threads: Option<u32>,
	pub tune: H264Tune,
	pub speed_preset: H264SpeedPreset,
}

impl Default for RecordingConfig {
	fn default() -> Self {
		Self {
			bitrate: 4000,
			keyframe_interval: 60,
			threads: None,
			tune: H264Tune::Zerolatency,
			speed_preset: H264SpeedPreset::Ultrafast,
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
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

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
pub enum H264SpeedPreset {
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

impl H264SpeedPreset {
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

impl FromStr for H264SpeedPreset {
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
			_ => Err(format!("Invalid H.264 speed preset: {}", s)),
		}
	}
}

pub use connection::FramrConnection;
pub use error::FramrError;
pub use output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Position, Size, Transform};
