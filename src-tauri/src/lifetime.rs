use std::io::Read;

use thiserror::Error;

#[cfg(windows)]
pub fn monitor(
    handle: u64,
    on_closed: impl FnOnce() + Send + 'static,
) -> Result<(), LifetimeError> {
    use std::fs::File;
    use std::os::windows::io::{FromRawHandle, RawHandle};

    if handle == 0 || handle == u64::MAX {
        return Err(LifetimeError::InvalidHandle);
    }
    let mut reader = unsafe { File::from_raw_handle(handle as RawHandle) };
    std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => {
                    on_closed();
                    return;
                }
                Ok(_) => {}
            }
        }
    });
    Ok(())
}

#[cfg(unix)]
pub fn monitor(
    handle: u64,
    on_closed: impl FnOnce() + Send + 'static,
) -> Result<(), LifetimeError> {
    use std::fs::File;
    use std::os::fd::{FromRawFd, RawFd};

    if handle > RawFd::MAX as u64 {
        return Err(LifetimeError::InvalidHandle);
    }
    let mut reader = unsafe { File::from_raw_fd(handle as RawFd) };
    std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => {
                    on_closed();
                    return;
                }
                Ok(_) => {}
            }
        }
    });
    Ok(())
}

#[derive(Debug, Error)]
pub enum LifetimeError {
    #[error("invalid lifetime handle")]
    InvalidHandle,
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::ptr;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Pipes::CreatePipe;

    #[test]
    fn closing_writer_notifies_monitor() {
        let mut reader: HANDLE = ptr::null_mut();
        let mut writer: HANDLE = ptr::null_mut();
        assert_ne!(
            unsafe { CreatePipe(&mut reader, &mut writer, ptr::null(), 0) },
            0
        );

        let (sender, receiver) = mpsc::channel();
        monitor(reader as usize as u64, move || sender.send(()).unwrap()).unwrap();
        unsafe { CloseHandle(writer) };

        receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("monitor did not observe pipe EOF");
    }
}
