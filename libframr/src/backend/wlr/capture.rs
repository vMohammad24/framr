use anyhow::Result;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::backend::wlr::core::WlrBackend;
use crate::backend::wlr::dispatch::{CaptureState, FrameState};
use crate::backend::wlr::shm::{
	WlBufferGuard, WlFrameGuard, allocate_shm_buffer, pixel_format_to_wl_shm,
};
use crate::error::FramrError;
use crate::output::FrameFormat;

impl WlrBackend {
	pub(crate) fn capture_output_raw(
		&self,
		wl_output: &WlOutput,
		region: Option<(i32, i32, i32, i32)>,
		include_cursor: bool,
	) -> Result<(memmap2::Mmap, FrameFormat)> {
		let mut state = CaptureState::default();
		let mut event_queue = self.conn.new_event_queue::<CaptureState>();
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = self
			.globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let cursor_val = if include_cursor { 1 } else { 0 };

		let frame = WlFrameGuard(if let Some((x, y, w, h)) = region {
			screencopy_mgr.capture_output_region(cursor_val, wl_output, x, y, w, h, &qh, ())
		} else {
			screencopy_mgr.capture_output(cursor_val, wl_output, &qh, ())
		});

		while !state.buffer_done {
			event_queue.blocking_dispatch(&mut state)?;
		}

		let frame_format = state
			.formats
			.first()
			.ok_or(FramrError::NoSupportedBufferFormat)?
			.clone();

		let shm: WlShm = self.globals.bind(&qh, 1..=1, ())?;
		let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
		let (buffer, _file, mmap) = allocate_shm_buffer(
			&shm,
			&qh,
			frame_format.width,
			frame_format.height,
			frame_format.stride,
			wl_fmt,
		)?;

		let buffer = WlBufferGuard(buffer);
		frame.copy(&buffer);

		while state.frame_state == FrameState::Pending {
			event_queue.blocking_dispatch(&mut state)?;
		}

		if state.frame_state == FrameState::Failed {
			return Err(FramrError::FrameCaptureFailed.into());
		}

		Ok((mmap, frame_format))
	}
}
