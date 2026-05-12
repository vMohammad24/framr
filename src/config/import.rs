use crate::config::types::UploadConfig;
use anyhow::{Result, bail};
use base64::Engine as _;
use console::style;
use dialoguer::{Select, theme::ColorfulTheme};
use std::path::Path;

pub fn parse_sxcu(contents: &str) -> Result<UploadConfig> {
	let uploader: UploadConfig = serde_json::from_str(contents)?;

	let raw: serde_json::Value = serde_json::from_str(contents)?;
	if let Some(dest_type) = raw.get("DestinationType").and_then(|v| v.as_str()) {
		if !dest_type.contains("FileUploader") && !dest_type.contains("ImageUploader") {
			bail!(
				"Invalid uploader type: {}. Supported types: FileUploader, ImageUploader",
				dest_type
			);
		}
	} else if raw.get("Name").is_none() || raw.get("RequestURL").is_none() {
		bail!("Missing Name or RequestURL field in .sxcu file.");
	}

	Ok(uploader)
}

pub fn parse_iscu(contents: &str) -> Result<UploadConfig> {
	Ok(serde_json::from_str(contents)?)
}

pub fn detect_and_parse(contents: &str) -> Result<UploadConfig> {
	if let Ok(uploader) = parse_sxcu(contents) {
		return Ok(uploader);
	}
	if let Ok(uploader) = parse_iscu(contents) {
		return Ok(uploader);
	}
	bail!("Could not detect file format. Supported formats: ShareX (.sxcu), iShare (.iscu)");
}

pub fn parse_from_file(path: &str, interactive: bool) -> Result<UploadConfig> {
	let contents = std::fs::read_to_string(path)?;
	let ext = Path::new(path)
		.extension()
		.and_then(|e| e.to_str())
		.map(|e| e.to_lowercase());

	let theme = ColorfulTheme::default();

	match ext.as_deref() {
		Some("sxcu") => parse_sxcu(&contents),
		Some("iscu") => parse_iscu(&contents),
		_ if !interactive => detect_and_parse(&contents),
		_ => {
			let format = Select::with_theme(&theme)
				.with_prompt(format!(
					"Could not detect format ({}) — select file format:",
					style(ext.unwrap_or_else(|| "unknown".into())).yellow()
				))
				.items(["ShareX (.sxcu)", "iShare (.iscu)"])
				.interact()?;
			match format {
				0 => parse_sxcu(&contents),
				_ => parse_iscu(&contents),
			}
		}
	}
}

pub fn parse_from_url(url: &str) -> Result<UploadConfig> {
	println!("  {} {}", style("Downloading...").dim(), style(url).blue());
	let response = ureq::get(url).call().map_err(|e| anyhow::anyhow!("{e}"))?;
	let status = response.status();
	if !status.is_success() {
		bail!("HTTP error: {status}");
	}
	let contents = response
		.into_body()
		.read_to_string()
		.map_err(|e| anyhow::anyhow!("{e}"))?;

	let url_lower = url.to_lowercase();
	if url_lower.ends_with(".sxcu") {
		parse_sxcu(&contents)
	} else if url_lower.ends_with(".iscu") {
		parse_iscu(&contents)
	} else {
		detect_and_parse(&contents)
	}
}

pub fn parse_from_deeplink(deeplink: &str) -> Result<UploadConfig> {
	let data = deeplink
		.strip_prefix("framr://")
		.ok_or_else(|| anyhow::anyhow!("Invalid deeplink"))?;

	if data.starts_with("http://") || data.starts_with("https://") {
		return parse_from_url(data);
	}

	let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
		.decode(data)
		.or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data))
		.or_else(|_| base64::engine::general_purpose::STANDARD.decode(data))
		.or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(data))
		.map_err(|e| anyhow::anyhow!("Failed to decode deeplink: {}", e))?;

	let contents = String::from_utf8(decoded)
		.map_err(|e| anyhow::anyhow!("Invalid UTF-8 in deeplink: {}", e))?;

	detect_and_parse(&contents)
}

pub fn import_from_source(source: &str, interactive: bool) -> Result<UploadConfig> {
	if source.starts_with("http://") || source.starts_with("https://") {
		parse_from_url(source)
	} else if source.starts_with("framr://") {
		parse_from_deeplink(source)
	} else {
		parse_from_file(source, interactive)
	}
}
