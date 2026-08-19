use std::path::PathBuf;
use std::ptr::NonNull;
use std::time::Duration;
use std::time::Instant;

use anyhow::{Result, anyhow};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::pod::Pod;

use crate::{Frame, FrameFormat, PixelFormat, RecordingConfig, Transform};

struct StreamData {
	format: spa::param::video::VideoInfoRaw,
	frames: crossbeam_channel::Sender<Frame>,
	started: Instant,
	timestamps: TimestampClock,
}

enum TimestampClock {
	Uninitialized,
	PipeWire {
		pts_origin: Duration,
		elapsed_origin: Duration,
	},
	Monotonic,
}

impl TimestampClock {
	fn resolve(&mut self, pipewire_pts: Option<Duration>, elapsed: Duration) -> Duration {
		match self {
			Self::Uninitialized => match pipewire_pts {
				Some(pts_origin) => {
					*self = Self::PipeWire {
						pts_origin,
						elapsed_origin: elapsed,
					};
					elapsed
				}
				None => {
					*self = Self::Monotonic;
					elapsed
				}
			},
			Self::PipeWire {
				pts_origin,
				elapsed_origin,
			} => pipewire_pts.map_or(elapsed, |pts| {
				if pts >= *pts_origin {
					elapsed_origin.saturating_add(pts - *pts_origin)
				} else {
					elapsed_origin.saturating_sub(*pts_origin - pts)
				}
			}),
			Self::Monotonic => elapsed,
		}
	}
}

enum Control {
	Stop,
}

struct PipeWireBuffer<'a> {
	stream: &'a pw::stream::Stream,
	raw: NonNull<pw::sys::pw_buffer>,
}

impl<'a> PipeWireBuffer<'a> {
	fn dequeue(stream: &'a pw::stream::Stream) -> Option<Self> {
		NonNull::new(unsafe { stream.dequeue_raw_buffer() }).map(|raw| Self { stream, raw })
	}

	fn timestamp(&self) -> Option<Duration> {
		let buffer = unsafe { self.raw.as_ref().buffer.as_ref() }?;
		let metas = if buffer.n_metas == 0 || buffer.metas.is_null() {
			&[]
		} else {
			unsafe { std::slice::from_raw_parts(buffer.metas, buffer.n_metas as usize) }
		};
		let meta = metas.iter().find(|meta| {
			meta.type_ == spa::sys::SPA_META_Header
				&& meta.size as usize >= std::mem::size_of::<spa::sys::spa_meta_header>()
				&& !meta.data.is_null()
		})?;
		let header = unsafe { &*meta.data.cast::<spa::sys::spa_meta_header>() };
		(header.pts >= 0).then(|| Duration::from_nanos(header.pts as u64))
	}

	fn data(&self) -> Option<(&[u8], spa::sys::spa_chunk)> {
		let buffer = unsafe { self.raw.as_ref().buffer.as_ref() }?;
		if buffer.n_datas == 0 || buffer.datas.is_null() {
			return None;
		}
		let data = unsafe { &*buffer.datas };
		let chunk = unsafe { data.chunk.as_ref() }?;
		if data.data.is_null() || data.maxsize == 0 {
			return None;
		}
		let mapped =
			unsafe { std::slice::from_raw_parts(data.data.cast::<u8>(), data.maxsize as usize) };
		Some((mapped, *chunk))
	}
}

impl Drop for PipeWireBuffer<'_> {
	fn drop(&mut self) {
		unsafe { self.stream.queue_raw_buffer(self.raw.as_ptr()) };
	}
}

fn pixel_format(format: spa::param::video::VideoFormat) -> Option<PixelFormat> {
	match format {
		spa::param::video::VideoFormat::BGRx => Some(PixelFormat::Xrgb8888),
		spa::param::video::VideoFormat::BGRA => Some(PixelFormat::Argb8888),
		spa::param::video::VideoFormat::RGBx => Some(PixelFormat::Xbgr8888),
		spa::param::video::VideoFormat::RGBA => Some(PixelFormat::Abgr8888),
		_ => None,
	}
}

pub(crate) fn run_pipewire_encoding_pipeline(
	node_id: u32,
	output_path: PathBuf,
	stop_receiver: crossbeam_channel::Receiver<()>,
	recording_config: RecordingConfig,
) -> Result<()> {
	pw::init();
	let mainloop = pw::main_loop::MainLoopRc::new(None)?;
	let context = pw::context::ContextRc::new(&mainloop, None)?;
	let core = context.connect_rc(None)?;
	let (frame_sender, frame_receiver) = crossbeam_channel::bounded(3);
	let encoder_thread = std::thread::spawn(move || {
		super::run_frame_encoding_pipeline(output_path, frame_receiver, recording_config)
	});
	let stream = pw::stream::StreamBox::new(
		&core,
		"libframr",
		properties! {
			*pw::keys::MEDIA_TYPE => "Video",
			*pw::keys::MEDIA_CATEGORY => "Capture",
			*pw::keys::MEDIA_ROLE => "Screen",
		},
	)?;
	let stream_error = std::sync::Arc::new(std::sync::Mutex::new(None));
	let error_for_callback = stream_error.clone();
	let loop_for_state = mainloop.downgrade();
	let loop_for_process = mainloop.downgrade();
	let listener = stream
		.add_local_listener_with_user_data(StreamData {
			format: Default::default(),
			frames: frame_sender,
			started: Instant::now(),
			timestamps: TimestampClock::Uninitialized,
		})
		.state_changed(move |_, _, _, state| {
			if let pw::stream::StreamState::Error(error) = state {
				if let Ok(mut stored) = error_for_callback.lock() {
					*stored = Some(error);
				}
				if let Some(mainloop) = loop_for_state.upgrade() {
					mainloop.quit();
				}
			}
		})
		.param_changed(|_, data, id, param| {
			let Some(param) = param else {
				return;
			};
			if id != spa::param::ParamType::Format.as_raw() {
				return;
			}
			if spa::param::format_utils::parse_format(param).ok()
				!= Some((
					spa::param::format::MediaType::Video,
					spa::param::format::MediaSubtype::Raw,
				)) {
				return;
			}
			let _ = data.format.parse(param);
		})
		.process(move |stream, state| {
			let Some(buffer) = PipeWireBuffer::dequeue(stream) else {
				return;
			};
			let size = state.format.size();
			let Some(format) = pixel_format(state.format.format()) else {
				return;
			};
			let pipewire_pts = buffer.timestamp();
			let Some((mapped, chunk)) = buffer.data() else {
				return;
			};
			let offset = chunk.offset as usize;
			if offset >= mapped.len() {
				return;
			}
			let stride = chunk.stride.unsigned_abs().max(size.width * 4);
			let length = stride as usize * size.height as usize;
			if chunk.size < length as u32 {
				return;
			}
			let Some(end) = offset.checked_add(length) else {
				return;
			};
			if end > mapped.len() {
				return;
			}
			let timestamp = state
				.timestamps
				.resolve(pipewire_pts, state.started.elapsed());
			let frame = Frame::from_owned(
				mapped[offset..end].to_vec(),
				FrameFormat {
					format,
					width: size.width as i32,
					height: size.height as i32,
					stride: stride as i32,
				},
				Transform::Normal,
				timestamp,
			);
			if let Err(crossbeam_channel::TrySendError::Disconnected(_)) =
				state.frames.try_send(frame)
				&& let Some(mainloop) = loop_for_process.upgrade()
			{
				mainloop.quit();
			}
		})
		.register()?;

	let format = spa::pod::object!(
		spa::utils::SpaTypes::ObjectParamFormat,
		spa::param::ParamType::EnumFormat,
		spa::pod::property!(
			spa::param::format::FormatProperties::MediaType,
			Id,
			spa::param::format::MediaType::Video
		),
		spa::pod::property!(
			spa::param::format::FormatProperties::MediaSubtype,
			Id,
			spa::param::format::MediaSubtype::Raw
		),
		spa::pod::property!(
			spa::param::format::FormatProperties::VideoFormat,
			Choice,
			Enum,
			Id,
			spa::param::video::VideoFormat::BGRx,
			spa::param::video::VideoFormat::BGRx,
			spa::param::video::VideoFormat::BGRA,
			spa::param::video::VideoFormat::RGBx,
			spa::param::video::VideoFormat::RGBA,
		),
	);
	let values = spa::pod::serialize::PodSerializer::serialize(
		std::io::Cursor::new(Vec::new()),
		&spa::pod::Value::Object(format),
	)?
	.0
	.into_inner();
	let mut params = [Pod::from_bytes(&values).ok_or_else(|| anyhow!("invalid PipeWire format"))?];
	stream.connect(
		spa::utils::Direction::Input,
		Some(node_id),
		pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
		&mut params,
	)?;

	let (control_sender, control_receiver) = pw::channel::channel();
	let loop_for_control = mainloop.clone();
	let attached = control_receiver.attach(mainloop.loop_(), move |_: Control| {
		loop_for_control.quit();
	});
	std::thread::spawn(move || {
		let _ = stop_receiver.recv();
		let _ = control_sender.send(Control::Stop);
	});
	mainloop.run();
	drop(attached);
	drop(listener);
	drop(stream);
	drop(core);
	drop(context);
	let encoder_result = encoder_thread
		.join()
		.map_err(|_| anyhow!("FFmpeg encoder thread panicked"))?;
	if let Some(error) = stream_error.lock().ok().and_then(|mut error| error.take()) {
		return Err(anyhow!("PipeWire stream failed: {error}"));
	}
	encoder_result
}
