use std::sync::Arc;

use anyhow::Result;
use gstreamer::prelude::*;
use gstreamer_app::AppSrc;
use memmap2::Mmap;

use crate::RecordingConfig;
use crate::output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat, Transform};

pub fn wait_for_gstreamer_eos(pipeline: &gstreamer::Pipeline) -> Result<()> {
	let bus = pipeline.bus().unwrap();
	for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
		use gstreamer::MessageView;
		match msg.view() {
			MessageView::Eos(..) => break,
			MessageView::Error(err) => {
				return Err(anyhow::anyhow!(
					"GStreamer error: {} ({})",
					err.error(),
					err.debug().unwrap_or_else(|| "no debug info".into())
				));
			}
			_ => (),
		}
	}
	pipeline.set_state(gstreamer::State::Null)?;
	Ok(())
}

fn apply_encoder_config(encoder: &gstreamer::Element, config: &RecordingConfig) {
	if config.tune.is_psy_tune() {
		encoder.set_property("psy-tune", config.tune.to_gst_value());
	} else {
		encoder.set_property("tune", config.tune.to_gst_value());
	}
	encoder.set_property("speed-preset", config.speed_preset.to_gst_value());
	encoder.set_property("bitrate", config.bitrate);
	encoder.set_property("key-int-max", config.keyframe_interval);
	encoder.set_property("threads", config.threads.unwrap_or(0));
}

fn push_buffer(appsrc: &AppSrc, mmap: &[u8], pts: u64, previous_pts: Option<u64>) -> Result<()> {
	let mut buffer = gstreamer::Buffer::with_size(mmap.len())
		.map_err(|_| anyhow::anyhow!("Failed to create buffer"))?;

	{
		let buffer_mut = buffer.get_mut().unwrap();
		buffer_mut.set_pts(gstreamer::ClockTime::from_nseconds(pts));

		if let Some(prev) = previous_pts {
			if pts > prev {
				let duration = gstreamer::ClockTime::from_nseconds(pts - prev);
				buffer_mut.set_duration(Some(duration));
			}
		}

		buffer_mut
			.copy_from_slice(0, mmap)
			.map_err(|e| anyhow::anyhow!("copy_from_slice failed: {e}"))?;
	}

	appsrc.push_buffer(buffer)?;
	Ok(())
}

pub fn run_single_encoding_pipeline(
	transform: Transform,
	output_path: std::path::PathBuf,
	frame_receiver: crossbeam_channel::Receiver<(Arc<Mmap>, usize, u64, FrameFormat)>,
	return_sender: crossbeam_channel::Sender<usize>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let flip_method = match transform {
		Transform::Normal => "none",
		Transform::_90 => "clockwise",
		Transform::_180 => "rotate-180",
		Transform::_270 => "counterclockwise",
		Transform::Flipped => "horizontal-flip",
		Transform::Flipped90 => "upper-left-diagonal",
		Transform::Flipped180 => "vertical-flip",
		Transform::Flipped270 => "upper-right-diagonal",
	};

	let tune_prop = if recording_config.tune.is_psy_tune() {
		"psy-tune"
	} else {
		"tune"
	};

	let threads = recording_config.threads.unwrap_or(0);
	let pipeline_str = format!(
		"appsrc name=src format=time is-live=true ! videoconvert ! videoflip method={} ! x264enc {}={} speed-preset={} bitrate={} key-int-max={} threads={} ! mp4mux ! filesink location={}",
		flip_method,
		tune_prop,
		recording_config.tune.as_str(),
		recording_config.speed_preset.as_str(),
		recording_config.bitrate,
		recording_config.keyframe_interval,
		threads,
		output_path.to_string_lossy()
	);

	let pipeline = gstreamer::parse::launch(&pipeline_str)
        .map_err(|e| anyhow::anyhow!("Failed to launch GStreamer pipeline. Ensure gst-plugins-good and gst-plugins-ugly (for x264enc) are installed. Error: {}", e))?
        .dynamic_cast::<gstreamer::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Failed to cast to Pipeline"))?;

	let appsrc = pipeline
		.by_name("src")
		.ok_or_else(|| anyhow::anyhow!("Failed to find appsrc"))?
		.dynamic_cast::<AppSrc>()
		.map_err(|_| anyhow::anyhow!("Failed to cast to AppSrc"))?;

	appsrc.set_max_bytes(1024 * 1024 * 50);
	appsrc.set_property("emit-signals", false);
	appsrc.set_property("is-live", true);

	pipeline.set_state(gstreamer::State::Ready)?;

	let (_, _, _, format) = frame_receiver.recv()?;

	let gst_format = match format.format {
		PixelFormat::Argb8888 => gstreamer_video::VideoFormat::Bgra,
		PixelFormat::Xrgb8888 => gstreamer_video::VideoFormat::Bgrx,
		PixelFormat::Abgr8888 => gstreamer_video::VideoFormat::Rgba,
		PixelFormat::Xbgr8888 => gstreamer_video::VideoFormat::Rgbx,
		_ => {
			return Err(anyhow::anyhow!(
				"Unsupported pixel format for recording: {:?}",
				format.format
			));
		}
	};

	let caps = gstreamer_video::VideoCapsBuilder::new()
		.format(gst_format)
		.width(format.width)
		.height(format.height)
		.framerate(gstreamer::Fraction::new(0, 1))
		.build();

	appsrc.set_caps(Some(&caps));

	pipeline.set_state(gstreamer::State::Playing)?;

	let mut previous_pts = None;

	while let Ok((mmap, buffer_idx, pts, _format)) = frame_receiver.recv() {
		push_buffer(&appsrc, &mmap, pts, previous_pts)?;
		let _ = return_sender.send(buffer_idx);
		previous_pts = Some(pts);
	}

	appsrc.end_of_stream()?;
	wait_for_gstreamer_eos(&pipeline)?;
	Ok(())
}

pub fn run_composite_encoding_pipeline(
	output_path: std::path::PathBuf,
	region: LogicalRegion,
	max_scale: i32,
	intersecting_outputs: Vec<OutputInfo>,
	frame_receivers: Vec<crossbeam_channel::Receiver<(Arc<Mmap>, usize, u64, FrameFormat)>>,
	format_receivers: Vec<crossbeam_channel::Receiver<FrameFormat>>,
	return_senders: Vec<crossbeam_channel::Sender<usize>>,
	stop_receiver: crossbeam_channel::Receiver<()>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let num_outputs = intersecting_outputs.len();
	let composite_width = (region.size.width as i32 * max_scale + 1) / 2 * 2;
	let composite_height = (region.size.height as i32 * max_scale + 1) / 2 * 2;

	let pipeline = gstreamer::Pipeline::new();

	let mut appsrcs = Vec::with_capacity(num_outputs);
	let compositor = gstreamer::ElementFactory::make("compositor")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create compositor. Error: {}", e))?;

	compositor.set_property("background", 0u32);
	compositor.set_property("width", composite_width);
	compositor.set_property("height", composite_height);

	let videoconvert = gstreamer::ElementFactory::make("videoconvert")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videoconvert. Error: {}", e))?;

	let encoder = gstreamer::ElementFactory::make("x264enc")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create x264enc element. Ensure gst-plugins-bad is installed and x264enc element is available.\nError: {}", e))?;

	apply_encoder_config(&encoder, &recording_config);

	let muxer = gstreamer::ElementFactory::make("mp4mux")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create mp4mux. Error: {}", e))?;

	let sink = gstreamer::ElementFactory::make("filesink")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create filesink. Error: {}", e))?;

	sink.set_property("location", output_path.to_string_lossy().as_ref());

	pipeline.add_many(&[&compositor, &videoconvert, &encoder, &muxer, &sink])?;
	gstreamer::Element::link_many(&[&compositor, &videoconvert, &encoder, &muxer, &sink])?;

	for (i, output) in intersecting_outputs.iter().enumerate() {
		let appsrc = gstreamer::ElementFactory::make("appsrc")
			.name(&format!("src_{}", i))
			.build()
			.map_err(|e| anyhow::anyhow!("Failed to create appsrc. Error: {}", e))?;

		appsrc.set_property("format", gstreamer::Format::Time);
		appsrc.set_property("is-live", true);

		pipeline.add(&appsrc)?;

		let sink_pad = compositor
			.request_pad_simple(&format!("sink_{}", i))
			.ok_or_else(|| anyhow::anyhow!("Failed to get sink pad"))?;

		let src_pad = appsrc
			.static_pad("src")
			.ok_or_else(|| anyhow::anyhow!("Failed to get src pad"))?;

		src_pad.link(&sink_pad)?;

		let rel_x = output.logical_position.x - region.position.x;
		let rel_y = output.logical_position.y - region.position.y;

		sink_pad.set_property("xpos", rel_x * max_scale);
		sink_pad.set_property("ypos", rel_y * max_scale);
		sink_pad.set_property("width", output.logical_size.width as i32 * max_scale);
		sink_pad.set_property("height", output.logical_size.height as i32 * max_scale);
		sink_pad.set_property("zorder", 0i32);

		appsrcs.push((appsrc, output.clone()));
	}

	pipeline.set_state(gstreamer::State::Ready)?;

	let mut frame_formats = Vec::with_capacity(num_outputs);
	for (i, format_receiver) in format_receivers.iter().enumerate() {
		let frame_format = format_receiver
			.recv()
			.map_err(|_| anyhow::anyhow!("Failed to receive initial format"))?;
		frame_formats.push(frame_format.clone());

		let gst_format = match frame_format.format {
			PixelFormat::Argb8888 => gstreamer_video::VideoFormat::Bgra,
			PixelFormat::Xrgb8888 => gstreamer_video::VideoFormat::Bgrx,
			PixelFormat::Abgr8888 => gstreamer_video::VideoFormat::Rgba,
			PixelFormat::Xbgr8888 => gstreamer_video::VideoFormat::Rgbx,
			_ => {
				return Err(anyhow::anyhow!(
					"Unsupported pixel format for recording: {:?}",
					frame_format.format
				));
			}
		};

		let caps = gstreamer_video::VideoCapsBuilder::new()
			.format(gst_format)
			.width(frame_format.width)
			.height(frame_format.height)
			.framerate(gstreamer::Fraction::new(0, 1))
			.build();

		appsrcs[i].0.set_property("caps", &caps);
	}

	pipeline.set_state(gstreamer::State::Playing)?;

	let mut start_pts = None;
	let mut previous_pts_vec: Vec<Option<u64>> = vec![None; num_outputs];

	let mut select = crossbeam_channel::Select::new();
	for i in 0..num_outputs {
		select.recv(&frame_receivers[i]);
	}
	select.recv(&stop_receiver);

	loop {
		let oper = select.select();
		let index = oper.index();

		if index == num_outputs {
			let _ = oper.recv(&stop_receiver);
			break;
		} else {
			let i = index;
			if let Ok((mmap, buffer_idx, pts, _frame_format)) = oper.recv(&frame_receivers[i]) {
				let (appsrc, _) = &appsrcs[i];

				if start_pts.is_none() {
					start_pts = Some(pts);
				}

				let relative_pts = pts.saturating_sub(start_pts.unwrap());

				let appsrc_downcast = appsrc
					.clone()
					.downcast::<AppSrc>()
					.map_err(|_| anyhow::anyhow!("Failed to cast to AppSrc"))?;

				push_buffer(&appsrc_downcast, &mmap, relative_pts, previous_pts_vec[i])?;

				let _ = return_senders[i].send(buffer_idx);

				previous_pts_vec[i] = Some(relative_pts);
			}
		}
	}

	for (appsrc, _) in &appsrcs {
		let appsrc_ref = appsrc.clone();
		appsrc_ref
			.downcast::<AppSrc>()
			.map_err(|_| anyhow::anyhow!("Failed to cast to AppSrc"))?
			.end_of_stream()?;
	}

	wait_for_gstreamer_eos(&pipeline)?;
	Ok(())
}
