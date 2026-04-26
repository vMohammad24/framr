use std::ffi::CString;
use std::os::unix::io::OwnedFd;

use rustix::fs::{MemfdFlags, SealFlags};

pub fn create_shm_fd() -> std::io::Result<OwnedFd> {
	loop {
		match rustix::fs::memfd_create(
			CString::new("libframr")?.as_c_str(),
			MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
		) {
			Ok(fd) => {
				let _ = rustix::fs::fcntl_add_seals(&fd, SealFlags::SHRINK | SealFlags::SEAL);
				return Ok(fd);
			}
			Err(rustix::io::Errno::INTR) => continue,
			Err(rustix::io::Errno::NOSYS) => break,
			Err(e) => return Err(std::io::Error::from(e)),
		}
	}

	fallback_shm_open()
}

fn fallback_shm_open() -> std::io::Result<OwnedFd> {
	use std::time::SystemTime;

	let id = SystemTime::now()
		.duration_since(SystemTime::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();

	let name = format!("/libframr-{id}");

	let flags = rustix::shm::OFlags::CREATE | rustix::shm::OFlags::EXCL | rustix::shm::OFlags::RDWR;
	let mode = rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR;

	let fd = rustix::shm::open(&name, flags, mode)?;
	let _ = rustix::shm::unlink(&name);
	Ok(fd)
}
