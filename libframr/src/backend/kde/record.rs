use crate::RecordingConfig;
use crate::backend::RecordingHandle;
use crate::output::LogicalRegion;
use anyhow::{Result, anyhow};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_stream_unstable_v1::{
	Event as StreamEvent, ZkdeScreencastStreamUnstableV1,
};
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1;

pub(crate) enum StreamTarget<'a> {
	Output(&'a WlOutput),
	Region { region: LogicalRegion, scale: f64 },
}

#[derive(Default)]
struct StreamState {
	node: Option<u32>,
	failed: Option<String>,
}

impl Dispatch<WlRegistry, GlobalListContents> for StreamState {
	fn event(
		_: &mut Self,
		_: &WlRegistry,
		_: <WlRegistry as Proxy>::Event,
		_: &GlobalListContents,
		_: &Connection,
		_: &QueueHandle<Self>,
	) {
	}
}

impl Dispatch<ZkdeScreencastUnstableV1, ()> for StreamState {
	fn event(
		_: &mut Self,
		_: &ZkdeScreencastUnstableV1,
		_: <ZkdeScreencastUnstableV1 as Proxy>::Event,
		_: &(),
		_: &Connection,
		_: &QueueHandle<Self>,
	) {
	}
}

impl Dispatch<ZkdeScreencastStreamUnstableV1, ()> for StreamState {
	fn event(
		state: &mut Self,
		_: &ZkdeScreencastStreamUnstableV1,
		event: StreamEvent,
		_: &(),
		_: &Connection,
		_: &QueueHandle<Self>,
	) {
		match event {
			StreamEvent::Created { node } => state.node = Some(node),
			StreamEvent::Failed { error } => state.failed = Some(error),
			StreamEvent::Closed if state.node.is_none() => {
				state.failed = Some("stream closed by compositor".to_string());
			}
			_ => {}
		}
	}
}

pub(crate) fn start_zkde_recording(
	conn: &Connection,
	target: StreamTarget,
	include_cursor: bool,
	output_path: std::path::PathBuf,
	recording_config: RecordingConfig,
) -> Result<RecordingHandle> {
	let (globals, mut queue) = registry_queue_init::<StreamState>(conn)
		.map_err(|e| anyhow!("Failed to initialize Wayland registry: {}", e))?;
	let qh = queue.handle();

	let manager: ZkdeScreencastUnstableV1 = globals.bind(&qh, 1..=4, ()).map_err(|_| {
		anyhow!(
			"KWin screencast protocol not available; run `framr config protocol` and re-login to authorize framr"
		)
	})?;

	let pointer = if include_cursor { 2u32 } else { 1u32 };

	let stream = match target {
		StreamTarget::Output(wl_output) => manager.stream_output(wl_output, pointer, &qh, ()),
		StreamTarget::Region { region, scale } => {
			if manager.version() < 3 {
				manager.destroy();
				return Err(anyhow!(
					"KWin screencast protocol too old for region capture"
				));
			}
			manager.stream_region(
				region.position.x,
				region.position.y,
				region.size.width,
				region.size.height,
				scale,
				pointer,
				&qh,
				(),
			)
		}
	};

	let mut state = StreamState::default();
	while state.node.is_none() && state.failed.is_none() {
		queue.blocking_dispatch(&mut state)?;
	}
	manager.destroy();

	let Some(node_id) = state.node else {
		stream.close();
		let _ = conn.flush();
		return Err(anyhow!(
			"KWin screencast failed: {}",
			state.failed.unwrap_or_else(|| "unknown error".to_string())
		));
	};

	gstreamer::init()?;

	let (stop_sender, stop_receiver) = crossbeam_channel::bounded(1);
	let conn = conn.clone();
	let pipeline_thread = std::thread::spawn(move || -> Result<()> {
		let result = crate::encoding::run_pipewire_encoding_pipeline(
			node_id,
			output_path,
			stop_receiver,
			recording_config,
		);
		stream.close();
		let _ = conn.flush();
		result
	});

	Ok(RecordingHandle {
		stop_sender,
		pipeline_thread,
	})
}
