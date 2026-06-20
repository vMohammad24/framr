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

fn encoder_min_dimensions(encoder: &gstreamer::Element) -> (i32, i32) {
	let (mut min_w, mut min_h) = (2, 2);
	if let Some(tmpl) = encoder.pad_template("sink") {
		for s in tmpl.caps().iter() {
			if let Ok(r) = s.get::<gstreamer::IntRange<i32>>("width") {
				min_w = min_w.max(r.min());
			}
			if let Ok(r) = s.get::<gstreamer::IntRange<i32>>("height") {
				min_h = min_h.max(r.min());
			}
		}
	}
	(min_w, min_h)
}

fn fit_encoder_dimensions(encoder: &gstreamer::Element, width: i32, height: i32) -> (i32, i32) {
	let (min_w, min_h) = encoder_min_dimensions(encoder);
	let scale = (min_w as f64 / width as f64)
		.max(min_h as f64 / height as f64)
		.max(1.0);
	let scaled_w = (width as f64 * scale).ceil() as i32;
	let scaled_h = (height as f64 * scale).ceil() as i32;
	let even_w = ((scaled_w + 1) / 2 * 2).max(min_w);
	let even_h = ((scaled_h + 1) / 2 * 2).max(min_h);
	(even_w, even_h)
}

fn apply_encoder_config(encoder: &gstreamer::Element, config: &RecordingConfig) {
	let Some(factory) = encoder.factory() else {
		return;
	};
	let name = factory.name();

	match config.encoder {
		crate::VideoEncoder::H264 => {
			encoder.set_property("bitrate", config.bitrate);
			if name == "x264enc" {
				encoder.set_property("speed-preset", config.speed.to_gst_value());
				encoder.set_property("key-int-max", config.keyframe_interval);
				if config.tune.is_psy_tune() {
					encoder.set_property("psy-tune", config.tune.to_gst_value());
				} else {
					encoder.set_property("tune", config.tune.to_gst_value());
				}
			} else if name.starts_with("vaapi") || name.starts_with("va") {
				if encoder.has_property("keyframe-period") {
					encoder.set_property("keyframe-period", config.keyframe_interval as i32);
				}
				if encoder.has_property("rate-control") {
					encoder.set_property_from_str("rate-control", "cbr");
				}
			} else if name.starts_with("nv") {
				if encoder.has_property("gop-size") {
					encoder.set_property("gop-size", config.keyframe_interval as i32);
				}
				if encoder.has_property("rc-mode") {
					encoder.set_property_from_str("rc-mode", "cbr");
				}
			}
		}
		crate::VideoEncoder::AV1 => {
			if name == "rav1enc" {
				let speed = 11 - config.speed.to_gst_value();
				encoder.set_property("speed-preset", speed as u32);
				encoder.set_property("bitrate", (config.bitrate * 1000) as i32);
				encoder.set_property("max-key-frame-interval", config.keyframe_interval as u64);
			} else {
				encoder.set_property("bitrate", config.bitrate);
				if name.starts_with("vaapi") || name.starts_with("va") {
					if encoder.has_property("keyframe-period") {
						encoder.set_property("keyframe-period", config.keyframe_interval as i32);
					}
					if encoder.has_property("rate-control") {
						encoder.set_property_from_str("rate-control", "cbr");
					}
				} else if name.starts_with("nv") {
					if encoder.has_property("gop-size") {
						encoder.set_property("gop-size", config.keyframe_interval as i32);
					}
					if encoder.has_property("rc-mode") {
						encoder.set_property_from_str("rc-mode", "cbr");
					}
				}
			}
		}
	}

	if encoder.has_property("threads") {
		encoder.set_property("threads", config.threads.unwrap_or(0));
	}
}

fn push_buffer(appsrc: &AppSrc, data: &[u8], pts: u64, previous_pts: Option<u64>) -> Result<()> {
	let mut buffer = gstreamer::Buffer::with_size(data.len())
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
			.copy_from_slice(0, data)
			.map_err(|e| anyhow::anyhow!("copy_from_slice failed: {e}"))?;
	}

	appsrc.push_buffer(buffer)?;
	Ok(())
}

fn configure_appsrc(appsrc: &AppSrc) {
	appsrc.set_format(gstreamer::Format::Time);
	appsrc.set_is_live(false);
	appsrc.set_do_timestamp(false);
	appsrc.set_max_bytes(1024 * 1024 * 300);
	appsrc.set_property("block", true);
	appsrc.set_property("min-percent", 50u32);
}

pub fn run_single_encoding_pipeline(
	transform: Transform,
	output_path: std::path::PathBuf,
	frame_receiver: crossbeam_channel::Receiver<(Arc<Mmap>, usize, u64, FrameFormat)>,
	return_sender: crossbeam_channel::Sender<usize>,
	recording_config: RecordingConfig,
) -> Result<()> {
	let pipeline = gstreamer::Pipeline::new();

	let appsrc = gstreamer::ElementFactory::make("appsrc")
		.name("src")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create appsrc. Error: {}", e))?
		.dynamic_cast::<AppSrc>()
		.unwrap();

	configure_appsrc(&appsrc);

	let videoconvert = gstreamer::ElementFactory::make("videoconvert")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videoconvert. Error: {}", e))?;

	let videorate = gstreamer::ElementFactory::make("videorate")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videorate. Error: {}", e))?;
	videorate.set_property("skip-to-first", true);

	let videoflip = gstreamer::ElementFactory::make("videoflip")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videoflip. Error: {}", e))?;

	let direction_nick = match transform {
		Transform::Normal => "identity",
		Transform::_90 => "90r",
		Transform::_180 => "180",
		Transform::_270 => "90l",
		Transform::Flipped => "horiz",
		Transform::Flipped90 => "urd",
		Transform::Flipped180 => "vert",
		Transform::Flipped270 => "uld",
	};

	videoflip.set_property_from_str("video-direction", direction_nick);

	let videoscale = gstreamer::ElementFactory::make("videoscale")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videoscale. Error: {}", e))?;

	let capsfilter = gstreamer::ElementFactory::make("capsfilter")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create capsfilter. Error: {}", e))?;

	let hw_encoder = crate::find_hardware_encoder(
		recording_config.encoder,
		recording_config.hw_encoder.as_deref(),
	);
	let encoder_name = hw_encoder
		.as_deref()
		.unwrap_or_else(|| match recording_config.encoder {
			crate::VideoEncoder::H264 => "x264enc",
			crate::VideoEncoder::AV1 => "rav1enc",
		});

	let encoder = gstreamer::ElementFactory::make(encoder_name)
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create encoder {}. Error: {}", encoder_name, e))?;

	apply_encoder_config(&encoder, &recording_config);

	let muxer_name = recording_config.container.gst_muxer();
	let muxer = gstreamer::ElementFactory::make(muxer_name)
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create muxer {}. Error: {}", muxer_name, e))?;

	let sink = gstreamer::ElementFactory::make("filesink")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create filesink. Error: {}", e))?;

	sink.set_property("location", output_path.to_string_lossy().as_ref());

	pipeline.add_many(&[
		appsrc.upcast_ref(),
		&videoconvert,
		&videorate,
		&videoflip,
		&videoscale,
		&capsfilter,
		&encoder,
		&muxer,
		&sink,
	])?;
	gstreamer::Element::link_many(&[
		appsrc.upcast_ref(),
		&videoconvert,
		&videorate,
		&videoflip,
		&videoscale,
		&capsfilter,
		&encoder,
		&muxer,
		&sink,
	])?;

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
		.framerate(gstreamer::Fraction::new(recording_config.fps as i32, 1))
		.build();

	appsrc.set_caps(Some(&caps));

	let (target_width, target_height) =
		fit_encoder_dimensions(&encoder, format.width, format.height);
	let scaled_caps = gstreamer_video::VideoCapsBuilder::new()
		.width(target_width)
		.height(target_height)
		.build();
	capsfilter.set_property("caps", &scaled_caps);

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

	let videorate = gstreamer::ElementFactory::make("videorate")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create videorate. Error: {}", e))?;
	videorate.set_property("skip-to-first", true);

	let hw_encoder = crate::find_hardware_encoder(
		recording_config.encoder,
		recording_config.hw_encoder.as_deref(),
	);
	let encoder_name = hw_encoder
		.as_deref()
		.unwrap_or_else(|| match recording_config.encoder {
			crate::VideoEncoder::H264 => "x264enc",
			crate::VideoEncoder::AV1 => "rav1enc",
		});

	let encoder = gstreamer::ElementFactory::make(encoder_name)
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create encoder {}. Error: {}", encoder_name, e))?;

	apply_encoder_config(&encoder, &recording_config);

	let muxer_name = recording_config.container.gst_muxer();
	let muxer = gstreamer::ElementFactory::make(muxer_name)
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create muxer {}. Error: {}", muxer_name, e))?;

	let sink = gstreamer::ElementFactory::make("filesink")
		.build()
		.map_err(|e| anyhow::anyhow!("Failed to create filesink. Error: {}", e))?;

	sink.set_property("location", output_path.to_string_lossy().as_ref());

	pipeline.add_many(&[
		&compositor,
		&videoconvert,
		&videorate,
		&encoder,
		&muxer,
		&sink,
	])?;
	gstreamer::Element::link_many(&[
		&compositor,
		&videoconvert,
		&videorate,
		&encoder,
		&muxer,
		&sink,
	])?;

	for (i, output) in intersecting_outputs.iter().enumerate() {
		let appsrc = gstreamer::ElementFactory::make("appsrc")
			.name(&format!("src_{}", i))
			.build()
			.map_err(|e| anyhow::anyhow!("Failed to create appsrc. Error: {}", e))?
			.dynamic_cast::<AppSrc>()
			.unwrap();

		configure_appsrc(&appsrc);

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

		appsrcs.push((appsrc, output));
	}

	pipeline.set_state(gstreamer::State::Ready)?;

	for (i, format_receiver) in format_receivers.iter().enumerate() {
		let frame_format = format_receiver
			.recv()
			.map_err(|_| anyhow::anyhow!("Failed to receive initial format"))?;

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
			.framerate(gstreamer::Fraction::new(recording_config.fps as i32, 1))
			.build();

		appsrcs[i].0.set_caps(Some(&caps));
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

				push_buffer(&appsrc, &mmap, relative_pts, previous_pts_vec[i])?;

				let _ = return_senders[i].send(buffer_idx);

				previous_pts_vec[i] = Some(relative_pts);
			}
		}
	}

	for (appsrc, _) in &appsrcs {
		appsrc.end_of_stream()?;
	}

	wait_for_gstreamer_eos(&pipeline)?;
	Ok(())
}
