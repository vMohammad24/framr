use anyhow::Result;
use wayland_client::Connection;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::wl_output::{Transform as WlTransform, WlOutput};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;

use crate::backend::wlr::dispatch::{
	OutputEnumState, PartialOutput, RegistryState, convert_transform,
};
use crate::error::FramrError;
use crate::output::{OutputInfo, Position, Size};

pub struct WlrBackend {
	pub(crate) conn: Connection,
	pub(crate) globals: GlobalList,
	pub(crate) outputs: Vec<OutputInfo>,
	pub(crate) wl_outputs: Vec<WlOutput>,
}

impl WlrBackend {
	pub fn new() -> Result<Self> {
		let conn = Connection::connect_to_env()
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
		let (globals, _) = registry_queue_init::<RegistryState>(&conn)
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;

		if !globals
			.contents()
			.clone_list()
			.iter()
			.any(|g| g.interface == "zwlr_screencopy_manager_v1")
		{
			return Err(FramrError::ProtocolNotSupported("wlr-screencopy".into()).into());
		}

		let mut this = Self {
			conn,
			globals,
			outputs: Vec::new(),
			wl_outputs: Vec::new(),
		};
		this.refresh_outputs()?;
		Ok(this)
	}

	pub(crate) fn new_without_screencopy() -> Result<Self> {
		let conn = Connection::connect_to_env()
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
		let (globals, _) = registry_queue_init::<RegistryState>(&conn)
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;

		let mut this = Self {
			conn,
			globals,
			outputs: Vec::new(),
			wl_outputs: Vec::new(),
		};
		this.refresh_outputs()?;
		Ok(this)
	}

	pub fn refresh_outputs(&mut self) -> Result<()> {
		let mut state = OutputEnumState::default();
		let mut event_queue = self.conn.new_event_queue::<OutputEnumState>();
		let qh = event_queue.handle();

		let registry = self.globals.registry();
		for g in self.globals.contents().clone_list() {
			if g.interface == "wl_output" && g.version >= 2 {
				let idx = state.outputs.len();
				let output = registry.bind::<WlOutput, _, OutputEnumState>(
					g.name,
					g.version.min(4),
					&qh,
					idx,
				);
				state.outputs.push(PartialOutput {
					wl_output: output,
					name: String::new(),
					description: String::new(),
					physical_size: Size {
						width: 0,
						height: 0,
					},
					transform: WlTransform::Normal,
					scale: 1,
					logical_position: Position { x: 0, y: 0 },
					logical_size: Size {
						width: 0,
						height: 0,
					},
				});
			}
		}

		let xdg_mgr: Option<ZxdgOutputManagerV1> = self.globals.bind(&qh, 3..=3, ()).ok();
		let xdg_outputs: Vec<_> = if let Some(ref mgr) = xdg_mgr {
			state
				.outputs
				.iter()
				.enumerate()
				.map(|(i, output)| mgr.get_xdg_output(&output.wl_output, &qh, i))
				.collect()
		} else {
			Vec::new()
		};

		event_queue.roundtrip(&mut state)?;

		for xdg in &xdg_outputs {
			xdg.destroy();
		}

		self.update_outputs(state.outputs);

		if self.outputs.is_empty() {
			return Err(FramrError::NoOutputs.into());
		}

		Ok(())
	}

	fn update_outputs(&mut self, partials: Vec<PartialOutput>) {
		self.wl_outputs = partials.iter().map(|p| p.wl_output.clone()).collect();
		self.outputs = partials
			.into_iter()
			.enumerate()
			.map(|(id, p)| OutputInfo {
				id,
				name: p.name,
				description: p.description,
				logical_position: p.logical_position,
				logical_size: p.logical_size,
				physical_size: p.physical_size,
				transform: convert_transform(p.transform),
				scale: p.scale,
			})
			.collect();
	}
}
