use std::fs::File;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;
use ureq::unversioned::multipart::{Form, Part};

use crate::config::{AppConfig, BodyType, UploadConfig, find_uploader_index, load_uploader_config};

pub enum UploadPayload<'a> {
	Bytes {
		bytes: &'a [u8],
		mime_type: &'static str,
	},
	File(&'a Path),
}

pub fn upload(
	payload: UploadPayload,
	uploader_name: Option<&str>,
	filename: &str,
) -> Result<String> {
	let cfg = load_uploader_config()?;

	if cfg.uploaders.is_empty() {
		bail!(
			"No uploaders configured. Use `framr config import` or `framr config create` to add one."
		);
	}

	let uploader = resolve_uploader(&cfg, uploader_name)?;
	let response_body = send_request(payload, filename, uploader)?;

	let url = parse_response_schema(&response_body, &uploader.output_url)?.ok_or_else(|| {
		anyhow::anyhow!(
			"Could not extract URL from upload response (schema: {})",
			uploader.output_url
		)
	})?;

	Ok(url)
}

fn resolve_uploader<'a>(cfg: &'a AppConfig, name: Option<&str>) -> Result<&'a UploadConfig> {
	let name = match name {
		Some(n) if !n.is_empty() => n,
		_ => cfg.default_uploader.as_deref().ok_or_else(|| {
			anyhow::anyhow!(
				"No default uploader configured. Use `framr config default` to set one, or specify an uploader with -u <name>."
			)
		})?,
	};

	let idx = find_uploader_index(cfg, name)
		.ok_or_else(|| anyhow::anyhow!("Uploader \"{}\" not found.", name))?;

	Ok(&cfg.uploaders[idx])
}

fn infer_mime_type(path: &Path) -> &'static str {
	if let Ok(Some(kind)) = infer::get_from_path(path) {
		return kind.mime_type();
	}
	"application/octet-stream"
}

fn send_request(payload: UploadPayload, filename: &str, uploader: &UploadConfig) -> Result<String> {
	use crate::config::types::RequestMethod;
	use ureq::RequestBuilder;

	fn apply_config<B>(mut req: RequestBuilder<B>, uploader: &UploadConfig) -> RequestBuilder<B> {
		for (key, value) in &uploader.parameters {
			req = req.query(key.as_str(), value.as_str());
		}
		for (key, value) in &uploader.headers {
			req = req.header(key.as_str(), unquote(value));
		}
		req.config().http_status_as_error(false).build()
	}

	let response = match uploader.request_method {
		RequestMethod::Get => apply_config(ureq::get(&uploader.request_url), uploader)
			.call()
			.map_err(|e| anyhow::anyhow!("{e}"))?,
		RequestMethod::Delete => apply_config(ureq::delete(&uploader.request_url), uploader)
			.call()
			.map_err(|e| anyhow::anyhow!("{e}"))?,
		method => {
			let req = match method {
				RequestMethod::Post => ureq::post(&uploader.request_url),
				RequestMethod::Put => ureq::put(&uploader.request_url),
				RequestMethod::Patch => ureq::patch(&uploader.request_url),
				_ => unreachable!(),
			};

			let builder = apply_config(req, uploader);
			match uploader.body_type {
				BodyType::Binary => match payload {
					UploadPayload::Bytes { bytes, .. } => {
						builder.send(bytes).map_err(|e| anyhow::anyhow!("{e}"))?
					}
					UploadPayload::File(path) => {
						let file = File::open(path)?;
						builder.send(file).map_err(|e| anyhow::anyhow!("{e}"))?
					}
				},
				BodyType::FormData => {
					let form = build_multipart_form(payload, filename, uploader)?;
					builder.send(form).map_err(|e| anyhow::anyhow!("{e}"))?
				}
				BodyType::URLEncoded => {
					let args: Vec<(&str, &str)> = uploader
						.arguments
						.iter()
						.map(|(k, v)| (k.as_str(), v.as_str()))
						.collect();
					builder
						.send_form(args)
						.map_err(|e| anyhow::anyhow!("{e}"))?
				}
				BodyType::Json => {
					let body = build_json_body(&uploader.arguments)?;
					let builder = builder.header("Content-Type", "application/json");
					builder.send(&body).map_err(|e| anyhow::anyhow!("{e}"))?
				}
				BodyType::Xml => {
					let body = build_xml_body(&uploader.arguments);
					let builder = builder.header("Content-Type", "application/xml");
					builder.send(&body).map_err(|e| anyhow::anyhow!("{e}"))?
				}
			}
		}
	};

	read_response_body(response, uploader)
}

fn read_response_body(
	response: ureq::http::Response<ureq::Body>,
	uploader: &UploadConfig,
) -> Result<String> {
	let status = response.status();
	let body = response
		.into_body()
		.read_to_string()
		.map_err(|e| anyhow::anyhow!("{e}"))?;

	if !status.is_success() {
		let error_detail = match &uploader.error_message {
			Some(schema) => match parse_response_schema(&body, schema) {
				Ok(Some(detail)) => detail,
				Ok(None) => body,
				Err(e) => bail!("{} (response body: {})", e, body),
			},
			None => body,
		};
		bail!("Upload failed with HTTP {}: {}", status, error_detail);
	}

	Ok(body)
}

fn build_multipart_form<'a>(
	payload: UploadPayload<'a>,
	filename: &str,
	uploader: &'a UploadConfig,
) -> Result<Form<'a>> {
	let mut form = Form::new();
	for (key, value) in &uploader.arguments {
		form = form.text(key.as_str(), value.as_str());
	}
	let form_name = uploader.file_form_name.as_deref().unwrap_or("file");
	let part = match payload {
		UploadPayload::Bytes { bytes, mime_type } => Part::bytes(bytes)
			.file_name(filename)
			.mime_str(mime_type)
			.map_err(|e| anyhow::anyhow!("{e}"))?,
		UploadPayload::File(path) => {
			let mime_type = infer_mime_type(path);
			Part::file(path)
				.map_err(|e| anyhow::anyhow!("Failed to create file part: {}", e))?
				.file_name(filename)
				.mime_str(mime_type)
				.map_err(|e| anyhow::anyhow!("{e}"))?
		}
	};
	Ok(form.part(form_name, part))
}

fn build_json_body(arguments: &[(String, String)]) -> Result<String> {
	let map: serde_json::Map<String, Value> = arguments
		.iter()
		.map(|(k, v)| (k.clone(), Value::String(v.clone())))
		.collect();
	serde_json::to_string(&map).map_err(Into::into)
}

fn build_xml_body(arguments: &[(String, String)]) -> String {
	let mut xml = String::from("<request>");
	for (key, value) in arguments {
		xml.push_str(&format!("<{}>{}</{}>", key, value, key));
	}
	xml.push_str("</request>");
	xml
}

fn parse_response_schema(body: &str, schema: &str) -> Result<Option<String>> {
	if let Some(path) = schema
		.strip_prefix("{json:")
		.and_then(|s| s.strip_suffix('}'))
	{
		let json: Value = serde_json::from_str(body)
			.map_err(|e| anyhow::anyhow!("Failed to parse response as JSON: {}", e))?;
		Ok(Some(navigate_json_path(&json, path)?))
	} else if schema == "{text}" {
		Ok(Some(body.trim().to_string()))
	} else {
		Ok(Some(schema.to_string()))
	}
}

fn navigate_json_path(json: &Value, path: &str) -> Result<String> {
	let mut current = json;

	let normalized_path = path.replace("[", ".").replace("]", "");

	for key in normalized_path.split('.').filter(|k| !k.is_empty()) {
		current = if let Ok(index) = key.parse::<usize>() {
			current.get(index)
		} else {
			current.get(key)
		}
		.ok_or_else(|| {
			anyhow::anyhow!(
				"JSON path segment '{}' (from path '{}') not found in response: {}",
				key,
				path,
				serde_json::to_string(json).unwrap_or_else(|_| json.to_string())
			)
		})?;
	}

	match current {
		Value::String(s) => Ok(s.clone()),
		other => Ok(other.to_string()),
	}
}

fn unquote(s: &str) -> &str {
	s.strip_prefix('"')
		.and_then(|s| s.strip_suffix('"'))
		.unwrap_or(s)
}
