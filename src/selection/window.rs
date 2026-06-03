use anyhow::{Context, Result};
use hyprland::{
	data::{Clients, Monitors},
	shared::HyprData,
};

#[derive(Clone, Debug)]
pub struct Window {
	pub title: String,
	pub width: i32,
	pub height: i32,
	pub x: i32,
	pub y: i32,
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
	windows.iter().position(|win| {
		let left = win.x as f64;
		let right = left + win.width as f64;
		let top = win.y as f64;
		let bottom = top + win.height as f64;

		pos.0 >= left && pos.0 <= right && pos.1 >= top && pos.1 <= bottom
	})
}

pub fn get_hypr_windows() -> Result<Vec<Window>> {
	let monitors = Monitors::get()?;
	let clients = Clients::get()?;

	let windows = clients
		.into_iter()
		.filter(|c| {
			monitors
				.iter()
				.any(|m| m.active_workspace.id == c.workspace.id)
		})
		.map(|c| Window {
			title: c.title,
			x: c.at.0 as i32,
			y: c.at.1 as i32,
			width: c.size.0 as i32,
			height: c.size.1 as i32,
		})
		.collect();

	Ok(windows)
}

// TODO: implement this
pub fn get_kde_windows() -> Result<Vec<Window>> {
	Ok(vec![])
}
