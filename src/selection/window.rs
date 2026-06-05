use anyhow::{Context, Result};
use hyprland::{
	data::{Clients, FullscreenMode, Monitors},
	shared::HyprData,
};

#[derive(Clone, Debug)]
pub struct Window {
	pub title: String,
	pub width: i32,
	pub height: i32,
	pub x: i32,
	pub y: i32,
	pub z_index: i32,
}

pub fn get_windows() -> Result<Vec<Window>> {
	let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Unknown".to_string());

	match desktop.as_str() {
		"Hyprland" => get_hypr_windows().context("Error fetching Hyprland windows"),
		"KDE" => get_kde_windows(),
		_ => Ok(vec![]),
	}
}

pub fn get_window_at_pos(pos: (f64, f64), windows: &[Window]) -> Option<usize> {
	let (px, py) = (pos.0 as i32, pos.1 as i32);

	windows
		.iter()
		.enumerate()
		.filter(|(_, win)| {
			let right = win.x + win.width;
			let bottom = win.y + win.height;

			px >= win.x && px <= right && py >= win.y && py <= bottom
		})
		.max_by_key(|(_, win)| win.z_index)
		.map(|(index, _)| index)
}

pub fn get_hypr_windows() -> Result<Vec<Window>> {
	let monitors = Monitors::get()?;
	let clients = Clients::get()?;

	let windows = clients
		.into_iter()
		.filter(|c| {
			monitors
				.iter()
				.any(|m| m.active_workspace.id == c.workspace.id && c.visible)
		})
		.map(|c| {
			let layer_base: i32 = if c.over_fullscreen {
				3000
			} else if c.floating {
				2500
			} else if c.fullscreen != FullscreenMode::None {
				2000
			} else {
				1000
			};

			Window {
				title: c.title,
				x: c.at.0 as i32,
				y: c.at.1 as i32,
				width: c.size.0 as i32,
				height: c.size.1 as i32,
				z_index: layer_base - c.focus_history_id as i32,
			}
		})
		.collect();

	Ok(windows)
}

// TODO: implement this
pub fn get_kde_windows() -> Result<Vec<Window>> {
	Ok(vec![])
}
