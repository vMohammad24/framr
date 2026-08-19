use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use swayipc::{Connection, Node, NodeType};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection as WaylandConnection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_virtual_desktop::client::{
	org_kde_plasma_virtual_desktop::{Event as VirtualDesktopEvent, OrgKdePlasmaVirtualDesktop},
	org_kde_plasma_virtual_desktop_management::{
		Event as VirtualDesktopManagementEvent, OrgKdePlasmaVirtualDesktopManagement,
	},
};
use wayland_protocols_plasma::plasma_window_management::client::{
	org_kde_plasma_stacking_order::{Event as StackingOrderEvent, OrgKdePlasmaStackingOrder},
	org_kde_plasma_window::{Event as PlasmaWindowEvent, OrgKdePlasmaWindow},
	org_kde_plasma_window_management::{OrgKdePlasmaWindowManagement, State as PlasmaWindowState},
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
		"KDE" => get_kde_windows().context("Error fetching KDE windows"),
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
	#[derive(Deserialize)]
	struct HyprWorkspaceRef {
		id: i32,
	}

	#[derive(Deserialize)]
	struct HyprClient {
		title: String,
		at: (i32, i32),
		size: (i32, i32),
		workspace: HyprWorkspaceRef,
		floating: bool,
		fullscreen: u8,
		#[serde(rename = "overFullscreen", alias = "allowedOverFullscreen", default)]
		over_fullscreen: bool,
		visible: bool,
		#[serde(rename = "focusHistoryID")]
		focus_history_id: i32,
	}

	#[derive(Deserialize)]
	struct HyprMonitor {
		#[serde(rename = "activeWorkspace")]
		active_workspace: HyprWorkspaceRef,
	}

	fn hypr_query(command: &str) -> Result<String> {
		let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
			.context("XDG_RUNTIME_DIR environment variable not set")?;
		let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
			.context("HYPRLAND_INSTANCE_SIGNATURE environment variable not set")?;
		let mut stream =
			UnixStream::connect(format!("{runtime_dir}/hypr/{signature}/.socket.sock"))
				.context("Failed to connect to Hyprland IPC socket")?;
		stream.write_all(command.as_bytes())?;
		let mut response = String::new();
		stream.read_to_string(&mut response)?;
		Ok(response)
	}

	let monitors: Vec<HyprMonitor> = serde_json::from_str(&hypr_query("j/monitors")?)
		.context("Failed to parse Hyprland monitors JSON")?;
	let clients: Vec<HyprClient> = serde_json::from_str(&hypr_query("j/clients")?)
		.context("Failed to parse Hyprland clients JSON")?;

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
			} else if c.fullscreen != 0 {
				2000
			} else {
				1000
			};

			Window {
				title: c.title,
				x: c.at.0,
				y: c.at.1,
				width: c.size.0,
				height: c.size.1,
				z_index: layer_base - c.focus_history_id,
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

#[derive(Default)]
struct KdeWindowQuery {
	windows: HashMap<String, PartialKdeWindow>,
	stacking_order: Vec<String>,
	active_desktops: HashSet<String>,
}

#[derive(Default)]
struct PartialKdeWindow {
	title: String,
	geometry: Option<(i32, i32, u32, u32)>,
	state: u32,
	virtual_desktops: HashSet<String>,
	unmapped: bool,
}

impl Dispatch<WlRegistry, GlobalListContents> for KdeWindowQuery {
	fn event(
		_: &mut Self,
		_: &WlRegistry,
		_: <WlRegistry as Proxy>::Event,
		_: &GlobalListContents,
		_: &WaylandConnection,
		_: &QueueHandle<Self>,
	) {
	}
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for KdeWindowQuery {
	fn event(
		_: &mut Self,
		_: &OrgKdePlasmaWindowManagement,
		_: <OrgKdePlasmaWindowManagement as Proxy>::Event,
		_: &(),
		_: &WaylandConnection,
		_: &QueueHandle<Self>,
	) {
	}
}

impl Dispatch<OrgKdePlasmaWindow, String> for KdeWindowQuery {
	fn event(
		state: &mut Self,
		_: &OrgKdePlasmaWindow,
		event: PlasmaWindowEvent,
		uuid: &String,
		_: &WaylandConnection,
		_: &QueueHandle<Self>,
	) {
		let window = state.windows.entry(uuid.clone()).or_default();
		match event {
			PlasmaWindowEvent::TitleChanged { title } => window.title = title,
			PlasmaWindowEvent::StateChanged { flags } => window.state = flags,
			PlasmaWindowEvent::Geometry {
				x,
				y,
				width,
				height,
			} => window.geometry = Some((x, y, width, height)),
			PlasmaWindowEvent::VirtualDesktopEntered { id } => {
				window.virtual_desktops.insert(id);
			}
			PlasmaWindowEvent::VirtualDesktopLeft { is: id } => {
				window.virtual_desktops.remove(&id);
			}
			PlasmaWindowEvent::Unmapped => window.unmapped = true,
			_ => {}
		}
	}
}

impl Dispatch<OrgKdePlasmaStackingOrder, ()> for KdeWindowQuery {
	fn event(
		state: &mut Self,
		_: &OrgKdePlasmaStackingOrder,
		event: StackingOrderEvent,
		_: &(),
		_: &WaylandConnection,
		_: &QueueHandle<Self>,
	) {
		if let StackingOrderEvent::Window { uuid } = event {
			state.stacking_order.push(uuid);
		}
	}
}

impl Dispatch<OrgKdePlasmaVirtualDesktopManagement, ()> for KdeWindowQuery {
	fn event(
		_: &mut Self,
		manager: &OrgKdePlasmaVirtualDesktopManagement,
		event: VirtualDesktopManagementEvent,
		_: &(),
		_: &WaylandConnection,
		qh: &QueueHandle<Self>,
	) {
		if let VirtualDesktopManagementEvent::DesktopCreated { desktop_id, .. } = event {
			manager.get_virtual_desktop(desktop_id.clone(), qh, desktop_id);
		}
	}
}

impl Dispatch<OrgKdePlasmaVirtualDesktop, String> for KdeWindowQuery {
	fn event(
		state: &mut Self,
		_: &OrgKdePlasmaVirtualDesktop,
		event: VirtualDesktopEvent,
		desktop_id: &String,
		_: &WaylandConnection,
		_: &QueueHandle<Self>,
	) {
		match event {
			VirtualDesktopEvent::Activated => {
				state.active_desktops.insert(desktop_id.clone());
			}
			VirtualDesktopEvent::Deactivated | VirtualDesktopEvent::Removed => {
				state.active_desktops.remove(desktop_id);
			}
			_ => {}
		}
	}
}

pub fn get_kde_windows() -> Result<Vec<Window>> {
	let connection = WaylandConnection::connect_to_env()
		.context("Failed to connect to the Wayland compositor")?;
	let (globals, mut queue) = registry_queue_init::<KdeWindowQuery>(&connection)
		.context("Failed to initialize the Wayland registry")?;
	let qh = queue.handle();

	let window_manager: OrgKdePlasmaWindowManagement = globals
		.bind(&qh, 17..=18, ())
		.context("KDE Plasma window management protocol is unavailable")?;
	let _desktop_manager: Option<OrgKdePlasmaVirtualDesktopManagement> =
		globals.bind(&qh, 1..=2, ()).ok();

	window_manager.get_stacking_order(&qh, ());

	let mut state = KdeWindowQuery::default();
	queue.roundtrip(&mut state)?;
	for uuid in &state.stacking_order {
		state.windows.entry(uuid.clone()).or_default();
		window_manager.get_window_by_uuid(uuid.clone(), &qh, uuid.clone());
	}
	queue.roundtrip(&mut state)?;

	let minimized = PlasmaWindowState::Minimized as u32;
	let skip_taskbar = PlasmaWindowState::Skiptaskbar as u32;
	let windows = state
		.stacking_order
		.into_iter()
		.enumerate()
		.filter_map(|(z_index, uuid)| {
			let window = state.windows.remove(&uuid)?;
			let (x, y, width, height) = window.geometry?;
			let on_active_desktop = state.active_desktops.is_empty()
				|| window.virtual_desktops.is_empty()
				|| window
					.virtual_desktops
					.iter()
					.any(|id| state.active_desktops.contains(id));

			if window.unmapped
				|| window.state & (minimized | skip_taskbar) != 0
				|| !on_active_desktop
			{
				return None;
			}

			Some(Window {
				title: window.title,
				x,
				y,
				width: i32::try_from(width).ok()?,
				height: i32::try_from(height).ok()?,
				z_index: i32::try_from(z_index).unwrap_or(i32::MAX),
			})
		})
		.collect();

	Ok(windows)
}
