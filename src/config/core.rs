use crate::config::types::{AppConfig, ConfigOverrides};
use anyhow::Result;
#[cfg(feature = "upload")]
use anyhow::{Context, bail};
use std::fs;
#[cfg(feature = "upload")]
use std::path::Path;
use std::path::PathBuf;

macro_rules! merge_fields {
	($base:expr, $over:expr, [$($field:ident),* $(,)?], [$($opt_field:ident),* $(,)?]) => {
		$(
			if let Some(val) = $over.$field {
				$base.$field = val;
			}
		)*
		$(
			if $over.$opt_field.is_some() {
				$base.$opt_field = $over.$opt_field;
			}
		)*
	};
}

macro_rules! restore_fields {
	($to_save:expr, $over:expr, $stored:expr, [$($field:ident),* $(,)?], [$($opt_field:ident),* $(,)?]) => {
		$(
			if $over.$field.as_ref() == Some(&$to_save.$field) {
				$to_save.$field = $stored.$field;
			}
		)*
		$(
			if $over.$opt_field.is_some() && $to_save.$opt_field == $over.$opt_field {
				$to_save.$opt_field = $stored.$opt_field;
			}
		)*
	};
}

pub fn load_config() -> Result<AppConfig> {
	let app_name = env!("CARGO_PKG_NAME");
	let mut cfg: AppConfig = confy::load(app_name, None)?;

	if let Some(over) = load_overrides() {
		merge_configs(&mut cfg, over);
	}

	Ok(cfg)
}

pub fn load_overrides() -> Option<ConfigOverrides> {
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

fn merge_configs(base: &mut AppConfig, over: ConfigOverrides) {
	#[cfg(feature = "upload")]
	merge_fields!(base, over, [upload_sound], [default_uploader]);
	merge_fields!(
		base,
		over,
		[],
		[
			default_action,
			default_capture,
			default_screen,
			image_format,
			image_quality,
		]
	);
	if let Some(silent) = over.silent {
		let base_silent = base.silent.get_or_insert_default();
		merge_fields!(base_silent, silent, [], [image, video, error]);
		#[cfg(feature = "upload")]
		merge_fields!(base_silent, silent, [], [upload]);
	}

	#[cfg(feature = "annotations")]
	if let Some(selection) = over.selection {
		merge_fields!(
			base.selection,
			selection,
			[
				background_color,
				border_color,
				border_width,
				toolbar_background_color,
				toolbar_active_color,
				toolbar_hover_color,
				annotation_color,
				annotation_line_width,
				blur_radius,
				pixelate_block_size,
				highlight_color,
				toolbar_y,
				toolbar_item_width,
				toolbar_height,
				show_toolbar,
				multi_region_mode,
			],
			[]
		);
	}
	if let Some(recording) = over.recording {
		merge_fields!(
			base.recording,
			recording,
			[
				encoder,
				container,
				bitrate,
				fps,
				keyframe_interval,
				tune,
				speed,
				backend,
			],
			[threads, vaapi_device]
		);
	}

	#[cfg(feature = "upload")]
	for dir in over.allowed_directories {
		if !base.allowed_directories.contains(&dir) {
			base.allowed_directories.push(dir);
		}
	}

	#[cfg(feature = "upload")]
	for over_u in over.uploaders {
		if let Some(existing) = base.uploaders.iter_mut().find(|u| u.name == over_u.name) {
			*existing = over_u;
		} else {
			base.uploaders.push(over_u);
		}
	}
}

#[cfg(feature = "upload")]
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
	allowed.push(PathBuf::from("/run"));
	allowed.push(PathBuf::from("/var/run/secrets"));

	allowed
		.into_iter()
		.filter_map(|p| p.canonicalize().ok())
		.collect()
}

#[cfg(feature = "upload")]
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

#[cfg(feature = "upload")]
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

		for field in [
			&mut u.file_form_name,
			&mut u.error_message,
			&mut u.deletion_url,
		]
		.into_iter()
		.flatten()
		{
			*field = resolve_string(&*field, &allowed_bases)?;
		}

		for vec in [&mut u.parameters, &mut u.headers, &mut u.arguments] {
			for (_, val) in vec {
				*val = resolve_string(val, &allowed_bases)?;
			}
		}
	}

	Ok(cfg)
}

pub fn save_config(cfg: &AppConfig) -> Result<()> {
	let app_name = env!("CARGO_PKG_NAME");
	let mut to_save = cfg.clone();
	let stored: AppConfig = confy::load(app_name, None)?;

	if let Some(over) = load_overrides() {
		#[cfg(feature = "upload")]
		restore_fields!(to_save, over, stored, [upload_sound], [default_uploader]);
		restore_fields!(
			to_save,
			over,
			stored,
			[],
			[
				default_action,
				default_capture,
				default_screen,
				image_format,
				image_quality,
			]
		);
		if let Some(over_silent) = over.silent {
			let stored_silent = stored.silent.unwrap_or_default();
			let to_save_silent = to_save.silent.get_or_insert_default();
			restore_fields!(
				to_save_silent,
				over_silent,
				stored_silent,
				[],
				[image, video, error]
			);
			#[cfg(feature = "upload")]
			restore_fields!(to_save_silent, over_silent, stored_silent, [], [upload]);
			if to_save_silent.is_empty() {
				to_save.silent = None;
			}
		}

		#[cfg(feature = "annotations")]
		if let Some(over_sel) = over.selection {
			restore_fields!(
				to_save.selection,
				over_sel,
				stored.selection,
				[
					background_color,
					border_color,
					border_width,
					toolbar_background_color,
					toolbar_active_color,
					toolbar_hover_color,
					annotation_color,
					annotation_line_width,
					blur_radius,
					pixelate_block_size,
					highlight_color,
					toolbar_y,
					toolbar_item_width,
					toolbar_height,
					show_toolbar,
					multi_region_mode,
				],
				[]
			);
		}
		if let Some(over_rec) = over.recording {
			restore_fields!(
				to_save.recording,
				over_rec,
				stored.recording,
				[
					encoder,
					container,
					bitrate,
					fps,
					keyframe_interval,
					tune,
					speed,
					backend,
				],
				[threads, vaapi_device]
			);
		}

		#[cfg(feature = "upload")]
		to_save.allowed_directories.retain(|d| {
			!over.allowed_directories.contains(d) || stored.allowed_directories.contains(d)
		});

		#[cfg(feature = "upload")]
		{
			let mut stored_uploaders = stored.uploaders;
			to_save.uploaders = to_save
				.uploaders
				.into_iter()
				.filter_map(|u| {
					if over.uploaders.iter().any(|over_u| over_u == &u) {
						stored_uploaders
							.iter()
							.position(|s| s.name == u.name)
							.map(|idx| stored_uploaders.remove(idx))
					} else {
						Some(u)
					}
				})
				.collect();
		}
	}

	confy::store(app_name, None, to_save)?;
	Ok(())
}
