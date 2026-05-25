use crate::config::core::load_overrides;
use crate::config::types::{AppConfig, BodyType, ConfigEnum, DefaultCaptureMethod, UploadConfig};
use anyhow::Result;
use console::style;

pub fn header(text: &str) -> String {
	style(format!("\n=== {} ===", text))
		.cyan()
		.bold()
		.to_string()
}

pub fn print_success(text: &str) {
	println!("{} {}", style("✔").green().bold(), style(text).green());
}

pub fn print_error(text: &str) {
	println!("{} {}", style("✖").red().bold(), style(text).red());
}

fn detail(label: &str, value: impl std::fmt::Display) {
	println!("  {:<18} {}", style(label).bold(), value);
}

fn print_setting(label: &str, value: impl std::fmt::Display) {
	println!("  {} {}", style(label).bold(), style(value).yellow());
}

pub fn uploader_list_entry(index: usize, uploader: &UploadConfig, is_default: bool) -> String {
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

pub fn display_uploader_details(uploader: &UploadConfig) {
	detail("Name:", style(&uploader.name).green().bold());
	detail(
		"Request Method:",
		style(&uploader.request_method).magenta().bold(),
	);
	detail("Request URL:", style(&uploader.request_url).blue());
	detail("Body Type:", style(uploader.body_type.label()).magenta());

	if let Some(ref form_name) = uploader.file_form_name {
		detail("File Form Name:", style(form_name).cyan());
	}

	display_kv_pairs("Headers", &uploader.headers);
	display_kv_pairs("URL Parameters", &uploader.parameters);
	display_kv_pairs("Body Arguments", &uploader.arguments);

	println!();
	detail("Output URL:", style(&uploader.output_url).green());

	if let Some(ref err_msg) = uploader.error_message {
		detail("Error Message:", style(err_msg).red());
	}
	println!();
}

pub fn display_kv_pairs(label: &str, pairs: &[(String, String)]) {
	println!("  {}:", style(label).bold());
	if pairs.is_empty() {
		println!("    {}", style("(none)").dim());
	} else {
		for (k, v) in pairs {
			println!("    {} = {}", style(k).yellow(), style(v).cyan());
		}
	}
}

pub fn find_uploader_index(cfg: &AppConfig, name_or_index: &str) -> Option<usize> {
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

pub fn ensure_unique_uploader_name(cfg: &AppConfig, name: String) -> String {
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

pub fn select_uploader_index(cfg: &AppConfig, prompt: &str) -> Result<usize> {
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

pub fn resolve_uploader_index(
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

pub fn manage_kv_pairs(label: &str, pairs: &mut Vec<(String, String)>) -> Result<()> {
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

pub fn select_request_method(default_idx: usize) -> Result<String> {
	let methods = ["POST", "GET", "PUT", "PATCH", "DELETE"];
	let sel = super::prompt_select("Request method", &methods, default_idx)?;
	Ok(methods[sel].to_string())
}

pub fn create_uploader_interactive(cfg: &mut AppConfig) -> Result<()> {
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

	let output_url: String =
		super::prompt_input("Output URL parse schema", Some("{json:url}".into()))?;
	let error_message = super::prompt_optional_input("Error message schema", None)?;
	let deletion_url = super::prompt_optional_input("Deletion URL", None)?;

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
		deletion_url,
		deletion_request_type: String::new(),
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

pub fn modify_uploader_at(cfg: &mut AppConfig, idx: usize) -> Result<()> {
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
	let body_selection =
		super::prompt_select("Body type", &variants, uploader.body_type.to_index())?;
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

	uploader.output_url =
		super::prompt_input("Output URL parse schema", Some(uploader.output_url.clone()))?;
	uploader.error_message =
		super::prompt_optional_input("Error message schema", uploader.error_message.as_deref())?;

	Ok(())
}

pub fn format_capture_label(cfg: &AppConfig) -> Option<String> {
	cfg.default_capture.map(|m| match (m, cfg.default_screen) {
		(DefaultCaptureMethod::Screen, Some(screen)) => {
			format!("{} (screen {})", m.label(), screen)
		}
		_ => m.label().to_string(),
	})
}

pub fn display_recording_settings(cfg: &AppConfig) {
	println!();
	println!("{}", style("Recording Settings:").cyan().bold());
	print_setting("Bitrate:", cfg.recording.bitrate);
	print_setting("Keyframe Interval:", cfg.recording.keyframe_interval);
	print_setting(
		"Threads:",
		cfg.recording
			.threads
			.map(|t| t.to_string())
			.unwrap_or_else(|| "Auto".to_string()),
	);
	print_setting("H.264 Tune:", cfg.recording.tune.as_str());
	print_setting("H.264 Speed Preset:", cfg.recording.speed_preset.as_str());
}
