use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy)]
pub enum BodyType {
	#[default]
	Binary,
	FormData,
	URLEncoded,
	Json,
	Xml,
}

impl BodyType {
	pub(crate) fn variants() -> &'static [&'static str] {
		&[
			"Binary",
			"Form data (multipart)",
			"Form URL encoded",
			"JSON",
			"XML",
		]
	}

	pub(crate) fn from_index(i: usize) -> Option<Self> {
		match i {
			0 => Some(Self::Binary),
			1 => Some(Self::FormData),
			2 => Some(Self::URLEncoded),
			3 => Some(Self::Json),
			4 => Some(Self::Xml),
			_ => None,
		}
	}

	pub(crate) fn to_index(self) -> usize {
		self as usize
	}

	pub(crate) fn label(self) -> &'static str {
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
	pub uploaders: Vec<UploadConfig>,
	pub default_uploader: Option<String>,
}
