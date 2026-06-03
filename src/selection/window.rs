use std::collections::HashMap;

use anyhow::{Context, Result};
use hyprland::{
	data::{Clients, Monitors},
	shared::HyprData,
};
use niri_ipc::{Request, Response, socket::Socket};

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
		"niri" => get_niri_windows(),
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

pub fn get_niri_windows() -> Result<Vec<Window>> {
	let mut socket = Socket::connect()?;

	let outputs = match socket.send(Request::Outputs)? {
		Ok(Response::Outputs(outputs)) => outputs,
		_ => return Ok(vec![]),
	};

	let workspaces = match socket.send(Request::Workspaces)? {
		Ok(Response::Workspaces(workspaces)) => workspaces,
		_ => return Ok(vec![]),
	};

	let clients = match socket.send(Request::Windows)? {
		Ok(Response::Windows(wins)) => wins,
		_ => return Ok(vec![]),
	};

	let output_positions: HashMap<&str, (i32, i32)> = outputs
		.iter()
		.filter_map(|(name, output)| {
			output
				.logical
				.map(|logical| (name.as_str(), (logical.x, logical.y)))
		})
		.collect();

	let active_ws_positions: HashMap<u64, (i32, i32)> = workspaces
		.iter()
		.filter(|ws| ws.is_active)
		.filter_map(|ws| {
			ws.output
				.as_ref()
				.and_then(|out_name| output_positions.get(out_name.as_str()))
				.map(|&pos| (ws.id, pos))
		})
		.collect();

	let windows = clients
		.into_iter()
		.filter(|c| {
			c.workspace_id
				.map(|id| active_ws_positions.contains_key(&id))
				.unwrap_or(false)
		})
		.map(|c| {
			let (rel_x, rel_y) = c.layout.tile_pos_in_workspace_view.unwrap_or((0.0, 0.0));
			let (offset_x, offset_y) = c.layout.window_offset_in_tile;

			let (out_x, out_y) = c
				.workspace_id
				.and_then(|ws_id| active_ws_positions.get(&ws_id).copied())
				.unwrap_or((0, 0));

			let x = (rel_x + offset_x).round() as i32 + out_x;
			let y = (rel_y + offset_y).round() as i32 + out_y;

			let (width, height) = c.layout.window_size;

			Window {
				title: c.title.unwrap_or_default(),
				x,
				y,
				width,
				height,
			}
		})
		.collect();

	Ok(windows)
}
