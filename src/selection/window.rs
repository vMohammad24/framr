use anyhow::{Context, Result};
use hyprland::{
	data::{Clients, Monitors},
	shared::HyprData,
};

#[derive(Clone, Debug)]
pub struct Window {
	pub title: String,
	pub width: i16,
	pub height: i16,
	pub x: i16,
	pub y: i16,
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
	for (idx, win) in windows.iter().enumerate() {
		let left = win.x as f64;
		let right = win.x as f64 + win.width as f64;
		let top = win.y as f64;
		let bottom = win.y as f64 + win.height as f64;

		if pos.0 >= left && pos.0 <= right && pos.1 >= top && pos.1 <= bottom {
			return Some(idx);
		}
	}
	None
}

pub fn get_hypr_windows() -> Result<Vec<Window>> {
	let monitors = Monitors::get()?;

	let active_workspaces: Vec<i32> = monitors.iter().map(|m| m.active_workspace.id).collect();

	let clients = Clients::get()?;

	let windows = clients
		.iter()
		.filter(|c| active_workspaces.contains(&c.workspace.id))
		.map(|c| Window {
			title: c.title.clone(),
			x: c.at.0,
			y: c.at.1,
			width: c.size.0,
			height: c.size.1,
		})
		.collect();

	Ok(windows)
}

// TODO: implement this
pub fn get_kde_windows() -> Result<Vec<Window>> {
	Ok(vec![])
}
