use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use anyhow::{Context, Result};

const PID_FILE_NAME: &str = "framr-recording.pid";

fn pid_file_path() -> Result<PathBuf> {
	let cache_dir =
		dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("Failed to get cache directory"))?;
	Ok(cache_dir.join(PID_FILE_NAME))
}

pub fn try_acquire_lock() -> Result<()> {
	let pid_path = pid_file_path()?;

	let mut file = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.open(&pid_path)
		.with_context(|| format!("Failed to open PID file: {}", pid_path.display()))?;

	let fd = file.as_raw_fd();

	let lock_res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };

	if lock_res != 0 {
		return Err(anyhow::anyhow!("Recording is already running"));
	}

	file.set_len(0).context("Failed to truncate PID file")?;

	let pid = unsafe { libc::getpid() };
	write!(file, "{}", pid).context("Failed to write PID to file")?;

	std::mem::forget(file);

	Ok(())
}

pub fn stop_recording() -> Result<()> {
	let pid_path = pid_file_path()?;

	if !pid_path.exists() {
		return Ok(());
	}

	let pid_content = std::fs::read_to_string(&pid_path)
		.with_context(|| format!("Failed to read PID file: {}", pid_path.display()))?;

	let pid: u32 = pid_content
		.trim()
		.parse()
		.with_context(|| format!("Invalid PID in file: {}", pid_content))?;

	unsafe {
		if libc::kill(pid as i32, libc::SIGINT) == 0 {
			println!("Stopping recording (PID: {})", pid);
		} else {
			let errno = *libc::__errno_location();
			if errno == libc::ESRCH {
				return Ok(());
			}
			return Err(anyhow::anyhow!("Failed to stop recording: errno {}", errno));
		}
	}

	Ok(())
}
