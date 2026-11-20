#[cfg(any(feature = "upload", feature = "annotations"))]
use serde::Deserializer;
#[cfg(feature = "annotations")]
use serde::Serializer;
#[cfg(any(feature = "upload", feature = "annotations"))]
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "annotations")]
use std::fmt;
#[cfg(any(feature = "upload", feature = "annotations"))]
use std::str::FromStr;
use strum::{AsRefStr, Display, EnumIter, IntoEnumIterator, IntoStaticStr};

pub use libframr::OutputImageFormat;
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

	fn variants_list(&self) -> Vec<&'static str> {
		Self::variants()
	}

	fn at_index(&self, i: usize) -> Option<Self> {
		Self::from_index(i)
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
	#[cfg_attr(not(feature = "upload"), serde(alias = "Upload"))]
	Save,
	Copy,
	#[cfg(feature = "upload")]
	Upload,
	#[cfg(feature = "upload")]
	UploadAndCopy,
	#[cfg_attr(not(feature = "upload"), serde(alias = "UploadAndCopy"))]
	SaveAndCopy,
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
	#[cfg_attr(not(feature = "annotations"), serde(alias = "Area"))]
	Full,
	#[cfg(feature = "annotations")]
	Area,
	Screen,
}

impl ConfigEnum for DefaultCaptureMethod {
	fn label(self) -> &'static str {
		match self {
			Self::Full => "Full (all screens)",
			#[cfg(feature = "annotations")]
			Self::Area => "Area (select region)",
			Self::Screen => "Screen (specific screen)",
		}
	}
}

#[cfg(feature = "upload")]
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

#[cfg(feature = "upload")]
impl ConfigEnum for BodyType {
	fn label(self) -> &'static str {
		match self {
			Self::FormData => "Form data (multipart)",
			Self::URLEncoded => "Form URL encoded",
			_ => self.into(),
		}
	}
}

#[cfg(feature = "upload")]
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
	strum::EnumString,
)]
#[strum(serialize_all = "UPPERCASE", ascii_case_insensitive)]
pub enum RequestMethod {
	#[default]
	Post,
	Get,
	Put,
	Patch,
	Delete,
}

#[cfg(feature = "upload")]
impl ConfigEnum for RequestMethod {
	fn label(self) -> &'static str {
		self.into()
	}
}

impl ConfigEnum for libframr::VideoEncoder {
	fn label(self) -> &'static str {
		match self {
			Self::H264 => "H.264",
			Self::AV1 => "AV1",
			Self::VP9 => "VP9",
		}
	}
}

impl ConfigEnum for libframr::ContainerFormat {
	fn label(self) -> &'static str {
		self.as_str()
	}
}

impl ConfigEnum for libframr::EncoderBackend {
	fn label(self) -> &'static str {
		self.into()
	}
}

impl ConfigEnum for libframr::H264Tune {
	fn label(self) -> &'static str {
		self.into()
	}
}

impl ConfigEnum for libframr::EncoderSpeed {
	fn label(self) -> &'static str {
		self.into()
	}
}

impl ConfigEnum for libframr::OutputImageFormat {
	fn label(self) -> &'static str {
		self.as_str()
	}
}

#[cfg(feature = "upload")]
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct UploadConfig {
	#[serde(alias = "Name")]
	pub name: String,
	#[serde(
		alias = "RequestMethod",
		alias = "requestMethod",
		default,
		deserialize_with = "deserialize_request_method"
	)]
	pub request_method: RequestMethod,
	#[serde(alias = "RequestURL", alias = "requestURL")]
	pub request_url: String,
	#[serde(alias = "Parameters", default, deserialize_with = "deserialize_kv")]
	pub parameters: Vec<(String, String)>,
	#[serde(
		alias = "Headers",
		alias = "headers",
		default,
		deserialize_with = "deserialize_kv"
	)]
	pub headers: Vec<(String, String)>,
	#[serde(
		alias = "Body",
		alias = "requestBodyType",
		default,
		deserialize_with = "deserialize_body_type"
	)]
	pub body_type: BodyType,
	#[serde(
		alias = "Arguments",
		alias = "formData",
		default,
		deserialize_with = "deserialize_kv"
	)]
	pub arguments: Vec<(String, String)>,
	#[serde(alias = "FileFormName", alias = "fileFormName")]
	pub file_form_name: Option<String>,
	#[serde(alias = "URL", alias = "responseURL")]
	pub output_url: String,
	#[serde(alias = "ErrorMessage")]
	pub error_message: Option<String>,
	#[serde(alias = "DeletionURL", alias = "deletionURL", default)]
	pub deletion_url: Option<String>,
	#[serde(
		alias = "DeleteRequestType",
		alias = "deleteRequestType",
		default = "default_deletion_method",
		deserialize_with = "deserialize_request_method"
	)]
	pub deletion_request_type: RequestMethod,
}

#[cfg(feature = "upload")]
fn deserialize_request_method<'de, D>(deserializer: D) -> Result<RequestMethod, D::Error>
where
	D: Deserializer<'de>,
{
	#[derive(Deserialize)]
	#[serde(untagged)]
	enum RawMethod {
		Enum(RequestMethod),
		String(String),
	}

	match RawMethod::deserialize(deserializer)? {
		RawMethod::Enum(m) => Ok(m),
		RawMethod::String(s) => RequestMethod::from_str(&s).map_err(DeError::custom),
	}
}

#[cfg(feature = "upload")]
fn default_deletion_method() -> RequestMethod {
	RequestMethod::Delete
}

#[cfg(feature = "upload")]
fn deserialize_kv<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
where
	D: Deserializer<'de>,
{
	#[derive(Deserialize)]
	#[serde(untagged)]
	enum RawKV {
		Map(serde_json::Map<String, serde_json::Value>),
		List(Vec<(String, String)>),
	}

	match RawKV::deserialize(deserializer)? {
		RawKV::Map(map) => Ok(map
			.into_iter()
			.map(|(k, v)| {
				let s = match v {
					serde_json::Value::String(s) => s,
					_ => v.to_string(),
				};
				(k, s)
			})
			.collect()),
		RawKV::List(list) => Ok(list),
	}
}

#[cfg(feature = "upload")]
fn deserialize_body_type<'de, D>(deserializer: D) -> Result<BodyType, D::Error>
where
	D: Deserializer<'de>,
{
	#[derive(Deserialize)]
	#[serde(untagged)]
	enum RawBodyType {
		Enum(BodyType),
		String(String),
	}

	match RawBodyType::deserialize(deserializer)? {
		RawBodyType::Enum(bt) => Ok(bt),
		RawBodyType::String(s) => match s.as_str() {
			"MultipartFormData" | "multipartFormData" => Ok(BodyType::FormData),
			"FormURLEncoded" => Ok(BodyType::URLEncoded),
			"JSON" => Ok(BodyType::Json),
			"XML" => Ok(BodyType::Xml),
			"Binary" | "binary" => Ok(BodyType::Binary),
			_ => Ok(BodyType::Binary),
		},
	}
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct AppConfig {
	#[cfg(feature = "upload")]
	#[serde(default)]
	pub uploaders: Vec<UploadConfig>,
	#[cfg(feature = "upload")]
	#[serde(default)]
	pub default_uploader: Option<String>,
	#[serde(default)]
	pub default_action: Option<DefaultAction>,
	#[serde(default)]
	pub default_capture: Option<DefaultCaptureMethod>,
	#[serde(default)]
	pub default_screen: Option<usize>,
	#[cfg(feature = "upload")]
	#[serde(default)]
	pub allowed_directories: Vec<String>,
	#[cfg(feature = "annotations")]
	#[serde(default)]
	pub selection: SelectionConfig,
	#[serde(default)]
	pub recording: RecordingConfig,
	#[serde(default)]
	pub image_format: Option<OutputImageFormat>,
	#[serde(default)]
	pub image_quality: Option<u8>,
	#[serde(default)]
	pub silent: Option<SilentConfig>,
	#[cfg(feature = "upload")]
	#[serde(default = "default_upload_sound")]
	pub upload_sound: String,
	#[serde(flatten)]
	pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ConfigOverrides {
	#[cfg(feature = "upload")]
	pub uploaders: Vec<UploadConfig>,
	#[cfg(feature = "upload")]
	pub default_uploader: Option<String>,
	pub default_action: Option<DefaultAction>,
	pub default_capture: Option<DefaultCaptureMethod>,
	pub default_screen: Option<usize>,
	#[cfg(feature = "upload")]
	pub allowed_directories: Vec<String>,
	#[cfg(feature = "annotations")]
	pub selection: Option<SelectionOverrides>,
	pub recording: Option<RecordingOverrides>,
	pub image_format: Option<OutputImageFormat>,
	pub image_quality: Option<u8>,
	pub silent: Option<SilentConfig>,
	#[cfg(feature = "upload")]
	pub upload_sound: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct RecordingOverrides {
	pub encoder: Option<libframr::VideoEncoder>,
	pub container: Option<libframr::ContainerFormat>,
	pub bitrate: Option<u32>,
	pub fps: Option<u32>,
	pub keyframe_interval: Option<u32>,
	pub threads: Option<u32>,
	pub tune: Option<libframr::H264Tune>,
	pub speed: Option<libframr::EncoderSpeed>,
	pub backend: Option<libframr::EncoderBackend>,
	pub vaapi_device: Option<std::path::PathBuf>,
}

#[cfg(feature = "annotations")]
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct SelectionOverrides {
	pub background_color: Option<Color>,
	pub border_color: Option<Color>,
	pub border_width: Option<f64>,
	pub toolbar_background_color: Option<Color>,
	pub toolbar_active_color: Option<Color>,
	pub toolbar_hover_color: Option<Color>,
	pub annotation_color: Option<Color>,
	pub annotation_line_width: Option<f64>,
	pub blur_radius: Option<f32>,
	pub pixelate_block_size: Option<usize>,
	pub highlight_color: Option<Color>,
	pub toolbar_y: Option<f64>,
	pub toolbar_item_width: Option<f64>,
	pub toolbar_height: Option<f64>,
	pub show_toolbar: Option<bool>,
	pub multi_region_mode: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default)]
pub struct SilentConfig {
	/// Silence notifications for screenshot save and copy actions.
	pub image: Option<bool>,
	/// Silence notifications for recording start, save, and copy actions.
	pub video: Option<bool>,
	/// Silence successful upload and uploader import notifications.
	#[cfg(feature = "upload")]
	pub upload: Option<bool>,
	/// Silence error notifications.
	pub error: Option<bool>,
	#[serde(flatten)]
	pub extra: BTreeMap<String, serde_json::Value>,
}

impl SilentConfig {
	pub fn is_empty(&self) -> bool {
		#[cfg(feature = "upload")]
		if self.upload.is_some() {
			return false;
		}
		self.image.is_none()
			&& self.video.is_none()
			&& self.error.is_none()
			&& self.extra.is_empty()
	}
}

#[derive(Clone, Copy)]
pub enum NotificationKind {
	Image,
	Video,
	#[cfg(feature = "upload")]
	Upload,
	Error,
}

impl AppConfig {
	pub fn notification_is_silent(&self, kind: NotificationKind) -> bool {
		let Some(silent) = self.silent.as_ref() else {
			return false;
		};

		match kind {
			NotificationKind::Image => silent.image.unwrap_or(false),
			NotificationKind::Video => silent.video.unwrap_or(false),
			#[cfg(feature = "upload")]
			NotificationKind::Upload => silent.upload.unwrap_or(false),
			NotificationKind::Error => silent.error.unwrap_or(false),
		}
	}
}

#[cfg(feature = "upload")]
fn default_upload_sound() -> String {
	dirs::config_local_dir()
		.unwrap_or_else(|| std::path::PathBuf::from("."))
		.join(env!("CARGO_PKG_NAME"))
		.join("sound.wav")
		.to_string_lossy()
		.to_string()
}

#[cfg(feature = "annotations")]
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
	pub highlight_color: Color,
	pub toolbar_y: f64,
	pub toolbar_item_width: f64,
	pub toolbar_height: f64,
	pub show_toolbar: bool,
	pub multi_region_mode: bool,
}

#[cfg(feature = "annotations")]
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
			highlight_color: Color::rgba(1.0, 1.0, 0.0, 0.4),
			toolbar_y: 20.0,
			toolbar_item_width: 50.0,
			toolbar_height: 40.0,
			show_toolbar: true,
			multi_region_mode: false,
		}
	}
}

#[cfg(feature = "annotations")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
	pub r: u8,
	pub g: u8,
	pub b: u8,
	pub a: u8,
}

#[cfg(feature = "annotations")]
impl Color {
	pub fn rgb(r: f64, g: f64, b: f64) -> Self {
		Self::rgba(r, g, b, 1.0)
	}

	pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
		Self {
			r: (r.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			g: (g.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			b: (b.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
			a: (a.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8,
		}
	}

	fn from_parsed(color: csscolorparser::Color) -> Self {
		let [r, g, b, a] = color.to_rgba8();
		Self { r, g, b, a }
	}

	pub fn components(&self) -> (f64, f64, f64, f64) {
		let [r, g, b, a] = [self.r, self.g, self.b, self.a].map(|v| v as f64 / 255.0);
		(r, g, b, a)
	}
}

#[cfg(feature = "annotations")]
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

#[cfg(feature = "annotations")]
impl Serialize for Color {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.collect_str(self)
	}
}

#[cfg(feature = "annotations")]
impl<'de> Deserialize<'de> for Color {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let s = String::deserialize(deserializer)?;
		csscolorparser::parse(&s)
			.map(Self::from_parsed)
			.map_err(|e| D::Error::custom(e.to_string()))
	}
}

#[cfg(feature = "annotations")]
impl FromStr for Color {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		csscolorparser::parse(s)
			.map(Self::from_parsed)
			.map_err(|e| e.to_string())
	}
}
