pub mod cli_ui;
pub mod core;
pub mod import;
pub mod macros;
pub mod types;

pub use cli_ui::*;
pub use core::{load_config, load_uploader_config};
pub use import::import_from_source;
pub(crate) use types::{AppConfig, BodyType, Color, SelectionConfig, UploadConfig};
pub use types::{DefaultAction, DefaultCaptureMethod};

use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

pub fn style_color(c: Color) -> String {
	style(c.to_string())
		.color256(get_color256(c.r, c.g, c.b))
		.to_string()
}

pub fn get_color256(r: u8, g: u8, b: u8) -> u8 {
	let r = (r as u32 * 5 / 255) as u8;
	let g = (g as u32 * 5 / 255) as u8;
	let b = (b as u32 * 5 / 255) as u8;
	16 + 36 * r + 6 * g + b
}

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

pub fn prompt_input_validated<T, F>(prompt: &str, default: Option<T>, validator: F) -> Result<T>
where
	T: std::str::FromStr + std::fmt::Display + Clone,
	T::Err: std::fmt::Display,
	F: Fn(&T) -> Result<(), String>,
{
	let theme = ColorfulTheme::default();
	let mut builder = Input::with_theme(&theme);
	builder = builder.with_prompt(prompt);
	if let Some(d) = default {
		builder = builder.default(d);
	}
	builder = builder.validate_with(validator);
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
	Ok(Input::<Color>::new()
		.with_prompt(prompt)
		.default(current)
		.interact_text()?)
}
