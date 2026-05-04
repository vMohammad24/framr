mod handler;
mod types;

pub(crate) use handler::find_uploader_index;
pub use handler::load_config;
pub use handler::load_uploader_config;
pub(crate) use types::{AppConfig, BodyType, Color, SelectionConfig, UploadConfig};
pub use types::{ConfigEnum, DefaultAction, DefaultCaptureMethod};

use anyhow::Result;
use console::{Term, style};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use libframr::FramrConnection;
use std::path::PathBuf;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use handler::*;

pub fn import_uploader(source: &str) -> Result<()> {
	let mut cfg = load_config()?;

	println!("{}", display_header("Import Uploader"));
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
	display_uploader_full_details(&uploader);

	cfg.uploaders.push(uploader);
	save_config(&cfg)?;
	display_success("Configuration saved.");
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
		println!("{}", display_uploader_list_entry(i, u, is_default));
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
		display_header(&format!("Uploader: {}", &cfg.uploaders[idx].name))
	);
	display_uploader_full_details(&cfg.uploaders[idx]);
	Ok(())
}

pub fn create_uploader() -> Result<()> {
	let mut cfg = load_config()?;
	create_uploader_interactive(&mut cfg)?;
	save_config(&cfg)?;
	display_success("Configuration saved.");
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
	display_success("Configuration saved.");
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
	if Confirm::with_theme(&ColorfulTheme::default())
		.with_prompt(format!(
			"Delete uploader \"{}\"?",
			style(&uploader_name).red().bold()
		))
		.default(false)
		.interact()?
	{
		cfg.uploaders.remove(idx);
		if cfg.default_uploader.as_deref() == Some(&uploader_name) {
			cfg.default_uploader = None;
		}
		save_config(&cfg)?;
		display_error(&format!("Deleted \"{}\"", uploader_name));
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
	display_success(&format!("Default uploader set to \"{}\".", name));
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
			T::variants()
				.iter()
				.position(|v| v.to_lowercase().contains(&n_lower))
				.ok_or_else(|| {
					anyhow::anyhow!(
						"Unknown {} \"{}\". Valid options: {}",
						item_name,
						n,
						T::variants()
							.iter()
							.map(|v| v.to_lowercase())
							.collect::<Vec<_>>()
							.join(", ")
					)
				})?
		}
		None => {
			let default_idx = current.map(|a| a.to_index()).unwrap_or(0);
			Select::with_theme(&ColorfulTheme::default())
				.with_prompt(prompt)
				.items(T::variants())
				.default(default_idx)
				.interact()?
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
	display_success(&format!("Default action set to \"{}\".", action.label()));
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

		let default_idx = cfg.default_screen.unwrap_or(0);
		let selection = Select::with_theme(&ColorfulTheme::default())
			.with_prompt("Select default monitor")
			.items(&items)
			.default(if default_idx < items.len() {
				default_idx
			} else {
				0
			})
			.interact()?;

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
	display_success(&format!(
		"Default capture method set to \"{}\".",
		method.label()
	));
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
		let theme = ColorfulTheme::default();
		let proceed = Confirm::with_theme(&theme)
			.with_prompt(format!(
				"Protocol handler already exists at {}. Overwrite?",
				style(desktop_file_path.display()).blue()
			))
			.default(false)
			.interact()?;

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

	// Try to update desktop database
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
	let theme = ColorfulTheme::default();
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
				println!("{}", display_uploader_list_entry(i, u, is_default));
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

		let selection = Select::with_theme(&theme)
			.with_prompt("Whatcha doin?")
			.items([
				"Import uploader (.sxcu / .iscu URL or File)",
				"Create new uploader",
				"Edit existing uploader",
				"Delete uploader",
				"Defaults...",
				"Selection UI settings...",
				"Save & Exit",
			])
			.default(5)
			.interact()?;

		let _ = term.clear_screen();

		match selection {
			0 => {
				let source: String = Input::with_theme(&theme)
					.with_prompt("Path to file or URL")
					.interact_text()?;

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
				display_success("Uploader imported and saved successfully.");
				thread::sleep(Duration::from_secs(1));
			}
			1 => {
				create_uploader_interactive(&mut cfg)?;
				save_config(&cfg)?;
				display_success("Uploader created and saved successfully.");
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
				display_success("Uploader modified and saved successfully.");
				thread::sleep(Duration::from_secs(1));
			}
			3 => {
				if cfg.uploaders.is_empty() {
					continue;
				}
				let sel = select_uploader_index(&cfg, "Select uploader to delete")?;
				let name = &cfg.uploaders[sel].name;
				if Confirm::with_theme(&theme)
					.with_prompt(format!(
						"Are you sure you want to delete \"{}\"?",
						style(name).red().bold()
					))
					.default(false)
					.interact()?
				{
					let removed = cfg.uploaders.remove(sel);
					if cfg.default_uploader.as_deref() == Some(&removed.name) {
						cfg.default_uploader = None;
					}
					save_config(&cfg)?;
					display_error(&format!("Deleted \"{}\"", removed.name));
					thread::sleep(Duration::from_secs(1));
				}
			}
			4 => {
				let defaults_selection = Select::with_theme(&theme)
					.with_prompt("Defaults")
					.items([
						"Set default uploader",
						"Set default action",
						"Set default capture method",
						"Back",
					])
					.interact()?;

				match defaults_selection {
					0 => {
						if cfg.uploaders.is_empty() {
							continue;
						}
						let sel = select_uploader_index(&cfg, "Select default uploader")?;
						let name = cfg.uploaders[sel].name.clone();
						cfg.default_uploader = Some(name.clone());
						save_config(&cfg)?;
						display_success(&format!("Default uploader set to \"{}\".", name));
						thread::sleep(Duration::from_secs(1));
					}
					1 => {
						let default_idx = cfg.default_action.map(|a| a.to_index()).unwrap_or(0);
						let sel = Select::with_theme(&theme)
							.with_prompt("Select default action")
							.items(DefaultAction::variants())
							.default(default_idx)
							.interact()?;
						let action = DefaultAction::from_index(sel).unwrap();
						cfg.default_action = Some(action);
						save_config(&cfg)?;
						display_success(&format!("Default action set to \"{}\".", action.label()));
						thread::sleep(Duration::from_secs(1));
					}
					2 => {
						let default_idx = cfg.default_capture.map(|m| m.to_index()).unwrap_or(0);
						let sel = Select::with_theme(&theme)
							.with_prompt("Select default capture method")
							.items(DefaultCaptureMethod::variants())
							.default(default_idx)
							.interact()?;
						let method = DefaultCaptureMethod::from_index(sel).unwrap();
						if method == DefaultCaptureMethod::Screen {
							let conn = FramrConnection::new()?;
							let outputs = conn.get_all_outputs()?;
							if outputs.is_empty() {
								display_error("No monitors detected.");
							} else {
								let items: Vec<String> = outputs
									.iter()
									.enumerate()
									.map(|(i, o)| format!("{}: {}", i, o))
									.collect();

								let current_screen = cfg.default_screen.unwrap_or(0);
								let selection = Select::with_theme(&theme)
									.with_prompt("Select default monitor")
									.items(&items)
									.default(if current_screen < items.len() {
										current_screen
									} else {
										0
									})
									.interact()?;

								cfg.default_screen = Some(selection);
								display_success(&format!(
									"Default monitor set to \"{}\".",
									items[selection]
								));
							}
						}
						cfg.default_capture = Some(method);
						save_config(&cfg)?;
						display_success(&format!(
							"Default capture method set to \"{}\".",
							method.label()
						));
						thread::sleep(Duration::from_secs(1));
					}
					_ => continue,
				}
			}
			5 => {
				modify_selection_config(&mut cfg)?;
				save_config(&cfg)?;
				display_success("Selection UI settings updated and saved.");
				thread::sleep(Duration::from_secs(1));
			}
			_ => {
				save_config(&cfg)?;
				display_success("Configuration saved. Exiting...");
				return Ok(());
			}
		}
	}
}

pub fn modify_selection_config(cfg: &mut AppConfig) -> Result<()> {
	let theme = ColorfulTheme::default();
	let mut s = cfg.selection;

	loop {
		println!("\n{}", style("Selection UI Settings").cyan().bold());
		println!("{}", style("━━━━━━━━━━━━━━━━━━━━━━━━━━━").dim());

		let items = [
			format!(
				"{:<25} {}",
				style("Background Color:").bold(),
				style_color(s.background_color)
			),
			format!(
				"{:<25} {}",
				style("Border Color:").bold(),
				style_color(s.border_color)
			),
			format!(
				"{:<25} {}",
				style("Border Width:").bold(),
				style(s.border_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar BG:").bold(),
				style_color(s.toolbar_background_color)
			),
			format!(
				"{:<25} {}",
				style("Toolbar Active:").bold(),
				style_color(s.toolbar_active_color)
			),
			format!(
				"{:<25} {}",
				style("Toolbar Hover:").bold(),
				style_color(s.toolbar_hover_color)
			),
			format!(
				"{:<25} {}",
				style("Annotation Color:").bold(),
				style_color(s.annotation_color)
			),
			format!(
				"{:<25} {}",
				style("Annotation Width:").bold(),
				style(s.annotation_line_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Blur Radius:").bold(),
				style(s.blur_radius).yellow()
			),
			format!(
				"{:<25} {}",
				style("Pixelate Block Size:").bold(),
				style(s.pixelate_block_size).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Y:").bold(),
				style(s.toolbar_y).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Item Width:").bold(),
				style(s.toolbar_item_width).yellow()
			),
			format!(
				"{:<25} {}",
				style("Toolbar Height:").bold(),
				style(s.toolbar_height).yellow()
			),
			style("Back").dim().to_string(),
		];

		let sel = Select::with_theme(&theme)
			.with_prompt("Edit Setting")
			.items(&items)
			.default(items.len() - 1)
			.interact()?;

		match sel {
			0 => s.background_color = input_color("Background Color", s.background_color)?,
			1 => s.border_color = input_color("Border Color", s.border_color)?,
			2 => {
				s.border_width = Input::with_theme(&theme)
					.with_prompt("Border Width")
					.default(s.border_width)
					.interact_text()?
			}
			3 => {
				s.toolbar_background_color =
					input_color("Toolbar Background Color", s.toolbar_background_color)?
			}
			4 => {
				s.toolbar_active_color =
					input_color("Toolbar Active Color", s.toolbar_active_color)?
			}
			5 => s.toolbar_hover_color = input_color("Toolbar Hover Color", s.toolbar_hover_color)?,
			6 => s.annotation_color = input_color("Annotation Color", s.annotation_color)?,
			7 => {
				s.annotation_line_width = Input::with_theme(&theme)
					.with_prompt("Annotation Line Width")
					.default(s.annotation_line_width)
					.interact_text()?
			}
			8 => {
				s.blur_radius = Input::with_theme(&theme)
					.with_prompt("Blur Radius")
					.default(s.blur_radius)
					.interact_text()?
			}
			9 => {
				s.pixelate_block_size = Input::with_theme(&theme)
					.with_prompt("Pixelate Block Size")
					.default(s.pixelate_block_size)
					.interact_text()?
			}
			10 => {
				s.toolbar_y = Input::with_theme(&theme)
					.with_prompt("Toolbar Y")
					.default(s.toolbar_y)
					.interact_text()?
			}
			11 => {
				s.toolbar_item_width = Input::with_theme(&theme)
					.with_prompt("Toolbar Item Width")
					.default(s.toolbar_item_width)
					.interact_text()?
			}
			12 => {
				s.toolbar_height = Input::with_theme(&theme)
					.with_prompt("Toolbar Height")
					.default(s.toolbar_height)
					.interact_text()?
			}
			_ => break,
		}
	}

	cfg.selection = s;
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

fn input_color(prompt: &str, current: Color) -> Result<Color> {
	let theme = ColorfulTheme::default();
	let current_hex = current.to_string();

	let input: String = Input::with_theme(&theme)
		.with_prompt(prompt)
		.default(current_hex)
		.validate_with(|input: &String| -> Result<(), String> {
			Color::from_str(input)
				.map(|_| ())
				.map_err(|e| e.to_string())
		})
		.interact_text()?;

	Ok(Color::from_str(&input).unwrap())
}
