mod converter;
mod pipewire;
mod vaapi;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ffmpeg::codec;
use ffmpeg::format::Pixel;
use ffmpeg::{Dictionary, Packet, Rational};
use ffmpeg_next as ffmpeg;

use crate::{
	EncoderBackend, Frame, FrameData, LogicalRegion, OutputInfo, RecordingConfig, VideoEncoder,
};
use converter::{FrameConverter, is_ten_bit, output_size, packed_format, packed_frame};
use vaapi::VaapiFrames;

pub(crate) enum FrameMessage {
	Frame { frame: Frame, slot: usize },
	Error(String),
}

impl FrameMessage {
	fn into_frame(self) -> Result<(Frame, usize)> {
		match self {
			Self::Frame { frame, slot } => Ok((frame, slot)),
			Self::Error(error) => Err(anyhow!(error)),
		}
	}
}

struct TimestampTimeline {
	origin: Option<Duration>,
	last_pts: Option<i64>,
	fps: u32,
}

impl TimestampTimeline {
	fn new(fps: u32) -> Self {
		Self {
			origin: None,
			last_pts: None,
			fps,
		}
	}

	fn pts(&mut self, timestamp: Duration) -> Option<i64> {
		let origin = *self.origin.get_or_insert(timestamp);
		let elapsed = timestamp.saturating_sub(origin).as_nanos();
		let pts = elapsed.saturating_mul(u128::from(self.fps)) / 1_000_000_000;
		let pts = i64::try_from(pts).unwrap_or(i64::MAX);
		if self.last_pts.is_some_and(|last| pts <= last) {
			return None;
		}
		self.last_pts = Some(pts);
		Some(pts)
	}
}

enum ActiveBackend {
	Software(FrameConverter),
	Vaapi(VaapiFrames),
}

struct VideoWriter {
	output: ffmpeg::format::context::Output,
	encoder: ffmpeg::encoder::video::Encoder,
	backend: ActiveBackend,
	stream_index: usize,
	encoder_time_base: Rational,
	stream_time_base: Rational,
}

impl VideoWriter {
	fn new(path: &Path, first_frame: &Frame, config: &RecordingConfig) -> Result<Self> {
		if config.fps == 0 {
			return Err(anyhow!("recording FPS must be greater than zero"));
		}
		ffmpeg::init().context("failed to initialize FFmpeg")?;
		match config.backend {
			EncoderBackend::Software => {
				Self::new_with_backend(path, first_frame, config, EncoderBackend::Software)
			}
			EncoderBackend::Vaapi => {
				Self::new_with_backend(path, first_frame, config, EncoderBackend::Vaapi)
			}
			EncoderBackend::Nvenc => {
				Self::new_with_backend(path, first_frame, config, EncoderBackend::Nvenc)
			}
			EncoderBackend::Auto => Self::new_with_backend(
				path,
				first_frame,
				config,
				EncoderBackend::Vaapi,
			)
			.or_else(|_| {
				if config.encoder == VideoEncoder::VP9 {
					Self::new_with_backend(path, first_frame, config, EncoderBackend::Software)
				} else {
					Self::new_with_backend(path, first_frame, config, EncoderBackend::Nvenc)
						.or_else(|_| {
							Self::new_with_backend(
								path,
								first_frame,
								config,
								EncoderBackend::Software,
							)
						})
				}
			}),
		}
	}

	fn new_with_backend(
		path: &Path,
		first_frame: &Frame,
		config: &RecordingConfig,
		backend: EncoderBackend,
	) -> Result<Self> {
		let use_vaapi = backend == EncoderBackend::Vaapi;
		let (visible_width, visible_height) = output_size(first_frame);
		let visible_width = visible_width.max(1);
		let visible_height = visible_height.max(1);
		let width = align_chroma(visible_width.max(2));
		let height = align_chroma(visible_height.max(2));
		let ten_bit = is_ten_bit(first_frame.format.format);
		let vaapi_format = if ten_bit { Pixel::P010LE } else { Pixel::NV12 };
		let encoder_format = match (backend, config.encoder, ten_bit) {
			(EncoderBackend::Vaapi, _, _) => Pixel::VAAPI,
			(EncoderBackend::Nvenc, _, false) => Pixel::NV12,
			(EncoderBackend::Nvenc, _, true) => Pixel::P010LE,
			(EncoderBackend::Software, VideoEncoder::H264, false) => Pixel::NV12,
			(EncoderBackend::Software, VideoEncoder::H264, true) => Pixel::YUV420P10LE,
			(EncoderBackend::Software, VideoEncoder::AV1, false) => Pixel::YUV420P,
			(EncoderBackend::Software, VideoEncoder::AV1, true) => Pixel::YUV420P10LE,
			(EncoderBackend::Software, VideoEncoder::VP9, false) => Pixel::YUV420P,
			(EncoderBackend::Software, VideoEncoder::VP9, true) => Pixel::YUV420P10LE,
			(EncoderBackend::Auto, _, _) => {
				return Err(anyhow!("automatic backend must be resolved before configuring encoder"));
			}
		};
		let codec_name = match (backend, config.encoder) {
			(EncoderBackend::Vaapi, VideoEncoder::H264) => "h264_vaapi",
			(EncoderBackend::Vaapi, VideoEncoder::AV1) => "av1_vaapi",
			(EncoderBackend::Vaapi, VideoEncoder::VP9) => "vp9_vaapi",
			(EncoderBackend::Nvenc, VideoEncoder::H264) => "h264_nvenc",
			(EncoderBackend::Nvenc, VideoEncoder::AV1) => "av1_nvenc",
			(EncoderBackend::Nvenc, VideoEncoder::VP9) => {
				return Err(anyhow!("NVENC does not support VP9 encoding"));
			}
			(EncoderBackend::Software, VideoEncoder::H264) => "libx264",
			(EncoderBackend::Software, VideoEncoder::AV1) => ["libsvtav1", "librav1e", "libaom-av1"]
				.into_iter()
				.find(|name| ffmpeg::encoder::find_by_name(name).is_some())
				.ok_or_else(|| anyhow!("no FFmpeg AV1 software encoder is available"))?,
			(EncoderBackend::Software, VideoEncoder::VP9) => "libvpx-vp9",
			(EncoderBackend::Auto, _) => {
				return Err(anyhow!("automatic backend must be resolved before opening encoder"));
			}
		};
		let codec = ffmpeg::encoder::find_by_name(codec_name)
			.ok_or_else(|| anyhow!("FFmpeg encoder {codec_name} is unavailable"))?;
		let mut output = ffmpeg::format::output(path)
			.with_context(|| format!("failed to create output {}", path.display()))?;
		let global_header = output
			.format()
			.flags()
			.contains(ffmpeg::format::Flags::GLOBAL_HEADER);
		let encoder_time_base = Rational(1, config.fps as i32);
		let mut encoder = codec::context::Context::new_with_codec(codec)
			.encoder()
			.video()?;
		encoder.set_width(width);
		encoder.set_height(height);
		encoder.set_format(encoder_format);
		encoder.set_time_base(encoder_time_base);
		encoder.set_frame_rate(Some(Rational(config.fps as i32, 1)));
		encoder.set_bit_rate(config.bitrate as usize * 1000);
		encoder.set_gop(config.keyframe_interval);
		encoder.set_max_b_frames(0);
		if global_header {
			encoder.set_flags(codec::Flags::GLOBAL_HEADER);
		}
		if let Some(threads) = config.threads {
			encoder.set_threading(codec::threading::Config::count(threads as usize));
		}

		let mut options = Dictionary::new();
		match (backend, config.encoder) {
			(EncoderBackend::Nvenc, VideoEncoder::H264 | VideoEncoder::AV1) => {
				options.set("preset", config.speed.nvenc_preset());
			}
			(EncoderBackend::Software, VideoEncoder::H264) => {
				options.set("preset", config.speed.software_preset());
				options.set("tune", config.tune.as_ref());
			}
			(EncoderBackend::Software, VideoEncoder::AV1) => match codec_name {
				"libsvtav1" => options.set("preset", &config.speed.av1_preset().to_string()),
				"librav1e" => {
					options.set("speed", &config.speed.av1_preset().min(10).to_string())
				}
				_ => options.set("cpu-used", &config.speed.av1_preset().min(8).to_string()),
			},
			(EncoderBackend::Software, VideoEncoder::VP9) => {
				options.set("deadline", "good");
				options.set("cpu-used", &config.speed.vp9_cpu_used().to_string());
			}
			_ => {}
		}

		let backend = if use_vaapi {
			let frames = VaapiFrames::new(
				&mut encoder,
				config.vaapi_device.as_deref(),
				vaapi_format,
				width,
				height,
				visible_width,
				visible_height,
				packed_format(first_frame),
				visible_width,
				visible_height,
				config.fps,
				matches!(&first_frame.data, FrameData::DmaBuf(_))
					&& first_frame.transform == crate::Transform::Normal,
			)?;
			ActiveBackend::Vaapi(frames)
		} else {
			ActiveBackend::Software(FrameConverter::new_padded(
				visible_width,
				visible_height,
				width,
				height,
				encoder_format,
			))
		};
		let encoder = encoder.open_as_with(codec, options).with_context(|| {
			format!("failed to open FFmpeg encoder {codec_name} for {width}x{height}")
		})?;
		let stream_index = {
			let mut stream = output.add_stream(codec)?;
			stream.set_time_base(encoder_time_base);
			stream.set_parameters(&encoder);
			set_stream_cropping(&mut stream, height - visible_height, width - visible_width)?;
			stream.index()
		};
		output.write_header()?;
		let stream_time_base = output
			.stream(stream_index)
			.ok_or_else(|| anyhow!("FFmpeg output stream disappeared"))?
			.time_base();

		Ok(Self {
			output,
			encoder,
			backend,
			stream_index,
			encoder_time_base,
			stream_time_base,
		})
	}

	fn write_frame(&mut self, frame: &Frame, pts: i64) -> Result<()> {
		match &mut self.backend {
			ActiveBackend::Software(converter) => {
				let mut converted = converter.convert(frame)?;
				converted.set_pts(Some(pts));
				self.encoder.send_frame(&converted)?;
			}
			ActiveBackend::Vaapi(frames) => {
				let source = if frames.uses_dmabuf() {
					frames.import_dmabuf(frame, pts)?
				} else {
					let mut source = packed_frame(frame)?;
					source.set_pts(Some(pts));
					source
				};
				let hardware = frames.process(&source)?;
				self.encoder.send_frame(&hardware)?;
			}
		}
		self.write_packets()
	}

	fn write_packets(&mut self) -> Result<()> {
		let mut packet = Packet::empty();
		loop {
			match self.encoder.receive_packet(&mut packet) {
				Ok(()) => {
					packet.set_stream(self.stream_index);
					packet.rescale_ts(self.encoder_time_base, self.stream_time_base);
					packet.write_interleaved(&mut self.output)?;
				}
				Err(ffmpeg::Error::Other {
					errno: ffmpeg::error::EAGAIN,
				})
				| Err(ffmpeg::Error::Eof) => break,
				Err(error) => return Err(error.into()),
			}
		}
		Ok(())
	}

	fn finish(mut self) -> Result<()> {
		self.encoder.send_eof()?;
		self.write_packets()?;
		self.output.write_trailer()?;
		Ok(())
	}
}

fn align_chroma(value: u32) -> u32 {
	(value + 1) & !1
}

fn set_stream_cropping(
	stream: &mut ffmpeg::format::stream::StreamMut<'_>,
	bottom: u32,
	right: u32,
) -> Result<()> {
	if bottom == 0 && right == 0 {
		return Ok(());
	}
	let side_data = unsafe {
		let parameters = (*stream.as_mut_ptr()).codecpar;
		ffmpeg::ffi::av_packet_side_data_new(
			&mut (*parameters).coded_side_data,
			&mut (*parameters).nb_coded_side_data,
			ffmpeg::ffi::AVPacketSideDataType::AV_PKT_DATA_FRAME_CROPPING,
			16,
			0,
		)
	};
	if side_data.is_null() {
		return Err(anyhow!(
			"failed to allocate encoded frame cropping metadata"
		));
	}
	let values = [0_u32, bottom, 0, right];
	let data = unsafe { std::slice::from_raw_parts_mut((*side_data).data, 16) };
	for (chunk, value) in data.chunks_exact_mut(4).zip(values) {
		chunk.copy_from_slice(&value.to_le_bytes());
	}
	Ok(())
}

pub(crate) fn run_single_encoding_pipeline(
	output_path: PathBuf,
	frame_receiver: crossbeam_channel::Receiver<FrameMessage>,
	return_sender: crossbeam_channel::Sender<usize>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let (first, first_slot) = frame_receiver
		.recv()
		.map_err(|_| anyhow!("capture ended before the first frame"))?
		.into_frame()?;
	let mut writer = VideoWriter::new(&output_path, &first, &recording_config)?;
	let mut timeline = TimestampTimeline::new(recording_config.fps);
	if let Some(pts) = timeline.pts(first.timestamp) {
		writer.write_frame(&first, pts)?;
	}
	let _ = return_sender.send(first_slot);

	while let Ok(message) = frame_receiver.recv() {
		let (frame, slot) = message.into_frame()?;
		let result = if let Some(pts) = timeline.pts(frame.timestamp) {
			writer.write_frame(&frame, pts)
		} else {
			Ok(())
		};
		let _ = return_sender.send(slot);
		result?;
	}
	writer.finish()
}

fn run_frame_encoding_pipeline(
	output_path: PathBuf,
	frame_receiver: crossbeam_channel::Receiver<Frame>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let first = frame_receiver
		.recv()
		.map_err(|_| anyhow!("capture ended before the first frame"))?;
	let mut writer = VideoWriter::new(&output_path, &first, &recording_config)?;
	let mut timeline = TimestampTimeline::new(recording_config.fps);
	if let Some(pts) = timeline.pts(first.timestamp) {
		writer.write_frame(&first, pts)?;
	}
	while let Ok(frame) = frame_receiver.recv() {
		if let Some(pts) = timeline.pts(frame.timestamp) {
			writer.write_frame(&frame, pts)?;
		}
	}
	writer.finish()
}

pub(crate) use pipewire::run_pipewire_encoding_pipeline;

pub(crate) fn run_composite_encoding_pipeline(
	output_path: PathBuf,
	region: LogicalRegion,
	max_scale: i32,
	intersecting_outputs: Vec<OutputInfo>,
	frame_receivers: Vec<crossbeam_channel::Receiver<FrameMessage>>,
	return_senders: Vec<crossbeam_channel::Sender<usize>>,
	stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let width = region.size.width * max_scale as u32;
	let height = region.size.height * max_scale as u32;
	let mut composite_outputs: Vec<CompositeOutput> = intersecting_outputs
		.iter()
		.map(|output| {
			CompositeOutput::new(
				output.logical_size.width * max_scale as u32,
				output.logical_size.height * max_scale as u32,
			)
		})
		.collect();
	let mut pending: Vec<Option<(Frame, usize)>> =
		(0..frame_receivers.len()).map(|_| None).collect();
	let mut writer = None;
	let mut timeline = TimestampTimeline::new(recording_config.fps);
	let mut select = crossbeam_channel::Select::new();
	for receiver in &frame_receivers {
		select.recv(receiver);
	}

	loop {
		if stop.load(std::sync::atomic::Ordering::Acquire) {
			break;
		}
		let operation = match select.select_timeout(Duration::from_millis(100)) {
			Ok(operation) => operation,
			Err(_) => continue,
		};
		let index = operation.index();
		let Ok(message) = operation.recv(&frame_receivers[index]) else {
			break;
		};
		let (frame, slot) = message.into_frame()?;
		let timestamp = frame.timestamp;
		if let Some((_, previous_slot)) = pending[index].replace((frame, slot)) {
			let _ = return_senders[index].send(previous_slot);
		}
		let Some(pts) = timeline.pts(timestamp) else {
			continue;
		};
		for (index, pending_frame) in pending.iter_mut().enumerate() {
			if let Some((frame, slot)) = pending_frame.take() {
				let update_result = composite_outputs[index].update(&frame);
				let _ = return_senders[index].send(slot);
				update_result?;
			}
		}
		if composite_outputs
			.iter()
			.any(|output| output.image.is_none())
		{
			continue;
		}
		let composite = composite_frame(
			&composite_outputs,
			&intersecting_outputs,
			region,
			max_scale,
			width,
			height,
			timestamp,
		)?;
		let writer = match &mut writer {
			Some(writer) => writer,
			None => writer.insert(VideoWriter::new(
				&output_path,
				&composite,
				&recording_config,
			)?),
		};
		writer.write_frame(&composite, pts)?;
	}

	for (index, pending_frame) in pending.into_iter().enumerate() {
		if let Some((_, slot)) = pending_frame {
			let _ = return_senders[index].send(slot);
		}
	}
	match writer {
		Some(writer) => writer.finish(),
		None => Err(anyhow!(
			"capture ended before a composite frame was available"
		)),
	}
}

struct CompositeOutput {
	converter: FrameConverter,
	image: Option<image::RgbaImage>,
	width: u32,
	height: u32,
}

impl CompositeOutput {
	fn new(width: u32, height: u32) -> Self {
		Self {
			converter: FrameConverter::new(width, height, Pixel::RGBA),
			image: None,
			width,
			height,
		}
	}

	fn update(&mut self, frame: &Frame) -> Result<()> {
		let video = self.converter.convert(frame)?;
		let image = self
			.image
			.get_or_insert_with(|| image::RgbaImage::new(self.width, self.height));
		let row_size = self.width as usize * 4;
		for row in 0..self.height as usize {
			let source_start = row * video.stride(0);
			let destination_start = row * row_size;
			image.as_mut()[destination_start..destination_start + row_size]
				.copy_from_slice(&video.data(0)[source_start..source_start + row_size]);
		}
		Ok(())
	}
}

fn composite_frame(
	frames: &[CompositeOutput],
	outputs: &[OutputInfo],
	region: LogicalRegion,
	max_scale: i32,
	width: u32,
	height: u32,
	timestamp: Duration,
) -> Result<Frame> {
	let mut composite = image::RgbaImage::new(width, height);
	for (frame, output) in frames.iter().zip(outputs) {
		let rgba = frame
			.image
			.as_ref()
			.ok_or_else(|| anyhow!("missing composite frame"))?;
		let x = (output.logical_position.x - region.position.x) as i64 * i64::from(max_scale);
		let y = (output.logical_position.y - region.position.y) as i64 * i64::from(max_scale);
		image::imageops::overlay(&mut composite, rgba, x, y);
	}
	Ok(Frame::from_owned(
		composite.into_raw(),
		crate::FrameFormat {
			format: crate::PixelFormat::Abgr8888,
			width: width as i32,
			height: height as i32,
			stride: width as i32 * 4,
		},
		crate::Transform::Normal,
		timestamp,
	))
}
