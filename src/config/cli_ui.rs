use crate::config::core::{load_config, load_overrides, save_config};
use crate::config::types::{
	AppConfig, BodyType, ConfigEnum, DefaultAction, DefaultCaptureMethod, UploadConfig,
};
use crate::utils::notify::send_notification;
use anyhow::{Result, bail};
use console::{Term, style};
use std::thread;
use std::time::Duration;

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
pub fn import_uploader(source: &str, silent: bool) -> Result<()> {
	let mut cfg = load_config()?;

	println!("{}", header("Import Uploader"));
	println!("  {} {}", style("Source:").bold(), style(source).blue());

	let mut uploader = super::import_from_source(source, false)?;

	let original_name = uploader.name.clone();
	uploader.name = ensure_unique_uploader_name(&cfg, uploader.name);

	if uploader.name != original_name {
		println!(
			"  {} Renamed \"{}\" to \"{}\"",
			style("Note:").yellow().bold(),
			style(&original_name).yellow(),
			style(&uploader.name).yellow()
		);
	}

	println!(
		"\n  {} {} ({})",
		style("Imported:").green().bold(),
		style(&uploader.name).green().bold(),
		style(&uploader.request_url).blue()
	);
	display_uploader_details(&uploader);

	let notification_message = format!("Imported {}", uploader.name);
	cfg.uploaders.push(uploader);
	save_config(&cfg)?;
	print_success("Configuration saved.");
	let _ = send_notification("framr success", &notification_message, None, silent);
	Ok(())
}

pub fn list_uploaders() -> Result<()> {
	let cfg = load_config()?;

	println!("\n{}", style("Framr Config - Uploaders").cyan().bold());

	if cfg.uploaders.is_empty() {
		println!(
			"  {}",
			style(
				"No uploaders configured. Use `framr config import <path>` or `framr config create` to add one."
			)
			.yellow()
		);
		return Ok(());
	}

	for (i, u) in cfg.uploaders.iter().enumerate() {
		let is_default = cfg.default_uploader.as_deref() == Some(&u.name);
		println!("{}", uploader_list_entry(i, u, is_default));
	}

	if let Some(ref default) = cfg.default_uploader {
		println!(
			"\n  {} {}",
			style("Default Uploader:").bold(),
			style(default).yellow().bold()
		);
	}

	if let Some(action) = cfg.default_action {
		println!(
			"  {} {}",
			style("Default Action:").bold(),
			style(action.label()).yellow().bold()
		);
	}

	if let Some(label) = format_capture_label(&cfg) {
		println!(
			"  {} {}",
			style("Default Method:").bold(),
			style(label).yellow().bold()
		);
	}

	println!(
		"  {} {}",
		style("Image Format:").bold(),
		style(cfg.image_format.unwrap_or_default().as_str())
			.yellow()
			.bold()
	);
	println!(
		"  {} {}",
		style("Default Sound:").bold(),
		style(&cfg.upload_sound).yellow().bold()
	);

	display_recording_settings(&cfg);

	println!(
		"\n  {} {}",
		style("Total:").bold(),
		style(cfg.uploaders.len()).yellow().bold()
	);
	Ok(())
}

pub fn show_uploader(name_or_index: &str) -> Result<()> {
	let cfg = load_config()?;

	let idx = find_uploader_index(&cfg, name_or_index)
		.ok_or_else(|| anyhow::anyhow!("Uploader \"{}\" not found.", name_or_index))?;

	println!(
		"{}",
		header(&format!("Uploader: {}", &cfg.uploaders[idx].name))
	);
	display_uploader_details(&cfg.uploaders[idx]);
	Ok(())
}

pub fn create_uploader() -> Result<()> {
	let mut cfg = load_config()?;
	create_uploader_interactive(&mut cfg)?;
	save_config(&cfg)?;
	print_success("Configuration saved.");
	Ok(())
}

pub fn edit_uploader(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;

	if cfg.uploaders.is_empty() {
		println!("\n{}", style("No uploaders to edit.").yellow());
		return Ok(());
	}

	let idx = resolve_uploader_index(&cfg, name_or_index, "Select uploader to edit")?;
	modify_uploader_at(&mut cfg, idx)?;
	save_config(&cfg)?;
	print_success("Configuration saved.");
	Ok(())
}

pub fn delete_uploader(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;

	if cfg.uploaders.is_empty() {
		println!("\n{}", style("No uploaders to delete.").yellow());
		return Ok(());
	}

	let idx = resolve_uploader_index(&cfg, name_or_index, "Select uploader to delete")?;

	let uploader_name = cfg.uploaders[idx].name.clone();
	if super::prompt_confirm(
		&format!(
			"Delete uploader \"{}\"?",
			style(&uploader_name).red().bold()
		),
		false,
	)? {
		cfg.uploaders.remove(idx);
		if cfg.default_uploader.as_deref() == Some(&uploader_name) {
			cfg.default_uploader = None;
		}
		save_config(&cfg)?;
		print_error(&format!("Deleted \"{}\"", uploader_name));
	} else {
		println!("  {}", style("Cancelled.").dim());
	}

	Ok(())
}

pub fn set_default_uploader(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;
	if let Some(n) = name_or_index {
		let idx = find_uploader_index(&cfg, n)
			.ok_or_else(|| anyhow::anyhow!("Uploader \"{}\" not found.", n))?;
		cfg.default_uploader = Some(cfg.uploaders[idx].name.clone());
	} else {
		let idx = select_uploader_index(&cfg, "Select default uploader")?;
		cfg.default_uploader = Some(cfg.uploaders[idx].name.clone());
	}
	save_config(&cfg)?;
	print_success(&format!(
		"Default uploader set to \"{}\".",
		cfg.default_uploader.as_ref().unwrap()
	));
	Ok(())
}

pub fn set_default_action(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;
	let action = if let Some(n) = name_or_index {
		let variants = DefaultAction::variants();
		let idx = variants
			.iter()
			.position(|v| v.to_lowercase().contains(&n.to_lowercase()))
			.ok_or_else(|| anyhow::anyhow!("Unknown action \"{}\"", n))?;
		DefaultAction::from_index(idx).unwrap()
	} else {
		let variants = DefaultAction::variants();
		let idx = super::prompt_select("Select default action", &variants, 0)?;
		DefaultAction::from_index(idx).unwrap()
	};
	cfg.default_action = Some(action);
	save_config(&cfg)?;
	print_success(&format!("Default action set to \"{}\".", action.label()));
	Ok(())
}

pub fn set_default_capture(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;
	let method = if let Some(n) = name_or_index {
		let variants = DefaultCaptureMethod::variants();
		let idx = variants
			.iter()
			.position(|v| v.to_lowercase().contains(&n.to_lowercase()))
			.ok_or_else(|| anyhow::anyhow!("Unknown method \"{}\"", n))?;
		DefaultCaptureMethod::from_index(idx).unwrap()
	} else {
		let variants = DefaultCaptureMethod::variants();
		let idx = super::prompt_select("Select default capture method", &variants, 0)?;
		DefaultCaptureMethod::from_index(idx).unwrap()
	};

	if method == DefaultCaptureMethod::Screen {
		let conn = libframr::FramrConnection::new()?;
		let outputs = conn.get_all_outputs()?;
		if outputs.is_empty() {
			bail!("No monitors detected.");
		}
		let items: Vec<String> = outputs
			.iter()
			.enumerate()
			.map(|(i, o)| format!("{}: {}", i, o))
			.collect();
		let selection = super::prompt_select("Select default monitor", &items, 0)?;
		cfg.default_screen = Some(selection);
	}

	cfg.default_capture = Some(method);
	save_config(&cfg)?;
	print_success(&format!(
		"Default capture method set to \"{}\".",
		method.label()
	));
	Ok(())
}

pub fn set_default_sound(path: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;
	let path = match path {
		Some(p) => p.to_string(),
		None => super::prompt_input("Default upload sound path", Some(cfg.upload_sound.clone()))?,
	};
	cfg.upload_sound = path;
	save_config(&cfg)?;
	print_success("Default upload sound updated.");
	Ok(())
}

pub fn set_default_format(name: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;
	use libframr::OutputImageFormat;
	let format = match name {
		Some(n) => n
			.parse::<OutputImageFormat>()
			.map_err(|e| anyhow::anyhow!("{}", e))?,
		None => {
			use strum::IntoEnumIterator;
			let formats: Vec<_> = OutputImageFormat::iter().collect();
			let names: Vec<_> = formats.iter().map(|f| f.as_str()).collect();
			let sel = super::prompt_select("Select default image format", &names, 0)?;
			formats[sel]
		}
	};
	cfg.image_format = Some(format);
	save_config(&cfg)?;
	print_success(&format!(
		"Default image format set to \"{}\".",
		format.as_str()
	));
	Ok(())
}

pub fn set_image_quality(quality: Option<u8>) -> Result<()> {
	let mut cfg = load_config()?;
	let quality = match quality {
		Some(q) => q,
		None => super::prompt_input(
			"Image quality (1-100)",
			Some(cfg.image_quality.unwrap_or(90)),
		)?,
	};
	cfg.image_quality = Some(quality);
	save_config(&cfg)?;
	print_success(&format!("Image quality set to {}%.", quality));
	Ok(())
}

pub fn create_uploader_interactive(cfg: &mut AppConfig) -> Result<()> {
	print!("{}", header("Create new uploader"));

	let mut uploader = UploadConfig {
		name: loop {
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
		},
		..Default::default()
	};

	modify_uploader_menu(&mut uploader)?;

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

	modify_uploader_menu(uploader)
}

pub fn modify_uploader_menu(uploader: &mut UploadConfig) -> Result<()> {
	crate::interactive_menu!(
		&format!("Modifying {}", uploader.name),
		uploader,
		[
			name: "Name" => text,
			request_url: "Request URL" => text,
			request_method: "Request Method" => enum,
			body_type: "Body Type" => enum,
			file_form_name: "File Form Name" => opt_text if uploader.body_type == BodyType::FormData,
			headers: "Headers" => kv,
			parameters: "URL Parameters" => kv,
			arguments: "Body Arguments" => kv,
			output_url: "Output URL" => text,
			error_message: "Error Message" => opt_text,
			deletion_url: "Deletion URL" => opt_text,
			deletion_request_type: "Deletion Method" => enum if uploader.deletion_url.is_some(),
		]
	)
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
	print_setting("Video Encoder:", cfg.recording.encoder.as_ref());
	print_setting("Bitrate:", format!("{} kbps", cfg.recording.bitrate));
	print_setting("Keyframe Interval:", cfg.recording.keyframe_interval);
	print_setting(
		"Threads:",
		cfg.recording
			.threads
			.map(|t| t.to_string())
			.unwrap_or_else(|| "Auto".to_string()),
	);
	if cfg.recording.encoder == libframr::VideoEncoder::H264 {
		print_setting("H.264 Tune:", cfg.recording.tune.as_ref());
	}
	print_setting("Encoder Speed:", cfg.recording.speed.as_ref());
	println!();
	println!("{}", style("Image Settings:").cyan().bold());
	print_setting("Format:", cfg.image_format.unwrap_or_default().as_str());
	print_setting("Quality:", format!("{}%", cfg.image_quality.unwrap_or(90)));
}

pub fn register_protocol_handler() -> Result<()> {
	let xdg_data_home = std::env::var("XDG_DATA_HOME")
		.map(std::path::PathBuf::from)
		.or_else(|_| {
			dirs::home_dir()
				.map(|p| p.join(".local/share"))
				.ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
		})?;

	let apps_dir = xdg_data_home.join("applications");
	std::fs::create_dir_all(&apps_dir)?;

	let desktop_file_path = apps_dir.join("framr-handler.desktop");

	if desktop_file_path.exists() {
		let proceed = super::prompt_confirm(
			&format!(
				"Protocol handler already exists at {}. Overwrite?",
				style(desktop_file_path.display()).blue()
			),
			false,
		)?;

		if !proceed {
			println!("  {} Registration cancelled.", style("ℹ").blue().bold());
			return Ok(());
		}
	}

	let exe_path = std::env::current_exe()?;
	let exe_str = exe_path
		.to_str()
		.ok_or_else(|| anyhow::anyhow!("Invalid executable path"))?;

	if exe_str.contains("/target/") {
		println!(
			"  {} {}",
			style("Warning:").yellow().bold(),
			style("Registering a handler pointing to a build directory. Deep links will break if you move or delete this binary.").yellow()
		);
	}

	let content = format!(
		r#"[Desktop Entry]
Name=Framr Deeplink Handler
Exec={} %u
Type=Application
Terminal=false
MimeType=x-scheme-handler/framr;
NoDisplay=true
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1
Comment=Handle framr:// deeplinks for importing uploaders
"#,
		exe_str
	);

	std::fs::write(&desktop_file_path, content)?;

	println!(
		"  {} Protocol handler registered at {}",
		style("✔").green().bold(),
		style(desktop_file_path.display()).blue()
	);

	match std::process::Command::new("update-desktop-database")
		.arg(&apps_dir)
		.status()
	{
		Ok(status) if !status.success() => {
			println!(
				"  {} {}",
				style("Note:").yellow().bold(),
				style(
					"'update-desktop-database' returned an error. You may need to log out for links to work."
				)
				.dim()
			);
		}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
			println!(
				"  {} {}",
				style("Note:").dim().bold(),
				style("'update-desktop-database' not found. You may need to restart your session for the protocol handler to be recognized.").dim()
			);
		}
		_ => {}
	}

	Ok(())
}

pub fn run_config_wizard() -> Result<()> {
	let mut cfg = load_config()?;
	let term = Term::stdout();

	loop {
		let _ = term.clear_screen();

		println!("\n{}", style("Configuration Menu").cyan().bold());
		println!("{}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━").dim());

		if cfg.uploaders.is_empty() {
			println!("  {}\n", style("(No uploaders currently configured)").dim());
		} else {
			for (i, u) in cfg.uploaders.iter().enumerate() {
				let is_default = cfg.default_uploader.as_deref() == Some(&u.name);
				println!("{}", uploader_list_entry(i, u, is_default));
			}
			println!();
		}

		println!(
			"  {} {}",
			style("Default Uploader:").bold(),
			cfg.default_uploader
				.as_deref()
				.map(|n| style(n).yellow().bold().to_string())
				.unwrap_or_else(|| style("(none)").dim().to_string())
		);
		println!(
			"  {} {}\n",
			style("Default Action:").bold(),
			cfg.default_action
				.map(|a| style(a.label()).yellow().bold().to_string())
				.unwrap_or_else(|| style("(none)").dim().to_string())
		);

		let method_label = format_capture_label(&cfg)
			.map(|l| style(l).yellow().bold().to_string())
			.unwrap_or_else(|| style("(none)").dim().to_string());
		println!("  {} {}\n", style("Default Method:").bold(), method_label);
		println!(
			"  {} {}",
			style("Image Format:").bold(),
			style(cfg.image_format.unwrap_or_default().as_str())
				.yellow()
				.bold()
		);
		println!(
			"  {} {}%",
			style("Image Quality:").bold(),
			style(cfg.image_quality.unwrap_or(90).to_string())
				.yellow()
				.bold()
		);
		println!(
			"  {} {}\n",
			style("Default Sound:").bold(),
			style(&cfg.upload_sound).yellow().bold()
		);

		display_recording_settings(&cfg);
		println!();

		let selection = super::prompt_select(
			"Whatcha doin?",
			&[
				"Import uploader (.sxcu / .iscu URL or File)",
				"Create new uploader",
				"Edit existing uploader",
				"Delete uploader",
				"General settings...",
				"Selection UI settings...",
				"Recording settings...",
				"Save & Exit",
			],
			7,
		)?;

		let _ = term.clear_screen();

		match selection {
			0 => {
				let source: String = super::prompt_input("Path to file or URL", None)?;
				import_uploader(&source, true)?;
				thread::sleep(Duration::from_secs(1));
			}
			1 => {
				create_uploader()?;
				thread::sleep(Duration::from_secs(1));
			}
			2 => {
				edit_uploader(None)?;
				thread::sleep(Duration::from_secs(1));
			}
			3 => {
				delete_uploader(None)?;
				thread::sleep(Duration::from_secs(1));
			}
			4 => {
				modify_app_config(&mut cfg)?;
				save_config(&cfg)?;
				print_success("General settings updated.");
				thread::sleep(Duration::from_secs(1));
			}
			5 => {
				modify_selection_config(&mut cfg)?;
				save_config(&cfg)?;
				print_success("Selection UI settings updated.");
				thread::sleep(Duration::from_secs(1));
			}
			6 => {
				modify_recording_config(&mut cfg)?;
				save_config(&cfg)?;
				print_success("Recording settings updated.");
				thread::sleep(Duration::from_secs(1));
			}
			_ => {
				save_config(&cfg)?;
				print_success("Configuration saved. Exiting...");
				return Ok(());
			}
		}
	}
}

pub fn modify_app_config(cfg: &mut AppConfig) -> Result<()> {
	crate::interactive_menu!(
		"General Settings",
		cfg,
		[
			default_uploader: "Default Uploader" => uploader,
			default_action: "Default Action" => opt_enum [DefaultAction],
			default_capture: "Default Capture" => custom [edit_capture_method] display_as opt_enum,
			image_format: "Image Format" => opt_enum [libframr::OutputImageFormat],
			image_quality: "Image Quality" => opt_num,
			upload_sound: "Upload Sound" => text,
		]
	)
}

fn edit_capture_method(s: &mut AppConfig) -> Result<()> {
	let variants = DefaultCaptureMethod::variants();
	let current_idx = s.default_capture.map(|v| v.to_index()).unwrap_or(0);
	let sel = super::prompt_select("Default Capture", &variants, current_idx)?;
	let method = DefaultCaptureMethod::from_index(sel).unwrap();

	if method == DefaultCaptureMethod::Screen {
		let conn = libframr::FramrConnection::new()?;
		let outputs = conn.get_all_outputs()?;

		if outputs.is_empty() {
			print_error("No monitors detected.");
		} else {
			let items: Vec<String> = outputs
				.iter()
				.enumerate()
				.map(|(i, o)| format!("{}: {}", i, o))
				.collect();

			let current_screen = s.default_screen.unwrap_or(0);
			let selection = super::prompt_select(
				"Select default monitor",
				&items,
				if current_screen < items.len() {
					current_screen
				} else {
					0
				},
			)?;

			s.default_screen = Some(selection);
		}
	}
	s.default_capture = Some(method);
	Ok(())
}

pub fn modify_selection_config(cfg: &mut AppConfig) -> Result<()> {
	crate::interactive_menu!(
		"Selection UI Settings",
		cfg.selection,
		[
			background_color: "Background Color" => color,
			border_color: "Border Color" => color,
			border_width: "Border Width" => num,
			toolbar_background_color: "Toolbar BG" => color,
			toolbar_active_color: "Toolbar Active" => color,
			toolbar_hover_color: "Toolbar Hover" => color,
			highlight_color: "Highlight Color" => color,
			annotation_color: "Annotation Color" => color,
			annotation_line_width: "Annotation Width" => num,
			blur_radius: "Blur Radius" => num,
			pixelate_block_size: "Pixelate Block Size" => num,
			toolbar_y: "Toolbar Y" => num,
			toolbar_item_width: "Toolbar Item Width" => num,
			toolbar_height: "Toolbar Height" => num,
			show_toolbar: "Show Toolbar" => bool,
		]
	)
}

pub fn modify_recording_config(cfg: &mut AppConfig) -> Result<()> {
	crate::interactive_menu!(
		"Recording Settings",
		cfg.recording,
		[
			encoder: "Video Encoder" => enum,
			bitrate: "Bitrate (kbps)" => nonzero_num,
			keyframe_interval: "Keyframe Interval" => nonzero_num,
			threads: "Threads" => opt_num,
			tune: "H.264 Tune" => enum if cfg.recording.encoder == libframr::VideoEncoder::H264,
			speed: "Encoder Speed" => enum,
		]
	)
}
