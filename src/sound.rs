use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::io::Read;
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

pub fn play_sound(sound_path: &str) -> Result<()> {
	let path = Path::new(sound_path);

	if !path.exists() {
		return Ok(());
	}

	let abs_path = std::fs::canonicalize(path).context("Failed to get absolute path")?;

	gst::init()?;

	let path_str = abs_path
		.to_str()
		.ok_or_else(|| anyhow::anyhow!("Invalid path"))?;

	let pipeline = gst::parse::launch(&format!(
		"filesrc location=\"{}\" ! decodebin ! audioconvert ! audioresample ! autoaudiosink",
		path_str
	))
	.context("Failed to create GStreamer pipeline")?;

	pipeline
		.set_state(gst::State::Playing)
		.context("Failed to set pipeline to playing")?;

	let bus = pipeline.bus().context("Failed to get bus")?;

	for msg in bus.iter_timed(gst::ClockTime::NONE) {
		use gst::MessageView;

		match msg.view() {
			MessageView::Eos(..) => break,
			MessageView::Error(err) => {
				pipeline.set_state(gst::State::Null)?;
				anyhow::bail!(
					"GStreamer error: {} ({})",
					err.error(),
					err.debug().map(|s| s.to_string()).unwrap_or_default()
				);
			}
			_ => (),
		}
	}

	pipeline.set_state(gst::State::Null)?;

	Ok(())
}
