use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::RecordingConfig;
use crate::backend::wlr::core::WlrBackend;
use crate::backend::wlr::dispatch::{CaptureState, FrameState, MultiCaptureState};
use crate::backend::wlr::shm::{
	ShmPool, ShmPoolError, WlBufferGuard, WlFrameGuard, allocate_shm_buffer, pixel_format_to_wl_shm,
};
use crate::backend::{CaptureBackend, RecordingHandle};
use crate::convert::convert_to_rgba;
use crate::error::FramrError;
use crate::output::{FrameFormat, LogicalRegion, OutputInfo};
use crate::transform::apply_transform;

impl CaptureBackend for WlrBackend {
	fn get_outputs(&self) -> Result<Vec<OutputInfo>> {
		Ok(self.outputs.clone())
	}

	fn capture_output(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
	) -> Result<RgbaImage> {
		let wl_output = self
			.wl_outputs
			.get(output.id)
			.ok_or_else(|| anyhow::anyhow!("WlOutput not found for id {}", output.id))?;

		let region_raw = region.map(|r| {
			(
				r.position.x - output.logical_position.x,
				r.position.y - output.logical_position.y,
				r.size.width as i32,
				r.size.height as i32,
			)
		});

		let (mmap, frame_format) =
			self.capture_output_raw(wl_output, region_raw, include_cursor)?;

		let mut raw: Vec<u8> = mmap.to_vec();

		convert_to_rgba(&mut raw, frame_format.format)
			.ok_or_else(|| anyhow::anyhow!("unsupported pixel format"))?;

		let width = frame_format.width as u32;
		let height = frame_format.height as u32;

		let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
			.ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;

		Ok(apply_transform(image, output.transform))
	}

	fn capture_all_outputs(&self, include_cursor: bool) -> Result<RgbaImage> {
		let outputs = &self.outputs;
		let bounds = crate::output::bounding_region(outputs)
			.ok_or::<anyhow::Error>(FramrError::NoOutputs.into())?;
		let min_x = bounds.position.x;
		let min_y = bounds.position.y;
		let total_width = bounds.size.width;
		let total_height = bounds.size.height;

		let mut state = MultiCaptureState::new(outputs.len());
		let mut event_queue = self.conn.new_event_queue::<MultiCaptureState>();
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = self
			.globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let cursor_val = if include_cursor { 1 } else { 0 };

		let frames: Vec<WlFrameGuard> = outputs
			.iter()
			.map(|output| {
				let wl_output = &self.wl_outputs[output.id];
				WlFrameGuard(screencopy_mgr.capture_output(cursor_val, wl_output, &qh, output.id))
			})
			.collect();

		while !state.all_buffer_done() {
			event_queue.blocking_dispatch(&mut state)?;
		}

		let mut buffers_files: Vec<(WlBufferGuard, std::fs::File, FrameFormat)> =
			Vec::with_capacity(outputs.len());

		let shm: WlShm = self.globals.bind(&qh, 1..=1, ())?;

		for (i, slot) in state.slots.iter().enumerate() {
			let frame_format = slot
				.formats
				.first()
				.ok_or(FramrError::NoSupportedBufferFormat)?
				.clone();

			let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
			let (buffer, file, _) = allocate_shm_buffer(
				&shm,
				&qh,
				frame_format.width,
				frame_format.height,
				frame_format.stride,
				wl_fmt,
			)?;

			let buffer = WlBufferGuard(buffer);
			frames[i].copy(&buffer);
			buffers_files.push((buffer, file, frame_format));
		}

		while !state.all_finished() {
			event_queue.blocking_dispatch(&mut state)?;
		}

		let mut composite = RgbaImage::new(total_width, total_height);

		for (i, output) in outputs.iter().enumerate() {
			if state.slots[i].frame_state == FrameState::Failed {
				return Err(FramrError::FrameCaptureFailed.into());
			}

			let (_buffer, file, frame_format) = &buffers_files[i];
			let mmap = unsafe { memmap2::Mmap::map(file)? };

			let mut raw = mmap.to_vec();

			convert_to_rgba(&mut raw, frame_format.format)
				.ok_or_else(|| anyhow::anyhow!("unsupported pixel format"))?;

			let width = frame_format.width as u32;
			let height = frame_format.height as u32;

			let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
				.ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;

			let image = apply_transform(image, output.transform);

			let x = (output.logical_position.x - min_x) as i64;
			let y = (output.logical_position.y - min_y) as i64;

			image::imageops::overlay(&mut composite, &image, x, y);
		}

		Ok(composite)
	}

	fn start_recording(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle> {
		gstreamer::init()?;

		let (stop_sender, stop_receiver) = crossbeam_channel::bounded(1);
		let (frame_sender, frame_receiver) =
			crossbeam_channel::bounded::<(std::sync::Arc<memmap2::Mmap>, usize, u64, FrameFormat)>(
				3,
			);
		let (return_sender, return_receiver) = crossbeam_channel::bounded::<usize>(3);

		let conn = self.conn.clone();
		let wl_output = self
			.wl_outputs
			.get(output.id)
			.ok_or_else(|| anyhow::anyhow!("WlOutput not found for id {}", output.id))?
			.clone();
		let output_info = output.clone();

		std::thread::spawn(move || -> Result<()> {
			WlrBackend::run_capture_loop(
				&conn,
				&wl_output,
				&output_info,
				region,
				include_cursor,
				stop_receiver,
				frame_sender,
				return_receiver,
				None,
				true,
			)
		});

		let transform = output.transform;
		let pipeline_thread = std::thread::spawn(move || -> Result<()> {
			crate::encoding::run_single_encoding_pipeline(
				transform,
				output_path,
				frame_receiver,
				return_sender,
				recording_config,
			)
		});

		Ok(RecordingHandle {
			stop_sender,
			pipeline_thread,
		})
	}

	fn start_recording_all(
		&self,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle> {
		let region = crate::output::bounding_region(&self.outputs)
			.ok_or::<anyhow::Error>(FramrError::NoOutputs.into())?;

		self.start_recording_region_internal(&region, include_cursor, output_path, recording_config)
	}

	fn start_recording_region_internal(
		&self,
		region: &LogicalRegion,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle> {
		gstreamer::init()?;

		let conn = self.conn.clone();
		let outputs = self.outputs.clone();
		let wl_outputs = self.wl_outputs.clone();
		let region = *region;

		let intersecting: Vec<_> = outputs
			.iter()
			.filter(|o| o.intersects(&region))
			.cloned()
			.collect();

		if intersecting.is_empty() {
			return Err(anyhow::anyhow!("No outputs intersect with region"));
		}

		let max_scale = intersecting
			.iter()
			.map(|o| o.scale.max(1))
			.max()
			.unwrap_or(1);

		let num_outputs = intersecting.len();
		let (stop_sender, stop_receiver) = crossbeam_channel::bounded(1);
		let frame_senders: Vec<_> = (0..num_outputs)
			.map(|_| {
				crossbeam_channel::bounded::<(std::sync::Arc<memmap2::Mmap>, usize, u64, FrameFormat)>(
					3,
				)
			})
			.collect();
		let frame_receivers: Vec<_> = frame_senders.iter().map(|s| s.1.clone()).collect();
		let (format_senders, format_receivers): (Vec<_>, Vec<_>) = (0..num_outputs)
			.map(|_| crossbeam_channel::bounded::<FrameFormat>(1))
			.unzip();
		let (return_senders, return_receivers): (Vec<_>, Vec<_>) = (0..num_outputs)
			.map(|_| crossbeam_channel::bounded::<usize>(3))
			.unzip();

		for (output_idx, output) in intersecting.iter().enumerate() {
			let conn = conn.clone();
			let wl_outputs = wl_outputs.clone();
			let stop_receiver = stop_receiver.clone();
			let frame_sender = frame_senders[output_idx].0.clone();
			let return_receiver = return_receivers[output_idx].clone();
			let format_sender = format_senders[output_idx].clone();
			let wl_output = wl_outputs[output.id].clone();
			let output_info = output.clone();

			std::thread::spawn(move || -> Result<()> {
				WlrBackend::run_capture_loop(
					&conn,
					&wl_output,
					&output_info,
					None,
					include_cursor,
					stop_receiver,
					frame_sender,
					return_receiver,
					Some(format_sender),
					false,
				)
			});
		}

		let pipeline_thread = std::thread::spawn(move || -> Result<()> {
			crate::encoding::run_composite_encoding_pipeline(
				output_path,
				region,
				max_scale,
				intersecting,
				frame_receivers,
				format_receivers,
				return_senders,
				stop_receiver,
				recording_config,
			)
		});

		Ok(RecordingHandle {
			stop_sender,
			pipeline_thread,
		})
	}
}

impl WlrBackend {
	fn run_capture_loop(
		conn: &wayland_client::Connection,
		wl_output: &wayland_client::protocol::wl_output::WlOutput,
		output_info: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
		stop_receiver: crossbeam_channel::Receiver<()>,
		frame_sender: crossbeam_channel::Sender<(
			std::sync::Arc<memmap2::Mmap>,
			usize,
			u64,
			FrameFormat,
		)>,
		return_receiver: crossbeam_channel::Receiver<usize>,
		format_sender: Option<crossbeam_channel::Sender<FrameFormat>>,
		use_relative_pts: bool,
	) -> Result<()> {
		let (globals, mut event_queue) = registry_queue_init::<CaptureState>(conn)
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
		let cursor_val = if include_cursor { 1 } else { 0 };

		let mut pool = ShmPool::new(3);
		let mut pool_format: Option<FrameFormat> = None;
		let mut first_pts = None;

		loop {
			if stop_receiver.try_recv().is_ok() {
				break;
			}

			while let Ok(idx) = return_receiver.try_recv() {
				if let Some(slot) = pool.slots.get_mut(idx) {
					slot.in_use = false;
				}
			}

			let mut state = CaptureState::default();
			let frame = WlFrameGuard(if let Some(region) = region {
				let local_x = region.position.x - output_info.logical_position.x;
				let local_y = region.position.y - output_info.logical_position.y;
				screencopy_mgr.capture_output_region(
					cursor_val,
					wl_output,
					local_x,
					local_y,
					region.size.width as i32,
					region.size.height as i32,
					&qh,
					(),
				)
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

			if let Some(ref f) = pool_format {
				if f.format != frame_format.format
					|| f.width != frame_format.width
					|| f.height != frame_format.height
				{
					return Err(FramrError::ResolutionChanged.into());
				}
			} else {
				if let Some(ref sender) = format_sender {
					if sender.send(frame_format.clone()).is_err() {
						return Ok(());
					}
				}
				pool_format = Some(frame_format.clone());
			}

			let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
			let buffer_idx = match pool.get_slot(
				&shm,
				&qh,
				frame_format.width,
				frame_format.height,
				frame_format.stride,
				wl_fmt,
			) {
				Ok(idx) => idx,
				Err(ShmPoolError::PoolFull) => match return_receiver.recv() {
					Ok(idx) => {
						if let Some(slot) = pool.slots.get_mut(idx) {
							slot.in_use = false;
						}
						idx
					}
					Err(_) => break,
				},
				Err(e) => return Err(e.into()),
			};

			let slot = &mut pool.slots[buffer_idx];
			slot.in_use = true;

			frame.copy(&slot.buffer);

			while state.frame_state == FrameState::Pending {
				event_queue.blocking_dispatch(&mut state)?;
			}

			if state.frame_state == FrameState::Failed {
				return Err(FramrError::FrameCaptureFailed.into());
			}

			let pts_nanos = (state.tv_sec_hi as u64) << 32 | (state.tv_sec_lo as u64);
			let pts = pts_nanos * 1_000_000_000 + (state.tv_nsec as u64);

			let final_pts = if use_relative_pts {
				if let Some(first) = first_pts {
					pts.saturating_sub(first)
				} else {
					first_pts = Some(pts);
					0
				}
			} else {
				pts
			};

			if frame_sender
				.send((slot.mmap.clone(), buffer_idx, final_pts, frame_format))
				.is_err()
			{
				break;
			}
		}

		Ok(())
	}
}
