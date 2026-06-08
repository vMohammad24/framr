use anyhow::{Context, Result};
use hyprland::{
	data::{Clients, FullscreenMode, Monitors},
	shared::HyprData,
};
use serde::Deserialize;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use swayipc::{Connection, Node, NodeType};

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
		"sway" => get_sway_windows().context("Error fetching Sway windows"),
		"mango" => get_mango_windows().context("Error fetching Mango windows"),
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

pub fn get_sway_windows() -> Result<Vec<Window>> {
	let mut connection = Connection::new().context("Failed to connect to Sway IPC")?;
	let tree = connection.get_tree().context("Failed to fetch Sway tree")?;

	let mut windows = Vec::new();
	let mut focus_counter = 0;

	fn traverse(node: &Node, windows: &mut Vec<Window>, focus_counter: &mut i32) {
		let is_window = node.app_id.is_some() || node.window.is_some();

		if node.visible.unwrap_or(false) && is_window {
			let is_fullscreen = matches!(node.fullscreen_mode, Some(1) | Some(2));
			let is_floating = node.node_type == NodeType::FloatingCon;

			let layer_base = if is_fullscreen {
				3000
			} else if is_floating {
				2500
			} else {
				1000
			};

			windows.push(Window {
				title: node.name.clone().unwrap_or_default(),
				x: node.rect.x,
				y: node.rect.y,
				width: node.rect.width,
				height: node.rect.height,
				z_index: layer_base - *focus_counter,
			});

			*focus_counter += 1;
		}

		for child in &node.nodes {
			traverse(child, windows, focus_counter);
		}

		for floating_child in &node.floating_nodes {
			traverse(floating_child, windows, focus_counter);
		}
	}

	traverse(&tree, &mut windows, &mut focus_counter);

	Ok(windows)
}

pub fn get_mango_windows() -> Result<Vec<Window>> {
	#[derive(Deserialize, Debug)]
	struct MangoClient {
		title: String,
		x: i32,
		y: i32,
		width: i32,
		height: i32,
		is_fullscreen: bool,
		is_floating: bool,
		is_focused: bool,
	}

	#[derive(Deserialize, Debug)]
	struct MangoResponse {
		clients: Vec<MangoClient>,
	}

	let socket_path = std::env::var("MANGO_INSTANCE_SIGNATURE")
		.context("MANGO_INSTANCE_SIGNATURE environment variable not set")?;

	let mut stream =
		UnixStream::connect(socket_path).context("Failed to connect to Mango IPC Unix socket")?;

	stream
		.write_all(b"get all-clients\n")
		.context("Failed to write to Mango IPC socket")?;

	let mut response = String::new();
	stream
		.read_to_string(&mut response)
		.context("Failed to read from Mango IPC socket")?;

	let parsed: MangoResponse =
		serde_json::from_str(&response).context("Failed to parse Mango clients JSON")?;

	let windows = parsed
		.clients
		.into_iter()
		.enumerate()
		.map(|(index, c)| {
			let layer_base = if c.is_fullscreen {
				3000
			} else if c.is_floating {
				2500
			} else {
				1000
			};

			let focus_boost = if c.is_focused { 50 } else { 0 };

			Window {
				title: c.title,
				x: c.x,
				y: c.y,
				width: c.width,
				height: c.height,
				z_index: layer_base + focus_boost - index as i32,
			}
		})
		.collect();

	Ok(windows)
}
// TODO: implement this
pub fn get_kde_windows() -> Result<Vec<Window>> {
	Ok(vec![])
}
