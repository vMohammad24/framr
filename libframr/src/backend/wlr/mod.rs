use std::ops::Deref;
use std::os::fd::AsFd;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use wayland_client::Connection;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::backend::{CaptureBackend, RecordingHandle};
use crate::buffer::create_shm_fd;
use crate::convert::convert_to_rgba;
use crate::error::FramrError;
use crate::output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Position, Size};
use crate::transform::apply_transform;

mod dispatch;
use dispatch::*;

fn pixel_format_to_wl_shm(fmt: PixelFormat) -> wayland_client::protocol::wl_shm::Format {
	use wayland_client::protocol::wl_shm::Format as WlFormat;
	match fmt {
		PixelFormat::Argb8888 => WlFormat::Argb8888,
		PixelFormat::Xrgb8888 => WlFormat::Xrgb8888,
		PixelFormat::Abgr8888 => WlFormat::Abgr8888,
		PixelFormat::Xbgr8888 => WlFormat::Xbgr8888,
		PixelFormat::Abgr2101010 => WlFormat::Abgr2101010,
		PixelFormat::Xbgr2101010 => WlFormat::Xbgr2101010,
	}
}

fn allocate_shm_buffer<T>(
	shm: &WlShm,
	qh: &wayland_client::QueueHandle<T>,
	width: i32,
	height: i32,
	stride: i32,
	wl_fmt: wayland_client::protocol::wl_shm::Format,
) -> Result<(WlBuffer, std::fs::File, memmap2::Mmap)>
where
	T: wayland_client::Dispatch<wayland_client::protocol::wl_shm_pool::WlShmPool, ()>
		+ wayland_client::Dispatch<WlBuffer, ()>
		+ 'static,
{
	let byte_size = stride * height;
	let fd = create_shm_fd()?;
	let file = std::fs::File::from(fd);
	file.set_len(byte_size as u64)?;

	let pool = shm.create_pool(file.as_fd(), byte_size, qh, ());
	let buffer = pool.create_buffer(0, width, height, stride, wl_fmt, qh, ());
	pool.destroy();

	let mmap = unsafe { memmap2::Mmap::map(&file)? };
	Ok((buffer, file, mmap))
}

pub struct WlrBackend {
	pub(crate) conn: Connection,
	pub(crate) globals: GlobalList,
	outputs: Vec<OutputInfo>,
	wl_outputs: Vec<WlOutput>,
}

struct WlBufferGuard(WlBuffer);
impl Drop for WlBufferGuard {
	fn drop(&mut self) {
		self.0.destroy();
	}
}
impl Deref for WlBufferGuard {
	type Target = WlBuffer;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

struct WlFrameGuard(ZwlrScreencopyFrameV1);
impl Drop for WlFrameGuard {
	fn drop(&mut self) {
		self.0.destroy();
	}
}
impl Deref for WlFrameGuard {
	type Target = ZwlrScreencopyFrameV1;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
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

	fn refresh_outputs(&mut self) -> Result<()> {
		let mut state = OutputEnumState::default();
		let mut event_queue = self.conn.new_event_queue::<OutputEnumState>();
		let qh = event_queue.handle();

		let _ = self.conn.display().get_registry(&qh, ());
		event_queue.roundtrip(&mut state)?;

		let Ok(xdg_mgr): Result<ZxdgOutputManagerV1, _> = self.globals.bind(&qh, 3..=3, ()) else {
			self.update_outputs(state.outputs);
			return Ok(());
		};

		let xdg_outputs: Vec<_> = state
			.outputs
			.iter()
			.enumerate()
			.map(|(i, output)| xdg_mgr.get_xdg_output(&output.wl_output, &qh, i))
			.collect();
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

		let mut state = CaptureState::default();
		let mut event_queue = self.conn.new_event_queue::<CaptureState>();
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = self
			.globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let cursor_val = if include_cursor { 1 } else { 0 };

		let frame = WlFrameGuard(if let Some(region) = region {
			let local_x = region.position.x - output.logical_position.x;
			let local_y = region.position.y - output.logical_position.y;
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
			screencopy_mgr.capture_output_region(
				cursor_val,
				wl_output,
				0,
				0,
				output.logical_size.width as i32,
				output.logical_size.height as i32,
				&qh,
				(),
			)
		});

		while !state.buffer_done {
			event_queue.blocking_dispatch(&mut state)?;
		}

		let frame_format = state
			.formats
			.first()
			.ok_or(FramrError::NoSupportedBufferFormat)?
			.clone();

		let byte_size = frame_format.byte_size();
		let fd = create_shm_fd()?;

		let file = std::fs::File::from(fd);
		file.set_len(byte_size as u64)?;

		let shm: WlShm = self.globals.bind(&qh, 1..=1, ())?;
		let pool = shm.create_pool(file.as_fd(), byte_size as i32, &qh, ());

		let wl_fmt = pixel_format_to_wl_shm(frame_format.format);

		let buffer = WlBufferGuard(pool.create_buffer(
			0,
			frame_format.width,
			frame_format.height,
			frame_format.stride,
			wl_fmt,
			&qh,
			(),
		));
		pool.destroy();

		frame.copy(&buffer);

		while state.frame_state == FrameState::Pending {
			event_queue.blocking_dispatch(&mut state)?;
		}

		if state.frame_state == FrameState::Failed {
			return Err(FramrError::FrameCaptureFailed.into());
		}

		let mmap = unsafe { memmap2::Mmap::map(&file)? };
		let mut raw = mmap.to_vec();

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
		if outputs.is_empty() {
			return Err(FramrError::NoOutputs.into());
		}

		let min_x = outputs
			.iter()
			.map(|o| o.logical_position.x)
			.min()
			.unwrap_or(0);
		let min_y = outputs
			.iter()
			.map(|o| o.logical_position.y)
			.min()
			.unwrap_or(0);
		let max_x = outputs
			.iter()
			.map(|o| o.logical_position.x + o.logical_size.width as i32)
			.max()
			.unwrap_or(0);
		let max_y = outputs
			.iter()
			.map(|o| o.logical_position.y + o.logical_size.height as i32)
			.max()
			.unwrap_or(0);

		let total_width = (max_x - min_x) as u32;
		let total_height = (max_y - min_y) as u32;

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

			let x = (output.logical_position.x - min_x) as u64;
			let y = (output.logical_position.y - min_y) as u64;

			for (px, py, pixel) in image.enumerate_pixels() {
				let dx = px as u64 + x;
				let dy = py as u64 + y;
				if dx < total_width as u64 && dy < total_height as u64 {
					composite.put_pixel(dx as u32, dy as u32, *pixel);
				}
			}
		}

		Ok(composite)
	}

	fn start_recording(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
		output_path: std::path::PathBuf,
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
			let (globals, mut event_queue) = registry_queue_init::<CaptureState>(&conn)
				.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
			let qh = event_queue.handle();

			let screencopy_mgr: ZwlrScreencopyManagerV1 = globals
				.bind(&qh, 3..=3, ())
				.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

			let shm: WlShm = globals.bind(&qh, 1..=1, ())?;

			let cursor_val = if include_cursor { 1 } else { 0 };

			let mut pool: Vec<(WlBuffer, std::fs::File, std::sync::Arc<memmap2::Mmap>, bool)> =
				Vec::new();
			let mut pool_format: Option<FrameFormat> = None;

			let mut first_pts = None;

			loop {
				if stop_receiver.try_recv().is_ok() {
					break;
				}

				while let Ok(idx) = return_receiver.try_recv() {
					if let Some(slot) = pool.get_mut(idx) {
						slot.3 = false;
					}
				}

				let mut state = CaptureState::default();
				let frame = WlFrameGuard(if let Some(region) = region {
					let local_x = region.position.x - output_info.logical_position.x;
					let local_y = region.position.y - output_info.logical_position.y;
					screencopy_mgr.capture_output_region(
						cursor_val,
						&wl_output,
						local_x,
						local_y,
						region.size.width as i32,
						region.size.height as i32,
						&qh,
						(),
					)
				} else {
					screencopy_mgr.capture_output_region(
						cursor_val,
						&wl_output,
						0,
						0,
						output_info.logical_size.width as i32,
						output_info.logical_size.height as i32,
						&qh,
						(),
					)
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
				}

				if pool_format.is_none() {
					pool_format = Some(frame_format.clone());
				}

				let buffer_idx = if let Some(idx) = pool.iter().position(|slot| !slot.3) {
					idx
				} else if pool.len() < 3 {
					let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
					let (buffer, file, mmap) = allocate_shm_buffer(
						&shm,
						&qh,
						frame_format.width,
						frame_format.height,
						frame_format.stride,
						wl_fmt,
					)?;
					let mmap = std::sync::Arc::new(mmap);
					pool.push((buffer, file, mmap, false));
					pool.len() - 1
				} else {
					match return_receiver.recv() {
						Ok(idx) => {
							if let Some(slot) = pool.get_mut(idx) {
								slot.3 = false;
							}
							idx
						}
						Err(_) => break,
					}
				};

				let (wl_buffer, _, mmap, in_use) = &mut pool[buffer_idx];
				*in_use = true;

				frame.copy(wl_buffer);

				while state.frame_state == FrameState::Pending {
					event_queue.blocking_dispatch(&mut state)?;
				}

				if state.frame_state == FrameState::Failed {
					return Err(FramrError::FrameCaptureFailed.into());
				}

				let pts_nanos = (state.tv_sec_hi as u64) << 32 | (state.tv_sec_lo as u64);
				let pts = pts_nanos * 1_000_000_000 + (state.tv_nsec as u64);

				let relative_pts = if let Some(first) = first_pts {
					pts.saturating_sub(first)
				} else {
					first_pts = Some(pts);
					0
				};

				if frame_sender
					.send((mmap.clone(), buffer_idx, relative_pts, frame_format))
					.is_err()
				{
					break;
				}
			}
			for (buffer, _, _, _) in pool {
				buffer.destroy();
			}

			Ok(())
		});

		let transform = output.transform;
		let pipeline_thread = std::thread::spawn(move || -> Result<()> {
			crate::encoding::run_single_encoding_pipeline(
				transform,
				output_path,
				frame_receiver,
				return_sender,
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
	) -> Result<RecordingHandle> {
		let outputs = self.outputs.clone();
		if outputs.is_empty() {
			return Err(FramrError::NoOutputs.into());
		}

		let min_x = outputs.iter().map(|o| o.logical_position.x).min().unwrap();
		let min_y = outputs.iter().map(|o| o.logical_position.y).min().unwrap();
		let max_x = outputs
			.iter()
			.map(|o| o.logical_position.x + o.logical_size.width as i32)
			.max()
			.unwrap();
		let max_y = outputs
			.iter()
			.map(|o| o.logical_position.y + o.logical_size.height as i32)
			.max()
			.unwrap();

		let region = LogicalRegion {
			position: Position { x: min_x, y: min_y },
			size: Size {
				width: (max_x - min_x) as u32,
				height: (max_y - min_y) as u32,
			},
		};

		self.start_recording_region_internal(&region, include_cursor, output_path)
	}

	fn start_recording_region_internal(
		&self,
		region: &LogicalRegion,
		include_cursor: bool,
		output_path: std::path::PathBuf,
	) -> Result<RecordingHandle> {
		gstreamer::init()?;

		let conn = self.conn.clone();
		let outputs = self.outputs.clone();
		let wl_outputs = self.wl_outputs.clone();
		let region = *region;

		let mut intersecting = Vec::new();
		for (i, output) in outputs.iter().enumerate() {
			let ox = output.logical_position.x;
			let oy = output.logical_position.y;
			let ow = output.logical_size.width as i32;
			let oh = output.logical_size.height as i32;

			let rx = region.position.x;
			let ry = region.position.y;
			let rw = region.size.width as i32;
			let rh = region.size.height as i32;

			if rx < ox + ow && rx + rw > ox && ry < oy + oh && ry + rh > oy {
				intersecting.push((i, output.clone()));
			}
		}

		if intersecting.is_empty() {
			return Err(anyhow::anyhow!("No outputs intersect with region"));
		}

		let max_scale = intersecting
			.iter()
			.map(|(_, o)| o.scale.max(1))
			.max()
			.unwrap_or(1) as i32;

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

		for (output_idx, (_, output)) in intersecting.iter().enumerate() {
			let conn = conn.clone();
			let wl_outputs = wl_outputs.clone();
			let stop_receiver = stop_receiver.clone();
			let frame_sender = frame_senders[output_idx].0.clone();
			let return_receiver = return_receivers[output_idx].clone();
			let format_sender = format_senders[output_idx].clone();
			let output = output.clone();
			let wl_output = wl_outputs[output.id].clone();

			std::thread::spawn(move || -> Result<()> {
				let (globals, mut event_queue) = registry_queue_init::<CaptureState>(&conn)
					.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
				let qh = event_queue.handle();

				let screencopy_mgr: ZwlrScreencopyManagerV1 = globals
					.bind(&qh, 3..=3, ())
					.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

				let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
				let cursor_val = if include_cursor { 1 } else { 0 };

				let mut pool: Vec<(WlBuffer, std::fs::File, std::sync::Arc<memmap2::Mmap>, bool)> =
					Vec::new();

				let mut state = CaptureState::default();
				let frame =
					WlFrameGuard(screencopy_mgr.capture_output(cursor_val, &wl_output, &qh, ()));

				while !state.buffer_done {
					event_queue.blocking_dispatch(&mut state)?;
				}

				let frame_format = state
					.formats
					.first()
					.ok_or(FramrError::NoSupportedBufferFormat)?
					.clone();

				let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
				let (buffer, file, mmap) = allocate_shm_buffer(
					&shm,
					&qh,
					frame_format.width,
					frame_format.height,
					frame_format.stride,
					wl_fmt,
				)?;
				let mmap = std::sync::Arc::new(mmap);
				pool.push((buffer, file, mmap, false));

				let (wl_buffer, _, mmap, in_use) = &mut pool[0];
				*in_use = true;

				frame.copy(wl_buffer);

				while state.frame_state == FrameState::Pending {
					event_queue.blocking_dispatch(&mut state)?;
				}

				if state.frame_state == FrameState::Failed {
					return Err(FramrError::FrameCaptureFailed.into());
				}

				if format_sender.send(frame_format.clone()).is_err() {
					return Ok(());
				}

				let pts_nanos = (state.tv_sec_hi as u64) << 32 | (state.tv_sec_lo as u64);
				let pts = pts_nanos * 1_000_000_000 + (state.tv_nsec as u64);

				if frame_sender
					.send((mmap.clone(), 0, pts, frame_format.clone()))
					.is_err()
				{
					return Ok(());
				}

				loop {
					if stop_receiver.try_recv().is_ok() {
						break;
					}

					while let Ok(idx) = return_receiver.try_recv() {
						if let Some(slot) = pool.get_mut(idx) {
							slot.3 = false;
						}
					}

					let mut state = CaptureState::default();
					let frame = WlFrameGuard(screencopy_mgr.capture_output(
						cursor_val,
						&wl_output,
						&qh,
						(),
					));

					while !state.buffer_done {
						event_queue.blocking_dispatch(&mut state)?;
					}

					let frame_format = state
						.formats
						.first()
						.ok_or(FramrError::NoSupportedBufferFormat)?
						.clone();

					let buffer_idx = if let Some(idx) = pool.iter().position(|slot| !slot.3) {
						idx
					} else if pool.len() < 3 {
						let wl_fmt = pixel_format_to_wl_shm(frame_format.format);
						let (buffer, file, mmap) = allocate_shm_buffer(
							&shm,
							&qh,
							frame_format.width,
							frame_format.height,
							frame_format.stride,
							wl_fmt,
						)?;
						let mmap = std::sync::Arc::new(mmap);
						pool.push((buffer, file, mmap, false));
						pool.len() - 1
					} else {
						return Ok(());
					};

					let (wl_buffer, _, mmap, in_use) = &mut pool[buffer_idx];
					*in_use = true;

					frame.copy(wl_buffer);

					while state.frame_state == FrameState::Pending {
						event_queue.blocking_dispatch(&mut state)?;
					}

					if state.frame_state == FrameState::Failed {
						return Err(FramrError::FrameCaptureFailed.into());
					}

					let pts_nanos = (state.tv_sec_hi as u64) << 32 | (state.tv_sec_lo as u64);
					let pts = pts_nanos * 1_000_000_000 + (state.tv_nsec as u64);

					if frame_sender
						.send((mmap.clone(), buffer_idx, pts, frame_format.clone()))
						.is_err()
					{
						break;
					}
				}

				Ok(())
			});
		}

		let encoder_outputs = intersecting.iter().map(|(_, o)| o.clone()).collect();
		let pipeline_thread = std::thread::spawn(move || -> Result<()> {
			crate::encoding::run_composite_encoding_pipeline(
				output_path,
				region,
				max_scale,
				encoder_outputs,
				frame_receivers,
				format_receivers,
				return_senders,
				stop_receiver,
			)
		});

		Ok(RecordingHandle {
			stop_sender,
			pipeline_thread,
		})
	}
}
