use super::types::{AppConfig, BodyType, ConfigEnum, UploadConfig};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use console::style;
use dialoguer::{Select, theme::ColorfulTheme};
use std::{
	fs,
	path::{Path, PathBuf},
};


pub fn load_config() -> Result<AppConfig> {
	let app_name = env!("CARGO_PKG_NAME");
	let mut cfg: AppConfig = confy::load(app_name, None)?;

	if let Some(over) = load_overrides() {
		merge_configs(&mut cfg, over);
	}

	Ok(cfg)
}

fn load_overrides() -> Option<AppConfig> {
	let override_path = std::env::var("FRAMR_OVERRIDES").ok()?;
	let path = PathBuf::from(override_path);
	if !path.exists() {
		return None;
	}

	let content = fs::read_to_string(&path).ok()?;
	serde_json::from_str(&content)
		.ok()
		.or_else(|| confy::load_path(&path).ok())
}

fn merge_configs(base: &mut AppConfig, over: AppConfig) {
	if let Some(uploader) = over.default_uploader {
		base.default_uploader = Some(uploader);
	}
	if let Some(action) = over.default_action {
		base.default_action = Some(action);
	}
	if let Some(capture) = over.default_capture {
		base.default_capture = Some(capture);
	}
	if let Some(screen) = over.default_screen {
		base.default_screen = Some(screen);
	}

	for dir in over.allowed_directories {
		if !base.allowed_directories.contains(&dir) {
			base.allowed_directories.push(dir);
		}
	}

	for over_u in over.uploaders {
		if let Some(existing) = base.uploaders.iter_mut().find(|u| u.name == over_u.name) {
			*existing = over_u;
		} else {
			base.uploaders.push(over_u);
		}
	}
}

fn get_system_secret_dirs() -> Vec<PathBuf> {
	let mut allowed = Vec::new();

	if let Some(mut dir) = dirs::config_dir() {
		dir.push(env!("CARGO_PKG_NAME"));
		dir.push("secrets");
		allowed.push(dir);
	}
	if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
		allowed.push(PathBuf::from(xdg_runtime));
	}
	allowed.push(PathBuf::from("/run/secrets"));
	allowed.push(PathBuf::from("/var/run/secrets"));

	allowed
		.into_iter()
		.filter_map(|p| p.canonicalize().ok())
		.collect()
}

fn resolve_string(s: &str, allowed_bases: &[PathBuf]) -> Result<String> {
	let expanded = shellexpand::full(s)
		.with_context(|| format!("Failed to expand shell variables in '{}'", s))?
		.into_owned();

	if let Some(path_str) = expanded.strip_prefix("file:") {
		let requested_path = Path::new(path_str);

		let resolved_path = requested_path
			.canonicalize()
			.with_context(|| format!("File not found or invalid path: {}", path_str))?;

		let is_safe = allowed_bases
			.iter()
			.any(|base| resolved_path.starts_with(base));

		if !is_safe {
			bail!(
				"Security Alert: Path '{}' attempts to read outside allowed secret boundaries. if you think this is a mistake add it to the allowed_directories in the config file, move it to one of the system secret directories or open a github issue.",
				path_str
			);
		}

		return fs::read_to_string(&resolved_path)
			.map(|content| content.trim().to_string())
			.with_context(|| format!("Failed to read safe file: {}", resolved_path.display()));
	}

	Ok(expanded)
}

pub fn load_uploader_config() -> Result<AppConfig> {
	let mut cfg = load_config()?;
	let mut allowed_bases = get_system_secret_dirs();

	for dir in &cfg.allowed_directories {
		if let Ok(expanded) = shellexpand::full(dir)
			&& let Ok(canon) = Path::new(expanded.as_ref()).canonicalize()
		{
			allowed_bases.push(canon);
		}
	}

	for u in &mut cfg.uploaders {
		u.request_url = resolve_string(&u.request_url, &allowed_bases)?;
		u.output_url = resolve_string(&u.output_url, &allowed_bases)?;

		if let Some(form_name) = &mut u.file_form_name {
			*form_name = resolve_string(form_name, &allowed_bases)?;
		}
		if let Some(error_msg) = &mut u.error_message {
			*error_msg = resolve_string(error_msg, &allowed_bases)?;
		}

		for vec in [&mut u.parameters, &mut u.headers, &mut u.arguments] {
			for (_, val) in vec {
				*val = resolve_string(val, &allowed_bases)?;
			}
		}
	}

	Ok(cfg)
}

pub(crate) fn save_config(cfg: &AppConfig) -> Result<()> {
	let app_name = env!("CARGO_PKG_NAME");
	let mut to_save = cfg.clone();

	if let Some(over) = load_overrides() {
		if to_save.default_uploader == over.default_uploader {
			to_save.default_uploader = None;
		}
		if to_save.default_action == over.default_action {
			to_save.default_action = None;
		}
		if to_save.default_capture == over.default_capture {
			to_save.default_capture = None;
		}
		if to_save.default_screen == over.default_screen {
			to_save.default_screen = None;
		}

		to_save
			.allowed_directories
			.retain(|d| !over.allowed_directories.contains(d));

		to_save
			.uploaders
			.retain(|u| !over.uploaders.iter().any(|over_u| over_u.name == u.name));
	}

	confy::store(app_name, None, to_save)?;
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

	super::prompt_select(prompt, &items, 0)
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

fn manage_kv_pairs(label: &str, pairs: &mut Vec<(String, String)>) -> Result<()> {
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
		let sel = super::prompt_select(&format!("Manage {}", label), &actions, 3)?;

		match sel {
			0 => {
				let key: String = super::prompt_input(&format!("{} Name", label), None)?;
				let val: String = super::prompt_input(&format!("{} Value", label), None)?;
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
				let r_sel = super::prompt_select("Select item to remove", &items, 0)?;
				pairs.remove(r_sel);
			}
			2 => pairs.clear(),
			_ => break,
		}
	}
	Ok(())
}

fn select_request_method(default_idx: usize) -> Result<String> {
	let methods = ["POST", "GET", "PUT", "PATCH", "DELETE"];
	let sel = super::prompt_select("Request method", &methods, default_idx)?;
	Ok(methods[sel].to_string())
}

pub(crate) fn create_uploader_interactive(cfg: &mut AppConfig) -> Result<()> {
	print!("{}", header("Create new uploader"));

	let name: String = loop {
		let input: String = super::prompt_input("Name", None)?;

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

	let request_url: String = super::prompt_input("Request URL", None)?;
	let request_method = select_request_method(0)?;

	let variants = BodyType::variants();
	let body_selection = super::prompt_select("Body type", &variants, 1)?;
	let body_type = BodyType::from_index(body_selection).unwrap_or_default();

	let file_form_name = if body_type == BodyType::FormData {
		super::prompt_optional_input("File form name", Some("file"))?
	} else {
		None
	};

	let mut headers = Vec::new();
	let mut parameters = Vec::new();
	let mut arguments = Vec::new();

	manage_kv_pairs("Headers", &mut headers)?;
	manage_kv_pairs("URL Parameters", &mut parameters)?;
	manage_kv_pairs("Body Arguments", &mut arguments)?;

	let output_url: String = super::prompt_input("Output URL parse schema", Some("{json:url}".into()))?;
	let error_message = super::prompt_optional_input("Error message schema", None)?;

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
	let uploader = &mut cfg.uploaders[idx];

	if let Some(over) = load_overrides()
		&& over.uploaders.iter().any(|u| u.name == uploader.name)
	{
		println!(
			"  {} This uploader is managed by {} and is read-only. Any changes made here will be lost when the app restarts.",
			style("Note:").yellow().bold(),
			style("Nix/FRAMR_OVERRIDES").blue().bold()
		);
	}

	print!("{}", header(&format!("Modifying {}", &uploader.name)));

	uploader.name = super::prompt_input("Name", Some(uploader.name.clone()))?;
	uploader.request_url = super::prompt_input("Request URL", Some(uploader.request_url.clone()))?;

	let methods = ["POST", "GET", "PUT", "PATCH", "DELETE"];
	let current_method_idx = methods
		.iter()
		.position(|&m| m == uploader.request_method)
		.unwrap_or(0);
	uploader.request_method = select_request_method(current_method_idx)?;

	let variants = BodyType::variants();
	let body_selection = super::prompt_select("Body type", &variants, uploader.body_type.to_index())?;
	uploader.body_type = BodyType::from_index(body_selection).unwrap_or_default();

	if uploader.body_type == BodyType::FormData {
		uploader.file_form_name =
			super::prompt_optional_input("File form name", uploader.file_form_name.as_deref())?;
	} else {
		uploader.file_form_name = None;
	}

	manage_kv_pairs("Headers", &mut uploader.headers)?;
	manage_kv_pairs("URL Parameters", &mut uploader.parameters)?;
	manage_kv_pairs("Body Arguments", &mut uploader.arguments)?;

	uploader.output_url = super::prompt_input("Output URL parse schema", Some(uploader.output_url.clone()))?;
	uploader.error_message =
		super::prompt_optional_input("Error message schema", uploader.error_message.as_deref())?;

	Ok(())
}

fn parse_sxcu(contents: &str) -> Result<UploadConfig> {
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

fn parse_iscu(contents: &str) -> Result<UploadConfig> {
	Ok(serde_json::from_str(contents)?)
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

fn parse_from_deeplink(deeplink: &str) -> Result<UploadConfig> {
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

pub(crate) fn import_from_source(source: &str, interactive: bool) -> Result<UploadConfig> {
	if source.starts_with("http://") || source.starts_with("https://") {
		parse_from_url(source)
	} else if source.starts_with("framr://") {
		parse_from_deeplink(source)
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
