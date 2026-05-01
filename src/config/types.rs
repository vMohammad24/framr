use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

pub trait ConfigEnum: Sized + Copy + PartialEq {
	fn variants() -> &'static [&'static str];
	fn from_index(i: usize) -> Option<Self>;
	fn to_index(self) -> usize;
	fn label(self) -> &'static str;
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
pub enum DefaultAction {
	#[default]
	Save,
	Copy,
	Upload,
	UploadAndCopy,
}

impl ConfigEnum for DefaultAction {
	fn variants() -> &'static [&'static str] {
		&[
			"Save to file",
			"Copy to clipboard",
			"Upload",
			"Upload and copy URL",
		]
	}

	fn from_index(i: usize) -> Option<Self> {
		match i {
			0 => Some(Self::Save),
			1 => Some(Self::Copy),
			2 => Some(Self::Upload),
			3 => Some(Self::UploadAndCopy),
			_ => None,
		}
	}

	fn to_index(self) -> usize {
		self as usize
	}

	fn label(self) -> &'static str {
		match self {
			Self::Save => "Save to file",
			Self::Copy => "Copy to clipboard",
			Self::Upload => "Upload",
			Self::UploadAndCopy => "Upload and copy URL",
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
pub enum DefaultCaptureMethod {
	#[default]
	Full,
	Area,
	Screen,
}

impl ConfigEnum for DefaultCaptureMethod {
	fn variants() -> &'static [&'static str] {
		&[
			"Full (all screens)",
			"Area (select region)",
			"Screen (specific screen)",
		]
	}

	fn from_index(i: usize) -> Option<Self> {
		match i {
			0 => Some(Self::Full),
			1 => Some(Self::Area),
			2 => Some(Self::Screen),
			_ => None,
		}
	}

	fn to_index(self) -> usize {
		self as usize
	}

	fn label(self) -> &'static str {
		match self {
			Self::Full => "Full (all screens)",
			Self::Area => "Area (select region)",
			Self::Screen => "Screen (specific screen)",
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
pub enum BodyType {
	#[default]
	Binary,
	FormData,
	URLEncoded,
	Json,
	Xml,
}

impl ConfigEnum for BodyType {
	fn variants() -> &'static [&'static str] {
		&[
			"Binary",
			"Form data (multipart)",
			"Form URL encoded",
			"JSON",
			"XML",
		]
	}

	fn from_index(i: usize) -> Option<Self> {
		match i {
			0 => Some(Self::Binary),
			1 => Some(Self::FormData),
			2 => Some(Self::URLEncoded),
			3 => Some(Self::Json),
			4 => Some(Self::Xml),
			_ => None,
		}
	}

	fn to_index(self) -> usize {
		self as usize
	}

	fn label(self) -> &'static str {
		match self {
			Self::Binary => "Binary",
			Self::FormData => "Form data (multipart)",
			Self::URLEncoded => "Form URL encoded",
			Self::Json => "JSON",
			Self::Xml => "XML",
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct UploadConfig {
	pub name: String,
	pub request_method: String,
	pub request_url: String,
	pub parameters: Vec<(String, String)>,
	pub headers: Vec<(String, String)>,
	pub body_type: BodyType,
	pub arguments: Vec<(String, String)>,
	pub file_form_name: Option<String>,
	pub output_url: String,
	pub error_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct AppConfig {
	#[serde(default)]
	pub uploaders: Vec<UploadConfig>,
	#[serde(default)]
	pub default_uploader: Option<String>,
	#[serde(default)]
	pub default_action: Option<DefaultAction>,
	#[serde(default)]
	pub default_capture: Option<DefaultCaptureMethod>,
	#[serde(default)]
	pub default_screen: Option<usize>,
	#[serde(default)]
	pub allowed_directories: Vec<String>,
	#[serde(default)]
	pub selection: SelectionConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(default)]
pub struct SelectionConfig {
	pub background_color: Color,
	pub border_color: Color,
	pub border_width: f64,
	pub toolbar_background_color: Color,
	pub toolbar_active_color: Color,
	pub toolbar_hover_color: Color,
	pub annotation_color: Color,
	pub annotation_line_width: f64,
	pub blur_radius: f32,
	pub pixelate_block_size: usize,
	pub toolbar_y: f64,
	pub toolbar_item_width: f64,
	pub toolbar_height: f64,
}

impl Default for SelectionConfig {
	fn default() -> Self {
		Self {
			background_color: Color::rgba(0.0, 0.0, 0.0, 0.4),
			border_color: Color::rgb(0.0, 0.5, 1.0),
			border_width: 2.0,
			toolbar_background_color: Color::rgba(0.15, 0.15, 0.15, 0.95),
			toolbar_active_color: Color::rgba(0.3, 0.6, 1.0, 0.8),
			toolbar_hover_color: Color::rgba(0.3, 0.3, 0.3, 0.8),
			annotation_color: Color::rgb(1.0, 0.0, 0.0),
			annotation_line_width: 4.0,
			blur_radius: 10.0,
			pixelate_block_size: 10,
			toolbar_y: 20.0,
			toolbar_item_width: 50.0,
			toolbar_height: 40.0,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
	pub r: u8,
	pub g: u8,
	pub b: u8,
	pub a: u8,
}

impl Color {
	pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
		Self { r, g, b, a }
	}

	pub fn rgb(r: f64, g: f64, b: f64) -> Self {
		Self {
			r: (r.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			g: (g.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			b: (b.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			a: u8::MAX,
		}
	}

	pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
		Self {
			r: (r.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			g: (g.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			b: (b.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			a: (a.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
		}
	}

	pub fn r_f64(&self) -> f64 {
		self.r as f64 / u8::MAX as f64
	}
	pub fn g_f64(&self) -> f64 {
		self.g as f64 / u8::MAX as f64
	}
	pub fn b_f64(&self) -> f64 {
		self.b as f64 / u8::MAX as f64
	}
	pub fn a_f64(&self) -> f64 {
		self.a as f64 / u8::MAX as f64
	}
}

impl fmt::Display for Color {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.a == u8::MAX {
			write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
		} else {
			write!(
				f,
				"#{:02X}{:02X}{:02X}{:02X}",
				self.r, self.g, self.b, self.a
			)
		}
	}
}

#[derive(Debug)]
pub enum ColorParseError {
	NonAscii,
	InvalidLength,
	Utf8Error(std::str::Utf8Error),
	ParseIntError(std::num::ParseIntError),
}

impl fmt::Display for ColorParseError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::NonAscii => write!(f, "hex color contains non-ASCII characters"),
			Self::InvalidLength => write!(f, "invalid hex color length"),
			Self::Utf8Error(e) => write!(f, "invalid utf8 in hex component: {}", e),
			Self::ParseIntError(e) => write!(f, "invalid hex component: {}", e),
		}
	}
}

impl std::error::Error for ColorParseError {}

impl FromStr for Color {
	type Err = ColorParseError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let hex = s.trim_start_matches('#');
		if !hex.is_ascii() {
			return Err(ColorParseError::NonAscii);
		}

		if hex.len() != 6 && hex.len() != 8 {
			return Err(ColorParseError::InvalidLength);
		}

		let components: Result<Vec<u8>, _> = hex
			.as_bytes()
			.chunks(2)
			.map(|chunk| {
				let s = std::str::from_utf8(chunk).map_err(ColorParseError::Utf8Error)?;
				u8::from_str_radix(s, 16).map_err(ColorParseError::ParseIntError)
			})
			.collect();

		let components = components?;

		match components.len() {
			3 => Ok(Self {
				r: components[0],
				g: components[1],
				b: components[2],
				a: u8::MAX,
			}),
			4 => Ok(Self {
				r: components[0],
				g: components[1],
				b: components[2],
				a: components[3],
			}),
			_ => Err(ColorParseError::InvalidLength),
		}
	}
}

impl Serialize for Color {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.collect_str(self)
	}
}

impl<'de> Deserialize<'de> for Color {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		struct ColorVisitor;

		impl<'de> Visitor<'de> for ColorVisitor {
			type Value = Color;

			fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
				formatter.write_str("a hex color string (e.g. #RRGGBB or #RRGGBBAA)")
			}

			fn visit_str<E>(self, value: &str) -> Result<Color, E>
			where
				E: de::Error,
			{
				Color::from_str(value).map_err(de::Error::custom)
			}
		}

		deserializer.deserialize_str(ColorVisitor)
	}
}
