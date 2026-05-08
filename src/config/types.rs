use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use strum::{AsRefStr, Display, EnumIter, IntoEnumIterator, IntoStaticStr};

pub use libframr::RecordingConfig;

pub trait ConfigEnum:
	Sized + Copy + PartialEq + AsRef<str> + std::fmt::Display + IntoEnumIterator + 'static
{
	fn from_index(i: usize) -> Option<Self> {
		Self::iter().nth(i)
	}

	fn to_index(self) -> usize {
		Self::iter().position(|e| e == self).unwrap_or(0)
	}

	fn label(self) -> &'static str;

	fn variants() -> Vec<&'static str> {
		Self::iter().map(|v| v.label()).collect()
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
	AsRefStr,
	Display,
	EnumIter,
	IntoStaticStr,
)]
#[strum(serialize_all = "title_case")]
pub enum DefaultAction {
	#[default]
	Save,
	Copy,
	Upload,
	UploadAndCopy,
}

impl ConfigEnum for DefaultAction {
	fn label(self) -> &'static str {
		self.into()
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
	AsRefStr,
	Display,
	EnumIter,
	IntoStaticStr,
)]
#[strum(serialize_all = "title_case")]
pub enum DefaultCaptureMethod {
	#[default]
	Full,
	Area,
	Screen,
}

impl ConfigEnum for DefaultCaptureMethod {
	fn label(self) -> &'static str {
		match self {
			Self::Full => "Full (all screens)",
			Self::Area => "Area (select region)",
			Self::Screen => "Screen (specific screen)",
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
	AsRefStr,
	Display,
	EnumIter,
	IntoStaticStr,
)]
pub enum BodyType {
	#[default]
	Binary,
	FormData,
	URLEncoded,
	Json,
	Xml,
}

impl ConfigEnum for BodyType {
	fn label(self) -> &'static str {
		match self {
			Self::FormData => "Form data (multipart)",
			Self::URLEncoded => "Form URL encoded",
			_ => self.into(),
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
	#[serde(default)]
	pub recording: RecordingConfig,
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
		let s = String::deserialize(deserializer)?;
		let rgba = csscolorparser::parse(&s).map_err(|e| D::Error::custom(e.to_string()))?;
		Ok(Self {
			r: (rgba.r * 255.0).round() as u8,
			g: (rgba.g * 255.0).round() as u8,
			b: (rgba.b * 255.0).round() as u8,
			a: (rgba.a * 255.0).round() as u8,
		})
	}
}

impl FromStr for Color {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let rgba = csscolorparser::parse(s).map_err(|e| e.to_string())?;
		Ok(Self {
			r: (rgba.r * 255.0).round() as u8,
			g: (rgba.g * 255.0).round() as u8,
			b: (rgba.b * 255.0).round() as u8,
			a: (rgba.a * 255.0).round() as u8,
		})
	}
}
