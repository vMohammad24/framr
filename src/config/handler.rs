use anyhow::{Result, bail};
use console::style;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use serde::Deserialize;
use std::path::Path;

use super::types::{AppConfig, BodyType, ConfigEnum, UploadConfig};

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct SxcuFile {
	Name: Option<String>,
	DestinationType: Option<String>,
	RequestMethod: Option<String>,
	RequestURL: Option<String>,
	Parameters: Option<serde_json::Map<String, serde_json::Value>>,
	Headers: Option<serde_json::Map<String, serde_json::Value>>,
	Body: Option<String>,
	Arguments: Option<serde_json::Map<String, serde_json::Value>>,
	FileFormName: Option<String>,
	URL: Option<String>,
	ErrorMessage: Option<String>,
}

impl From<SxcuFile> for UploadConfig {
	fn from(sxcu: SxcuFile) -> Self {
		Self {
			name: sxcu.Name.unwrap_or_default(),
			request_method: sxcu.RequestMethod.unwrap_or_else(|| "POST".into()),
			request_url: sxcu.RequestURL.unwrap_or_default(),
			parameters: sxcu
				.Parameters
				.map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect())
				.unwrap_or_default(),
			headers: sxcu
				.Headers
				.map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect())
				.unwrap_or_default(),
			body_type: sxcu
				.Body
				.and_then(|b| match b.as_str() {
					"MultipartFormData" => Some(BodyType::FormData),
					"FormURLEncoded" => Some(BodyType::URLEncoded),
					"JSON" => Some(BodyType::Json),
					"XML" => Some(BodyType::Xml),
					"Binary" => Some(BodyType::Binary),
					_ => None,
				})
				.unwrap_or_default(),
			arguments: sxcu
				.Arguments
				.map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect())
				.unwrap_or_default(),
			file_form_name: sxcu.FileFormName,
			output_url: sxcu.URL.unwrap_or_default(),
			error_message: sxcu.ErrorMessage,
		}
	}
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct IscuFile {
	name: String,
	requestURL: String,
	headers: Option<serde_json::Map<String, serde_json::Value>>,
	formData: Option<serde_json::Map<String, serde_json::Value>>,
	fileFormName: Option<String>,
	requestBodyType: Option<String>,
	responseURL: String,
}

impl From<IscuFile> for UploadConfig {
	fn from(iscu: IscuFile) -> Self {
		Self {
			name: iscu.name,
			request_method: "POST".into(),
			request_url: iscu.requestURL,
			parameters: Vec::new(),
			error_message: None,
			headers: iscu
				.headers
				.map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect())
				.unwrap_or_default(),
			body_type: iscu
				.requestBodyType
				.and_then(|b| match b.as_str() {
					"multipartFormData" => Some(BodyType::FormData),
					"binary" => Some(BodyType::Binary),
					_ => None,
				})
				.unwrap_or_default(),
			arguments: iscu
				.formData
				.map(|m| m.into_iter().map(|(k, v)| (k, v.to_string())).collect())
				.unwrap_or_default(),
			file_form_name: iscu.fileFormName,
			output_url: iscu.responseURL,
		}
	}
}

pub fn load_config() -> Result<AppConfig> {
	let app_name = env!("CARGO_PKG_NAME");
	let cfg: AppConfig = confy::load(app_name, None)?;
	if cfg!(debug_assertions) {
		dbg!(&cfg);
	}
	Ok(cfg)
}

pub(crate) fn save_config(cfg: &AppConfig) -> Result<()> {
	let app_name = env!("CARGO_PKG_NAME");
	confy::store(app_name, None, cfg)?;
	Ok(())
}

fn header(text: &str) -> String {
	style(format!("\n=== {} ===", text))
		.cyan()
		.bold()
		.to_string()
}

fn print_success(text: &str) {
	println!("{} {}", style("✔").green().bold(), style(text).green());
}

fn print_error(text: &str) {
	println!("{} {}", style("✖").red().bold(), style(text).red());
}

fn uploader_list_entry(index: usize, uploader: &UploadConfig, is_default: bool) -> String {
	let default_marker = if is_default {
		format!("{} ", style("*").yellow().bold())
	} else {
		String::new()
	};
	format!(
		"  {} {}{} {} {}",
		style(format!("{:>2}.", index + 1)).dim(),
		default_marker,
		style(&uploader.name).green().bold(),
		style(format!("[{}]", &uploader.request_method)).magenta(),
		style(&uploader.request_url).blue()
	)
}

fn display_uploader_details(uploader: &UploadConfig) {
	println!(
		"  {:<18} {}",
		style("Name:").bold(),
		style(&uploader.name).green().bold()
	);
	println!(
		"  {:<18} {}",
		style("Request Method:").bold(),
		style(&uploader.request_method).magenta().bold()
	);
	println!(
		"  {:<18} {}",
		style("Request URL:").bold(),
		style(&uploader.request_url).blue()
	);
	println!(
		"  {:<18} {}",
		style("Body Type:").bold(),
		style(uploader.body_type.label()).magenta()
	);

	if let Some(ref form_name) = uploader.file_form_name {
		println!(
			"  {:<18} {}",
			style("File Form Name:").bold(),
			style(form_name).cyan()
		);
	}

	display_kv_pairs("Headers", &uploader.headers);
	display_kv_pairs("URL Parameters", &uploader.parameters);
	display_kv_pairs("Body Arguments", &uploader.arguments);

	println!(
		"\n  {:<18} {}",
		style("Output URL:").bold(),
		style(&uploader.output_url).green()
	);

	if let Some(ref err_msg) = uploader.error_message {
		println!(
			"  {:<18} {}",
			style("Error Message:").bold(),
			style(err_msg).red()
		);
	}
	println!();
}

fn display_kv_pairs(label: &str, pairs: &[(String, String)]) {
	println!("  {}:", style(label).bold());
	if pairs.is_empty() {
		println!("    {}", style("(none)").dim());
	} else {
		for (k, v) in pairs {
			println!("    {} = {}", style(k).yellow(), style(v).cyan());
		}
	}
}

pub(crate) fn find_uploader_index(cfg: &AppConfig, name_or_index: &str) -> Option<usize> {
	if let Ok(idx) = name_or_index.parse::<usize>()
		&& idx > 0
		&& idx <= cfg.uploaders.len()
	{
		return Some(idx - 1);
	}
	cfg.uploaders
		.iter()
		.position(|u| u.name.eq_ignore_ascii_case(name_or_index))
}

pub(crate) fn ensure_unique_uploader_name(cfg: &AppConfig, name: String) -> String {
	let mut counter = 1;
	let mut new_name = name.clone();

	while cfg
		.uploaders
		.iter()
		.any(|u| u.name.eq_ignore_ascii_case(&new_name))
	{
		new_name = format!("{} ({})", name, counter);
		counter += 1;
	}

	new_name
}

pub(crate) fn select_uploader_index(cfg: &AppConfig, prompt: &str) -> Result<usize> {
	let theme = ColorfulTheme::default();
	let items: Vec<String> = cfg
		.uploaders
		.iter()
		.map(|u| {
			let is_default = cfg.default_uploader.as_deref() == Some(&u.name);
			format!(
				"{} {} {} {}",
				u.name,
				if is_default {
					format!("{} ", style("(default)").yellow().bold())
				} else {
					String::new()
				},
				style(format!("[{}]", u.request_method)).magenta(),
				style(&u.request_url).blue()
			)
		})
		.collect();
	Ok(Select::with_theme(&theme)
		.with_prompt(prompt)
		.items(&items)
		.interact()?)
}

pub(crate) fn resolve_uploader_index(
	cfg: &AppConfig,
	name_or_index: Option<&str>,
	prompt: &str,
) -> Result<usize> {
	match name_or_index {
		Some(name) => find_uploader_index(cfg, name)
			.ok_or_else(|| anyhow::anyhow!("Uploader \"{}\" not found.", name)),
		None => select_uploader_index(cfg, prompt),
	}
}

fn optional_input(prompt: &str, current: Option<&str>) -> Result<Option<String>> {
	let theme = ColorfulTheme::default();

	let p = if let Some(c) = current {
		format!("{} [{}] (leave empty to keep)", prompt, style(c).dim())
	} else {
		format!("{} (leave empty to skip)", prompt)
	};

	let val: String = Input::with_theme(&theme)
		.with_prompt(&p)
		.allow_empty(true)
		.interact_text()?;

	if val.trim().is_empty() {
		Ok(current.map(String::from))
	} else {
		Ok(Some(val))
	}
}

fn manage_kv_pairs(label: &str, pairs: &mut Vec<(String, String)>) -> Result<()> {
	let theme = ColorfulTheme::default();
	loop {
		println!("\n  {}", style(format!("Current {}:", label)).cyan().bold());
		if pairs.is_empty() {
			println!("    {}", style("(none)").dim());
		} else {
			for (i, (k, v)) in pairs.iter().enumerate() {
				println!(
					"    {}. {} = {}",
					style(i + 1).yellow(),
					style(k).green(),
					style(v).magenta()
				);
			}
		}

		let actions = ["Add new", "Remove existing", "Clear all", "Done / Skip"];
		let sel = Select::with_theme(&theme)
			.with_prompt(format!("Manage {}", label))
			.items(actions)
			.default(3)
			.interact()?;

		match sel {
			0 => {
				let key: String = Input::with_theme(&theme)
					.with_prompt(format!("{} Name", label))
					.interact_text()?;
				let val: String = Input::with_theme(&theme)
					.with_prompt(format!("{} Value", label))
					.interact_text()?;
				pairs.push((key, val));
			}
			1 => {
				if pairs.is_empty() {
					continue;
				}
				let items: Vec<String> = pairs
					.iter()
					.map(|(k, v)| format!("{} = {}", k, v))
					.collect();
				let r_sel = Select::with_theme(&theme)
					.with_prompt("Select item to remove")
					.items(&items)
					.interact()?;
				pairs.remove(r_sel);
			}
			2 => pairs.clear(),
			_ => break,
		}
	}
	Ok(())
}

fn select_request_method(theme: &ColorfulTheme, default_idx: usize) -> Result<String> {
	let methods = ["POST", "GET", "PUT", "PATCH", "DELETE"];
	let sel = Select::with_theme(theme)
		.with_prompt("Request method")
		.items(methods)
		.default(default_idx)
		.interact()?;
	Ok(methods[sel].to_string())
}

pub(crate) fn create_uploader_interactive(cfg: &mut AppConfig) -> Result<()> {
	let theme = ColorfulTheme::default();
	print!("{}", header("Create new uploader"));

	let name: String = loop {
		let input: String = Input::with_theme(&theme)
			.with_prompt("Name")
			.interact_text()?;

		if cfg
			.uploaders
			.iter()
			.any(|u| u.name.eq_ignore_ascii_case(&input))
		{
			print_error(&format!(
				"An uploader named \"{}\" already exists. Please choose a different name.",
				input
			));
		} else {
			break input;
		}
	};

	let request_url: String = Input::with_theme(&theme)
		.with_prompt("Request URL")
		.interact_text()?;

	let request_method = select_request_method(&theme, 0)?;

	let body_selection = Select::with_theme(&theme)
		.with_prompt("Body type")
		.items(BodyType::variants())
		.default(1)
		.interact()?;
	let body_type = BodyType::from_index(body_selection).unwrap_or_default();

	let file_form_name = if body_type == BodyType::FormData {
		optional_input("File form name", Some("file"))?
	} else {
		None
	};

	let mut headers = Vec::new();
	let mut parameters = Vec::new();
	let mut arguments = Vec::new();

	manage_kv_pairs("Headers", &mut headers)?;
	manage_kv_pairs("URL Parameters", &mut parameters)?;
	manage_kv_pairs("Body Arguments", &mut arguments)?;

	let output_url: String = Input::with_theme(&theme)
		.with_prompt("Output URL parse schema (e.g. {json:data.url})")
		.default("{json:url}".into())
		.interact_text()?;

	let error_message = optional_input("Error message schema (e.g. {json:error})", None)?;

	let uploader = UploadConfig {
		name,
		request_method,
		request_url,
		parameters,
		headers,
		body_type,
		arguments,
		file_form_name,
		output_url,
		error_message,
	};

	println!(
		"\n  {} {} ({})",
		style("Created:").green().bold(),
		style(&uploader.name).green().bold(),
		style(&uploader.request_url).blue()
	);

	cfg.uploaders.push(uploader);
	Ok(())
}

pub(crate) fn modify_uploader_at(cfg: &mut AppConfig, idx: usize) -> Result<()> {
	let theme = ColorfulTheme::default();
	let uploader = &mut cfg.uploaders[idx];

	print!("{}", header(&format!("Modifying {}", &uploader.name)));

	uploader.name = Input::with_theme(&theme)
		.with_prompt("Name")
		.default(uploader.name.clone())
		.interact_text()?;

	uploader.request_url = Input::with_theme(&theme)
		.with_prompt("Request URL")
		.default(uploader.request_url.clone())
		.interact_text()?;

	let methods = ["POST", "GET", "PUT", "PATCH", "DELETE"];
	let current_method_idx = methods
		.iter()
		.position(|&m| m == uploader.request_method)
		.unwrap_or(0);
	uploader.request_method = select_request_method(&theme, current_method_idx)?;

	let body_selection = Select::with_theme(&theme)
		.with_prompt("Body type")
		.items(BodyType::variants())
		.default(uploader.body_type.to_index())
		.interact()?;
	uploader.body_type = BodyType::from_index(body_selection).unwrap_or_default();

	if uploader.body_type == BodyType::FormData {
		uploader.file_form_name =
			optional_input("File form name", uploader.file_form_name.as_deref())?;
	} else {
		uploader.file_form_name = None;
	}

	manage_kv_pairs("Headers", &mut uploader.headers)?;
	manage_kv_pairs("URL Parameters", &mut uploader.parameters)?;
	manage_kv_pairs("Body Arguments", &mut uploader.arguments)?;

	uploader.output_url = Input::with_theme(&theme)
		.with_prompt("Output URL parse schema")
		.default(uploader.output_url.clone())
		.interact_text()?;

	uploader.error_message =
		optional_input("Error message schema", uploader.error_message.as_deref())?;

	Ok(())
}

fn parse_sxcu(contents: &str) -> Result<UploadConfig> {
	let sxcu: SxcuFile = serde_json::from_str(contents)?;

	if let Some(ref uploader_type) = sxcu.DestinationType {
		if !uploader_type.contains("FileUploader") && !uploader_type.contains("ImageUploader") {
			bail!(
				"Invalid uploader type: {}. Supported types: FileUploader, ImageUploader",
				uploader_type
			);
		}
	} else {
		bail!("Missing Type field in .sxcu file. Supported types: FileUploader, ImageUploader");
	}

	Ok(sxcu.into())
}

fn parse_iscu(contents: &str) -> Result<UploadConfig> {
	let iscu: IscuFile = serde_json::from_str(contents)?;
	Ok(iscu.into())
}

fn detect_and_parse(contents: &str) -> Result<UploadConfig> {
	if let Ok(uploader) = parse_sxcu(contents) {
		return Ok(uploader);
	}
	if let Ok(uploader) = parse_iscu(contents) {
		return Ok(uploader);
	}
	bail!("Could not detect file format. Supported formats: ShareX (.sxcu), iShare (.iscu)");
}

fn parse_from_file(path: &str, interactive: bool) -> Result<UploadConfig> {
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

fn parse_from_url(url: &str) -> Result<UploadConfig> {
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

pub(crate) fn import_from_source(source: &str, interactive: bool) -> Result<UploadConfig> {
	if source.starts_with("http://") || source.starts_with("https://") {
		parse_from_url(source)
	} else {
		parse_from_file(source, interactive)
	}
}

pub(crate) fn display_header(text: &str) -> String {
	header(text)
}

pub(crate) fn display_success(text: &str) {
	print_success(text);
}

pub(crate) fn display_error(text: &str) {
	print_error(text);
}

pub(crate) fn display_uploader_list_entry(
	index: usize,
	uploader: &UploadConfig,
	is_default: bool,
) -> String {
	uploader_list_entry(index, uploader, is_default)
}

pub(crate) fn display_uploader_full_details(uploader: &UploadConfig) {
	display_uploader_details(uploader);
}
