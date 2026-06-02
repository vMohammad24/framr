use anyhow::Result;
use libframr::{FramrConnection, RecordingConfig};
use std::path::PathBuf;

use crate::app::capture::resolve_output;
use crate::cli::Cli;
use crate::config::AppConfig;
use crate::pidfile;
use crate::selection;

pub fn get_recording_config(cli: &Cli, cfg: Option<&AppConfig>) -> RecordingConfig {
	let base_config = cfg.map(|c| c.recording).unwrap_or_default();

	RecordingConfig {
		bitrate: cli.bitrate.unwrap_or(base_config.bitrate),
		keyframe_interval: cli
			.keyframe_interval
			.unwrap_or(base_config.keyframe_interval),
		threads: cli.threads.filter(|&t| t != 0).or(base_config.threads),
		tune: cli.tune.unwrap_or(base_config.tune),
		speed_preset: cli.speed_preset.unwrap_or(base_config.speed_preset),
	}
}

pub fn record(
	cli: &Cli,
	cfg: Option<&AppConfig>,
	is_upload_action: bool,
) -> Result<Option<(PathBuf, String)>> {
	if pidfile::try_acquire_lock().is_err() {
		pidfile::stop_recording()?;
		return Ok(None);
	}

	let conn = FramrConnection::new()?;
	let recording_config = get_recording_config(cli, cfg);

	let filename = resolve_output(cli, "recording_%Y-%m-%d_%H-%M-%S.mp4", "mp4")
		.to_string_lossy()
		.to_string();

	let path = if is_upload_action && cli.output.is_none() {
		std::env::temp_dir().join(&filename)
	} else {
		let p = match &cli.output {
			Some(dir) => dir.join(&filename),
			None => PathBuf::from(&filename),
		};
		if let Some(parent) = p.parent() {
			std::fs::create_dir_all(parent)?;
		}
		p
	};

	let handle = if let Some(screen_num) = cli.screen {
		let output = conn.get_output(screen_num)?;
		conn.start_recording(&output, None, cli.cursor, path.clone(), recording_config)?
	} else {
		let mut selection_cfg = cfg.map(|c| c.selection).unwrap_or_default();
		selection_cfg.show_toolbar = false;
		let ui = selection::SelectionUI::new(selection_cfg)?;
		let (region, _) = ui
			.run(false)?
			.ok_or_else(|| anyhow::anyhow!("Selection cancelled"))?;

		conn.start_recording_region(&region, cli.cursor, path.clone(), recording_config)?
	};

	println!("Recording to {}... Press Ctrl+C to stop.", path.display());

	let (tx, rx) = std::sync::mpsc::channel();
	ctrlc::set_handler(move || {
		let _ = tx.send(());
	})?;

	loop {
		if rx
			.recv_timeout(std::time::Duration::from_millis(100))
			.is_ok()
		{
			println!("\nStopping recording...");
			break;
		}
		if handle.pipeline_thread.is_finished() {
			println!("\nRecording stopped unexpectedly.");
			break;
		}
	}

	let _ = handle.stop_sender.send(());
	handle
		.pipeline_thread
		.join()
		.map_err(|_| anyhow::anyhow!("Pipeline thread panicked"))??;

	Ok(Some((path, filename)))
}
