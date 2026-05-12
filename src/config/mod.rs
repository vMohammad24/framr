pub mod cli_ui;
pub mod core;
pub mod import;
pub mod types;

pub use cli_ui::find_uploader_index;
pub use core::{load_config, load_uploader_config, save_config};
pub(crate) use types::{AppConfig, BodyType, Color, SelectionConfig, UploadConfig};
pub use types::{ConfigEnum, DefaultAction, DefaultCaptureMethod};

use anyhow::Result;
use console::{Term, style};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use libframr::{FramrConnection, H264SpeedPreset, H264Tune};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::config::cli_ui::*;
use crate::config::import::*;

pub fn prompt_input<T>(prompt: &str, default: Option<T>) -> Result<T>
where
	T: std::str::FromStr + std::fmt::Display + Clone,
	T::Err: std::fmt::Display,
{
	let theme = ColorfulTheme::default();
	let mut builder = Input::with_theme(&theme);
	builder = builder.with_prompt(prompt);
	if let Some(d) = default {
		builder = builder.default(d);
	}
	Ok(builder.interact_text()?)
}

pub fn prompt_optional_input(prompt: &str, current: Option<&str>) -> Result<Option<String>> {
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

pub fn prompt_select<T: std::fmt::Display>(
	prompt: &str,
	items: &[T],
	default: usize,
) -> Result<usize> {
	let theme = ColorfulTheme::default();
	Ok(Select::with_theme(&theme)
		.with_prompt(prompt)
		.items(items)
		.default(default)
		.interact()?)
}

pub fn prompt_confirm(prompt: &str, default: bool) -> Result<bool> {
	let theme = ColorfulTheme::default();
	Ok(Confirm::with_theme(&theme)
		.with_prompt(prompt)
		.default(default)
		.interact()?)
}

pub fn prompt_color(prompt: &str, current: Color) -> Result<Color> {
	let theme = ColorfulTheme::default();
	let input: String = Input::with_theme(&theme)
		.with_prompt(prompt)
		.default(current.to_string())
		.validate_with(|input: &String| -> Result<(), String> {
			use std::str::FromStr;
			Color::from_str(input)
				.map(|_| ())
				.map_err(|e| e.to_string())
		})
		.interact_text()?;

	use std::str::FromStr;
	Ok(Color::from_str(&input).unwrap())
}

pub fn import_uploader(source: &str) -> Result<()> {
	let mut cfg = load_config()?;

	println!("{}", header("Import Uploader"));
	println!("  {} {}", style("Source:").bold(), style(source).blue());

	let mut uploader = import_from_source(source, false)?;

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

	cfg.uploaders.push(uploader);
	save_config(&cfg)?;
	print_success("Configuration saved.");
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

	if let Some(method) = cfg.default_capture {
		let label = if method == DefaultCaptureMethod::Screen
			&& let Some(screen) = cfg.default_screen
		{
			format!("{} (screen {})", method.label(), screen)
		} else {
			method.label().to_string()
		};
		println!(
			"  {} {}",
			style("Default Method:").bold(),
			style(label).yellow().bold()
		);
	}

	println!(
		"  {} {}",
		style("Default Sound:").bold(),
		style(&cfg.upload_sound).yellow().bold()
	);

	println!();
	println!("{}", style("Recording Settings:").cyan().bold());
	println!(
		"  {} {}",
		style("Bitrate:").bold(),
		style(cfg.recording.bitrate).yellow()
	);
	println!(
		"  {} {}",
		style("Keyframe Interval:").bold(),
		style(cfg.recording.keyframe_interval).yellow()
	);
	println!(
		"  {} {}",
		style("Threads:").bold(),
		style(
			cfg.recording
				.threads
				.map(|t| t.to_string())
				.unwrap_or_else(|| "Auto".to_string())
		)
		.yellow()
	);
	println!(
		"  {} {}",
		style("H.264 Tune:").bold(),
		style(cfg.recording.tune.as_str()).yellow()
	);
	println!(
		"  {} {}",
		style("H.264 Speed Preset:").bold(),
		style(cfg.recording.speed_preset.as_str()).yellow()
	);

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
	if prompt_confirm(
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

	if cfg.uploaders.is_empty() {
		println!("\n{}", style("No uploaders configured.").yellow());
		return Ok(());
	}

	let idx = resolve_uploader_index(&cfg, name_or_index, "Select default uploader")?;
	let name = &cfg.uploaders[idx].name;

	match &cfg.default_uploader {
		Some(current) if current.eq_ignore_ascii_case(name) => {
			println!(
				"  {}",
				style(&format!("\"{}\" is already the default uploader.", name)).dim()
			);
			return Ok(());
		}
		Some(current) => {
			println!(
				"  {} {} → {}",
				style("Default:").bold(),
				style(current).red(),
				style(name).green().bold()
			);
		}
		None => {
			println!("  {} {}", style("Default:").bold(), style("(none)").red());
		}
	}

	cfg.default_uploader = Some(name.clone());
	save_config(&cfg)?;
	print_success(&format!("Default uploader set to \"{}\".", name));
	Ok(())
}

fn set_default_enum<T: ConfigEnum>(
	current: Option<T>,
	name_or_index: Option<&str>,
	prompt: &str,
	item_name: &str,
) -> Result<T> {
	let idx = match name_or_index {
		Some(n) => {
			let n_lower = n.to_lowercase();
			let variants = T::variants();
			variants
				.iter()
				.position(|v| v.to_lowercase().contains(&n_lower))
				.ok_or_else(|| {
					anyhow::anyhow!(
						"Unknown {} \"{}\". Valid options: {}",
						item_name,
						n,
						variants
							.iter()
							.map(|v| v.to_lowercase())
							.collect::<Vec<_>>()
							.join(", ")
					)
				})?
		}
		None => {
			let default_idx = current.map(|a| a.to_index()).unwrap_or(0);
			let variants = T::variants();
			prompt_select(prompt, &variants, default_idx)?
		}
	};

	let val = T::from_index(idx).unwrap();

	match current {
		Some(curr) if curr == val => {
			println!(
				"  {}",
				style(&format!(
					"\"{}\" is already the default {}.",
					val.label(),
					item_name
				))
				.dim()
			);
		}
		Some(curr) => {
			println!(
				"  {} {} → {}",
				style(&format!("Default {}:", item_name)).bold(),
				style(curr.label()).red(),
				style(val.label()).green().bold()
			);
		}
		None => {
			println!(
				"  {} {}",
				style(&format!("Default {}:", item_name)).bold(),
				style("(none)").red()
			);
		}
	}

	Ok(val)
}

pub fn set_default_action(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;

	let action = set_default_enum(
		cfg.default_action,
		name_or_index,
		"Select default action",
		"action",
	)?;

	if cfg.default_action == Some(action) {
		return Ok(());
	}

	cfg.default_action = Some(action);
	save_config(&cfg)?;
	print_success(&format!("Default action set to \"{}\".", action.label()));
	Ok(())
}

pub fn set_default_capture(name_or_index: Option<&str>) -> Result<()> {
	let mut cfg = load_config()?;

	let method = set_default_enum(
		cfg.default_capture,
		name_or_index,
		"Select default capture method",
		"method",
	)?;

	if method == DefaultCaptureMethod::Screen {
		let conn = FramrConnection::new()?;
		let outputs = conn.get_all_outputs()?;

		if outputs.is_empty() {
			anyhow::bail!("No monitors detected.");
		}

		let items: Vec<String> = outputs
			.iter()
			.enumerate()
			.map(|(i, o)| format!("{}: {}", i, o))
			.collect();

		let current_screen = cfg.default_screen.unwrap_or(0);
		let selection = prompt_select(
			"Select default monitor",
			&items,
			if current_screen < items.len() {
				current_screen
			} else {
				0
			},
		)?;

		cfg.default_screen = Some(selection);
		println!(
			"  {} {}",
			style("Default monitor:").bold(),
			style(&items[selection]).yellow().bold()
		);
	}

	if cfg.default_capture == Some(method) && method != DefaultCaptureMethod::Screen {
		return Ok(());
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
		None => prompt_input("Default upload sound path", Some(cfg.upload_sound.clone()))?,
	};

	cfg.upload_sound = path;
	save_config(&cfg)?;
	print_success("Default upload sound updated.");
	Ok(())
}

pub fn register_protocol_handler() -> Result<()> {
	let xdg_data_home = std::env::var("XDG_DATA_HOME")
		.map(PathBuf::from)
		.or_else(|_| {
			dirs::home_dir()
				.map(|p| p.join(".local/share"))
				.ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
		})?;

	let apps_dir = xdg_data_home.join("applications");
	std::fs::create_dir_all(&apps_dir)?;

	let desktop_file_path = apps_dir.join("framr-handler.desktop");

	if desktop_file_path.exists() {
		let proceed = prompt_confirm(
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

		let capture_label = cfg
			.default_capture
			.map(|m| {
				if m == DefaultCaptureMethod::Screen
					&& let Some(screen) = cfg.default_screen
				{
					format!("{} (screen {})", m.label(), screen)
				} else {
					m.label().to_string()
				}
			})
			.unwrap_or_else(|| style("(none)").dim().to_string());
		println!(
			"  {} {}\n",
			style("Default Method:").bold(),
			cfg.default_capture
				.map(|_| style(&capture_label).yellow().bold().to_string())
				.unwrap_or_else(|| style("(none)").dim().to_string())
		);
		println!(
			"  {} {}\n",
			style("Default Sound:").bold(),
			style(&cfg.upload_sound).yellow().bold()
		);

		println!();
		println!("{}", style("Recording Settings:").cyan().bold());
		println!(
			"  {} {}",
			style("Bitrate:").bold(),
			style(cfg.recording.bitrate).yellow()
		);
		println!(
			"  {} {}",
			style("Keyframe Interval:").bold(),
			style(cfg.recording.keyframe_interval).yellow()
		);
		println!(
			"  {} {}",
			style("Threads:").bold(),
			style(
				cfg.recording
					.threads
					.map(|t| t.to_string())
					.unwrap_or_else(|| "Auto".to_string())
			)
			.yellow()
		);
		println!(
			"  {} {}",
			style("H.264 Tune:").bold(),
			style(cfg.recording.tune.as_str()).yellow()
		);
		println!(
			"  {} {}",
			style("H.264 Speed Preset:").bold(),
			style(cfg.recording.speed_preset.as_str()).yellow()
		);
		println!();

		let selection = prompt_select(
			"Whatcha doin?",
			&[
				"Import uploader (.sxcu / .iscu URL or File)",
				"Create new uploader",
				"Edit existing uploader",
				"Delete uploader",
				"Defaults...",
				"Selection UI settings...",
				"Recording settings...",
				"Save & Exit",
			],
			5,
		)?;

		let _ = term.clear_screen();

		match selection {
			0 => {
				let source: String = prompt_input("Path to file or URL", None)?;

				let mut uploader = import_from_source(&source, true)?;
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

				cfg.uploaders.push(uploader);
				save_config(&cfg)?;
				print_success("Uploader imported and saved successfully.");
				thread::sleep(Duration::from_secs(1));
			}
			1 => {
				create_uploader_interactive(&mut cfg)?;
				save_config(&cfg)?;
				print_success("Uploader created and saved successfully.");
				thread::sleep(Duration::from_secs(1));
			}
			2 => {
				if cfg.uploaders.is_empty() {
					continue;
				}
				let sel = select_uploader_index(&cfg, "Select uploader to edit")?;
				let _ = term.clear_screen();
				modify_uploader_at(&mut cfg, sel)?;
				save_config(&cfg)?;
				print_success("Uploader modified and saved successfully.");
				thread::sleep(Duration::from_secs(1));
			}
			3 => {
				if cfg.uploaders.is_empty() {
					continue;
				}
				let sel = select_uploader_index(&cfg, "Select uploader to delete")?;
				let name = &cfg.uploaders[sel].name;
				if prompt_confirm(
					&format!(
						"Are you sure you want to delete \"{}\"?",
						style(name).red().bold()
					),
					false,
				)? {
					let removed = cfg.uploaders.remove(sel);
					if cfg.default_uploader.as_deref() == Some(&removed.name) {
						cfg.default_uploader = None;
					}
					save_config(&cfg)?;
					print_error(&format!("Deleted \"{}\"", removed.name));
					thread::sleep(Duration::from_secs(1));
				}
			}
			4 => {
				let defaults_selection = prompt_select(
					"Defaults",
					&[
						"Set default uploader",
						"Set default action",
						"Set default capture method",
						"Set default sound",
						"Back",
					],
					4,
				)?;

				match defaults_selection {
					0 => {
						if cfg.uploaders.is_empty() {
							continue;
						}
						let sel = select_uploader_index(&cfg, "Select default uploader")?;
						let name = cfg.uploaders[sel].name.clone();
						cfg.default_uploader = Some(name.clone());
						save_config(&cfg)?;
						print_success(&format!("Default uploader set to \"{}\".", name));
						thread::sleep(Duration::from_secs(1));
					}
					1 => {
						let default_idx = cfg.default_action.map(|a| a.to_index()).unwrap_or(0);
						let variants = DefaultAction::variants();
						let sel = prompt_select("Select default action", &variants, default_idx)?;
						let action = DefaultAction::from_index(sel).unwrap();
						cfg.default_action = Some(action);
						save_config(&cfg)?;
						print_success(&format!("Default action set to \"{}\".", action.label()));
						thread::sleep(Duration::from_secs(1));
					}
					2 => {
						let default_idx = cfg.default_capture.map(|m| m.to_index()).unwrap_or(0);
						let variants = DefaultCaptureMethod::variants();
						let sel =
							prompt_select("Select default capture method", &variants, default_idx)?;
						let method = DefaultCaptureMethod::from_index(sel).unwrap();
						if method == DefaultCaptureMethod::Screen {
							let conn = FramrConnection::new()?;
							let outputs = conn.get_all_outputs()?;
							if outputs.is_empty() {
								print_error("No monitors detected.");
							} else {
								let items: Vec<String> = outputs
									.iter()
									.enumerate()
									.map(|(i, o)| format!("{}: {}", i, o))
									.collect();

								let current_screen = cfg.default_screen.unwrap_or(0);
								let selection = prompt_select(
									"Select default monitor",
									&items,
									if current_screen < items.len() {
										current_screen
									} else {
										0
									},
								)?;

								cfg.default_screen = Some(selection);
								print_success(&format!(
									"Default monitor set to \"{}\".",
									items[selection]
								));
							}
						}
						cfg.default_capture = Some(method);
						save_config(&cfg)?;
						print_success(&format!(
							"Default capture method set to \"{}\".",
							method.label()
						));
						thread::sleep(Duration::from_secs(1));
					}
					3 => {
						let sound: String = prompt_input(
							"Default upload sound path",
							Some(cfg.upload_sound.clone()),
						)?;
						cfg.upload_sound = sound;
						save_config(&cfg)?;
						print_success("Default upload sound updated.");
						thread::sleep(Duration::from_secs(1));
					}
					_ => continue,
				}
			}
			5 => {
				modify_selection_config(&mut cfg)?;
				save_config(&cfg)?;
				print_success("Selection UI settings updated and saved.");
				thread::sleep(Duration::from_secs(1));
			}
			6 => {
				modify_recording_config(&mut cfg)?;
				save_config(&cfg)?;
				print_success("Recording settings updated and saved.");
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

pub fn modify_selection_config(cfg: &mut AppConfig) -> Result<()> {
	loop {
		println!("\n{}", style("Selection UI Settings").cyan().bold());
		println!("{}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━").dim());

		let s = &mut cfg.selection;
		let items = [
			format!(
				"{:<25} {}",
				style("Background Color").bold(),
				style_color(s.background_color)
			),
			format!(
				"{:<25} {}",
				style("Border Color").bold(),
				style_color(s.border_color)
			),
			format!(
				"{:<25} {}",
				style("Border Width").bold(),
				style(s.border_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar BG").bold(),
				style_color(s.toolbar_background_color)
			),
			format!(
				"{:<25} {}",
				style("Toolbar Active").bold(),
				style_color(s.toolbar_active_color)
			),
			format!(
				"{:<25} {}",
				style("Toolbar Hover").bold(),
				style_color(s.toolbar_hover_color)
			),
			format!(
				"{:<25} {}",
				style("Annotation Color").bold(),
				style_color(s.annotation_color)
			),
			format!(
				"{:<25} {}",
				style("Annotation Width").bold(),
				style(s.annotation_line_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Blur Radius").bold(),
				style(s.blur_radius).yellow()
			),
			format!(
				"{:<25} {}",
				style("Pixelate Block Size").bold(),
				style(s.pixelate_block_size).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Y").bold(),
				style(s.toolbar_y).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Item Width").bold(),
				style(s.toolbar_item_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Height").bold(),
				style(s.toolbar_height).yellow()
			),
			style("Back").dim().to_string(),
		];

		let sel = prompt_select("Edit Setting", &items, items.len() - 1)?;

		match sel {
			0 => s.background_color = prompt_color("Background Color", s.background_color)?,
			1 => s.border_color = prompt_color("Border Color", s.border_color)?,
			2 => s.border_width = prompt_input("Border Width", Some(s.border_width))?,
			3 => {
				s.toolbar_background_color =
					prompt_color("Toolbar Background Color", s.toolbar_background_color)?
			}
			4 => {
				s.toolbar_active_color =
					prompt_color("Toolbar Active Color", s.toolbar_active_color)?
			}
			5 => {
				s.toolbar_hover_color = prompt_color("Toolbar Hover Color", s.toolbar_hover_color)?
			}
			6 => s.annotation_color = prompt_color("Annotation Color", s.annotation_color)?,
			7 => {
				s.annotation_line_width =
					prompt_input("Annotation Line Width", Some(s.annotation_line_width))?
			}
			8 => s.blur_radius = prompt_input("Blur Radius", Some(s.blur_radius))?,
			9 => {
				s.pixelate_block_size =
					prompt_input("Pixelate Block Size", Some(s.pixelate_block_size))?
			}
			10 => s.toolbar_y = prompt_input("Toolbar Y", Some(s.toolbar_y))?,
			11 => {
				s.toolbar_item_width =
					prompt_input("Toolbar Item Width", Some(s.toolbar_item_width))?
			}
			12 => s.toolbar_height = prompt_input("Toolbar Height", Some(s.toolbar_height))?,
			_ => break,
		}
	}
	Ok(())
}

fn style_color(c: Color) -> String {
	style(c.to_string())
		.color256(get_color256(c.r, c.g, c.b))
		.to_string()
}

fn get_color256(r: u8, g: u8, b: u8) -> u8 {
	let r = (r as u32 * 5 / 255) as u8;
	let g = (g as u32 * 5 / 255) as u8;
	let b = (b as u32 * 5 / 255) as u8;
	16 + 36 * r + 6 * g + b
}

fn validated_text_input<T, F>(prompt: &str, current: T, validator: F) -> Result<T>
where
	T: std::fmt::Display + std::str::FromStr,
	T::Err: std::fmt::Display,
	F: Fn(&String) -> Result<(), String> + Copy,
{
	let theme = ColorfulTheme::default();
	let val: String = Input::with_theme(&theme)
		.with_prompt(prompt)
		.default(current.to_string())
		.validate_with(validator)
		.interact_text()?;
	val.parse()
		.map_err(|e| anyhow::anyhow!("Invalid input: {}", e))
}

pub fn modify_recording_config(cfg: &mut AppConfig) -> Result<()> {
	loop {
		println!("\n{}", style("Recording Settings").cyan().bold());
		println!("{}", style("━━━━━━━━━━━━━━━━━━━━━━━━━").dim());

		let r = &mut cfg.recording;
		let items = [
			format!(
				"{:<25} {}",
				style("Bitrate (kbps)").bold(),
				style(r.bitrate).yellow()
			),
			format!(
				"{:<25} {}",
				style("Keyframe Interval").bold(),
				style(r.keyframe_interval).yellow()
			),
			format!(
				"{:<25} {}",
				style("Threads").bold(),
				style(
					r.threads
						.map(|t| t.to_string())
						.unwrap_or_else(|| "Auto".to_string())
				)
				.yellow()
			),
			format!(
				"{:<25} {}",
				style("H.264 Tune").bold(),
				style(r.tune.as_str()).yellow()
			),
			format!(
				"{:<25} {}",
				style("H.264 Speed Preset").bold(),
				style(r.speed_preset.as_str()).yellow()
			),
			style("Back").dim().to_string(),
		];

		let sel = prompt_select("Edit Setting", &items, items.len() - 1)?;

		match sel {
			0 => {
				r.bitrate = validated_text_input("Bitrate (kbps)", r.bitrate, |input| {
					let val = input
						.parse::<u32>()
						.map_err(|_| "Must be a valid number".to_string())?;
					if val == 0 {
						Err("Bitrate must be greater than 0".to_string())
					} else {
						Ok(())
					}
				})?;
			}
			1 => {
				r.keyframe_interval = validated_text_input(
					"Keyframe Interval (frames)",
					r.keyframe_interval,
					|input| {
						let val = input
							.parse::<u32>()
							.map_err(|_| "Must be a valid number".to_string())?;
						if val == 0 {
							Err("Keyframe interval must be greater than 0".to_string())
						} else {
							Ok(())
						}
					},
				)?;
			}
			2 => {
				let val: u32 = validated_text_input(
					"Threads (0 for auto)",
					r.threads.unwrap_or(0),
					|input| {
						input
							.parse::<u32>()
							.map(|_| ())
							.map_err(|_| "Must be a valid number or 0 for auto".to_string())
					},
				)?;
				r.threads = if val == 0 { None } else { Some(val) };
			}
			3 => {
				let options = [
					H264Tune::Zerolatency,
					H264Tune::Film,
					H264Tune::Animation,
					H264Tune::Grain,
					H264Tune::Stillimage,
					H264Tune::Fastdecode,
				];
				let names: Vec<_> = options.iter().map(|o| o.as_str()).collect();
				let current = options.iter().position(|o| *o == r.tune).unwrap_or(0);
				let selection = prompt_select("Select H.264 Tune", &names, current)?;
				r.tune = options[selection];
			}
			4 => {
				let options = [
					H264SpeedPreset::Ultrafast,
					H264SpeedPreset::Superfast,
					H264SpeedPreset::Veryfast,
					H264SpeedPreset::Faster,
					H264SpeedPreset::Fast,
					H264SpeedPreset::Medium,
					H264SpeedPreset::Slow,
					H264SpeedPreset::Slower,
					H264SpeedPreset::Veryslow,
					H264SpeedPreset::Placebo,
				];
				let names: Vec<_> = options.iter().map(|o| o.as_str()).collect();
				let current = options
					.iter()
					.position(|o| *o == r.speed_preset)
					.unwrap_or(0);
				let selection = prompt_select("Select H.264 Speed Preset", &names, current)?;
				r.speed_preset = options[selection];
			}
			_ => break,
		}
	}
	Ok(())
}
