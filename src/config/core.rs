use crate::config::types::AppConfig;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_config() -> Result<AppConfig> {
	let app_name = env!("CARGO_PKG_NAME");
	let mut cfg: AppConfig = confy::load(app_name, None)?;

	if let Some(over) = load_overrides() {
		merge_configs(&mut cfg, over);
	}

	Ok(cfg)
}

pub fn load_overrides() -> Option<AppConfig> {
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

pub fn save_config(cfg: &AppConfig) -> Result<()> {
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
