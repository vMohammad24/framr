use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use rodio::{DeviceSinkBuilder, Player, buffer::SamplesBuffer};
use std::io::Read;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};

const SOUND_URL: &str = "https://cdn.nest.rip/uploads/20ec5f7b-5b80-4fe0-abca-a8beb4453743.wav";

fn get_config_dir() -> Result<PathBuf> {
	dirs::config_local_dir()
		.map(|p| p.join(env!("CARGO_PKG_NAME")))
		.ok_or_else(|| anyhow::anyhow!("Failed to get config directory"))
}

fn ensure_sound_file() -> Result<PathBuf> {
	let config_dir = get_config_dir().context("Failed to get config directory")?;
	std::fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

	let sound_path = config_dir.join("sound.wav");

	if sound_path.exists() {
		return Ok(sound_path);
	}

	let config = ureq::config::Config::builder()
		.timeout_global(Some(std::time::Duration::from_secs(10)))
		.build();
	let agent: ureq::Agent = config.into();

	let mut response = agent
		.get(SOUND_URL)
		.call()
		.context("Failed to download sound file")?;

	let mut sound_data = Vec::new();
	response
		.body_mut()
		.as_reader()
		.read_to_end(&mut sound_data)
		.context("Failed to read sound file data")?;

	std::fs::write(&sound_path, sound_data).context("Failed to write sound file")?;

	Ok(sound_path)
}

pub fn init_sound() {
	let _ = ensure_sound_file();
}

fn append_decoded_frames(
	decoder: &mut ffmpeg::codec::decoder::Audio,
	resampler: &mut ffmpeg::software::resampling::Context,
	samples: &mut Vec<f32>,
) -> Result<()> {
	let mut decoded = ffmpeg::frame::Audio::empty();

	loop {
		match decoder.receive_frame(&mut decoded) {
			Ok(()) => {
				let mut converted = ffmpeg::frame::Audio::empty();
				resampler
					.run(&decoded, &mut converted)
					.context("Failed to convert decoded audio")?;
				samples.extend_from_slice(converted.plane::<f32>(0));
			}
			Err(ffmpeg::Error::Eof) => break,
			Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => break,
			Err(error) => return Err(error).context("Failed to decode audio frame"),
		}
	}

	Ok(())
}

fn flush_resampler(
	resampler: &mut ffmpeg::software::resampling::Context,
	samples: &mut Vec<f32>,
) -> Result<()> {
	while let Some(delay) = resampler.delay() {
		let output = *resampler.output();
		let output_samples = usize::try_from(delay.output).context("Invalid resampler delay")?;
		let mut converted =
			ffmpeg::frame::Audio::new(output.format, output_samples, output.channel_layout);
		let remaining = resampler
			.flush(&mut converted)
			.context("Failed to flush converted audio")?;
		samples.extend_from_slice(converted.plane::<f32>(0));

		if remaining.is_none() {
			break;
		}
	}

	Ok(())
}

fn decode_sound(path: &Path) -> Result<SamplesBuffer> {
	ffmpeg::init().context("Failed to initialize FFmpeg")?;

	let mut input = ffmpeg::format::input(path).context("Failed to open sound file")?;
	let stream = input
		.streams()
		.best(ffmpeg::media::Type::Audio)
		.context("Sound file contains no audio stream")?;
	let stream_index = stream.index();
	let decoder_context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
		.context("Failed to create audio decoder")?;
	let mut decoder = decoder_context
		.decoder()
		.audio()
		.context("Failed to open audio decoder")?;

	let channels = decoder.channels();
	let channel_layout = if decoder.channel_layout().is_empty() {
		ffmpeg::ChannelLayout::default(i32::from(channels))
	} else {
		decoder.channel_layout()
	};
	let sample_rate = decoder.rate();
	let output_format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
	let mut resampler = ffmpeg::software::resampling::Context::get(
		decoder.format(),
		channel_layout,
		sample_rate,
		output_format,
		channel_layout,
		sample_rate,
	)
	.context("Failed to create audio sample converter")?;
	let mut samples = Vec::new();

	for (stream, packet) in input.packets() {
		if stream.index() != stream_index {
			continue;
		}

		decoder
			.send_packet(&packet)
			.context("Failed to send audio packet to decoder")?;
		append_decoded_frames(&mut decoder, &mut resampler, &mut samples)?;
	}

	decoder
		.send_eof()
		.context("Failed to finish audio decoding")?;
	append_decoded_frames(&mut decoder, &mut resampler, &mut samples)?;
	flush_resampler(&mut resampler, &mut samples)?;

	let channels = NonZeroU16::new(channels).context("Audio stream has no channels")?;
	let sample_rate = NonZeroU32::new(sample_rate).context("Audio stream has no sample rate")?;

	Ok(SamplesBuffer::new(channels, sample_rate, samples))
}

pub fn play_sound(sound_path: &str) -> Result<()> {
	let path = Path::new(sound_path);

	if !path.exists() {
		return Ok(());
	}

	let sound = decode_sound(path)?;

	let mut sink =
		DeviceSinkBuilder::open_default_sink().context("Failed to open default audio output")?;

	sink.log_on_drop(false);

	let player = Player::connect_new(sink.mixer());
	player.append(sound);

	player.sleep_until_end();

	Ok(())
}
