//! The persistent Host must not retain any handles owned by its submitting CLI.
use std::io;

#[cfg(not(windows))]
pub(crate) fn spawn() -> io::Result<()> {
    use std::process::{Command, Stdio};
    Command::new(std::env::current_exe()?)
        .args(["host", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn spawn() -> io::Result<()> {
    spawn_detached(&std::env::current_exe()?, "host run")?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached(
    executable: &std::path::Path,
    arguments: &str,
) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    let application: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Windows paths cannot contain quotes. The argument tail is internal and
    // fixed by the caller, never assembled from notification contents.
    let mut command_line = OsString::from("\"");
    command_line.push(executable);
    command_line.push("\" ");
    command_line.push(arguments);
    let mut command_line: Vec<u16> = command_line.encode_wide().chain(Some(0)).collect();
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // A detached process has no console or inherited standard streams. Setting
    // inherit_handles to FALSE also excludes arbitrary inheritable pipes/files,
    // which redirecting stdin/stdout/stderr alone does not accomplish.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    // Own both returned handles immediately, including the unused main thread.
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess) };
    let _thread_handle = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
    Ok(process_handle)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

    fn wait_for(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(path.exists(), "missing child marker: {}", path.display());
    }

    #[test]
    fn detached_host_does_not_retain_cli_streams_or_other_handles() {
        let directory = tempfile::tempdir().unwrap();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let event = unsafe { CreateEventW(&attributes, 1, 0, std::ptr::null()) };
        assert!(!event.is_null());
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let mut bootstrap = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host_process::tests::bootstrap_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("POPU_HANDLE_TEST_DIR", directory.path())
            .env(
                "POPU_HANDLE_TEST_EVENT",
                (event.as_raw_handle() as usize).to_string(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = bootstrap.stdout.take().unwrap();
        let stderr = bootstrap.stderr.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut output = String::new();
            let result = {
                let mut stdout = stdout;
                stdout.read_to_string(&mut output)
            };
            let _ = sender.send(result.map(|_| output));
        });
        let bootstrap_status = bootstrap.wait().unwrap();
        let eof = receiver.recv_timeout(Duration::from_secs(2));
        // Always release the probe before asserting, even against a broken
        // launcher that leaves the reader waiting for inherited pipe EOF.
        let ready = directory.path().join("ready");
        wait_for(&ready);
        let event_state = unsafe { WaitForSingleObject(event.as_raw_handle(), 0) };
        let child_was_alive = !directory.path().join("finished").exists();
        std::fs::write(directory.path().join("release"), "").unwrap();
        wait_for(&directory.path().join("finished"));
        reader.join().unwrap();
        let mut errors = String::new();
        let mut stderr = stderr;
        stderr.read_to_string(&mut errors).unwrap();
        assert!(bootstrap_status.success(), "bootstrap failed: {errors}");
        assert!(eof.is_ok(), "CLI stdout stayed open while the Host lived");
        assert!(eof.unwrap().is_ok());
        assert!(child_was_alive, "Host exited before the EOF assertion");
        assert_eq!(
            event_state, WAIT_TIMEOUT,
            "Host inherited an unrelated event handle"
        );
    }

    #[test]
    #[ignore = "subprocess helper for the handle inheritance regression"]
    fn bootstrap_probe() {
        if std::env::var_os("POPU_HANDLE_TEST_DIR").is_none() {
            return;
        }
        spawn_detached(
            &std::env::current_exe().unwrap(),
            "--exact host_process::tests::host_probe --ignored --nocapture",
        )
        .unwrap();
    }

    #[test]
    #[ignore = "subprocess helper for the handle inheritance regression"]
    fn host_probe() {
        let Some(directory) = std::env::var_os("POPU_HANDLE_TEST_DIR") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let parent_event = std::env::var("POPU_HANDLE_TEST_EVENT")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        // If this value refers to the inherited parent event, the parent sees
        // it signalled. A reused local handle cannot signal that parent object.
        unsafe {
            SetEvent(parent_event as _);
        }
        std::fs::write(directory.join("ready"), "").unwrap();
        wait_for(&directory.join("release"));
        std::fs::write(directory.join("finished"), "").unwrap();
    }
}
