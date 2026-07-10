use std::fs::{OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

const PID_FILE_NAME: &str = "framr-recording.pid";

fn pid_file_path() -> Result<PathBuf> {
	let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
	Ok(dir.join(PID_FILE_NAME))
}

pub struct RecordingLock {
	file: std::fs::File,
	path: PathBuf,
}

impl Drop for RecordingLock {
	fn drop(&mut self) {
		let _ = self.file.flush();
		let _ = std::fs::remove_file(&self.path);
	}
}

pub fn try_acquire_lock() -> Result<RecordingLock> {
	let pid_path = pid_file_path()?;
	let mut file = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(&pid_path)
		.with_context(|| format!("Failed to open PID file: {}", pid_path.display()))?;

	match file.try_lock() {
		Ok(()) => {}
		Err(TryLockError::WouldBlock) => {
			return Err(anyhow::anyhow!("Recording is already running"));
		}
		Err(TryLockError::Error(err)) => {
			return Err(err).context("Failed to acquire lock on PID file");
		}
	}

	file.set_len(0).context("Failed to truncate PID file")?;
	write!(file, "{}", std::process::id()).context("Failed to write PID to file")?;
	file.flush().context("Failed to flush PID file")?;

	Ok(RecordingLock {
		file,
		path: pid_path,
	})
}

pub fn stop_recording() -> Result<()> {
	let pid_path = pid_file_path()?;

	let mut file = match OpenOptions::new().read(true).open(&pid_path) {
		Ok(f) => f,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
		Err(e) => {
			return Err(e)
				.with_context(|| format!("Failed to open PID file: {}", pid_path.display()));
		}
	};

	match file.try_lock() {
		Ok(()) => {
			drop(file);
			let _ = std::fs::remove_file(&pid_path);
			return Ok(());
		}
		Err(TryLockError::WouldBlock) => {}
		Err(TryLockError::Error(err)) => {
			return Err(err).context("Failed to probe PID file lock");
		}
	}

	let mut pid_content = String::new();
	file.read_to_string(&mut pid_content)
		.with_context(|| format!("Failed to read PID file: {}", pid_path.display()))?;
	let pid: i32 = pid_content
		.trim()
		.parse()
		.with_context(|| format!("Invalid PID in file: {}", pid_content))?;

	if pid <= 0 {
		return Err(anyhow::anyhow!("Refusing to signal invalid PID: {pid}"));
	}

	let res = unsafe { libc::kill(pid, libc::SIGINT) };
	if res == 0 {
		println!("Stopping recording (PID: {pid})");
		Ok(())
	} else {
		let err = std::io::Error::last_os_error();
		if err.raw_os_error() == Some(libc::ESRCH) {
			return Ok(());
		}
		Err(err).context("Failed to stop recording")
	}
}
