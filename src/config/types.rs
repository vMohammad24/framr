use serde::{Deserialize, Serialize};

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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
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

#[derive(Default, Debug, Serialize, Deserialize)]
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
}
