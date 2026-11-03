//! Start only an absent Host; never replay a submitted business request.
use std::io;
use std::time::{Duration, Instant};

use crate::{host_process, ipc};

pub(crate) fn call(request: &ipc::RpcRequest) -> io::Result<ipc::RpcResponse> {
    let stream = connect_or_start(ipc::connect, host_process::spawn, Duration::from_secs(3))?;
    ipc::call_connected(stream, request)
}

fn absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

fn connect_or_start<T>(
    mut connect: impl FnMut() -> io::Result<T>,
    spawn: impl FnOnce() -> io::Result<()>,
    timeout: Duration,
) -> io::Result<T> {
    match connect() {
        Ok(stream) => return Ok(stream),
        Err(error) if absent(&error) => {}
        Err(error) => return Err(error),
    }
    spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match connect() {
            Ok(stream) => return Ok(stream),
            Err(error) if absent(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("Host did not become ready: {error}"),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn existing_host_is_used_without_starting_another() {
        assert_eq!(
            connect_or_start(|| Ok(7), || panic!("must not spawn"), Duration::ZERO).unwrap(),
            7
        );
    }

    #[test]
    fn connection_errors_other_than_absence_do_not_start_a_host() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidData,
            io::ErrorKind::BrokenPipe,
        ] {
            let error = connect_or_start::<()>(
                || Err(kind.into()),
                || panic!("must not spawn"),
                Duration::ZERO,
            )
            .unwrap_err();
            assert_eq!(error.kind(), kind);
        }
    }

    #[test]
    fn absent_host_is_started_once_and_only_connection_is_retried() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::ConnectionRefused] {
            let attempts = Cell::new(0);
            let starts = Cell::new(0);
            let value = connect_or_start(
                || {
                    attempts.set(attempts.get() + 1);
                    if attempts.get() < 3 {
                        Err(kind.into())
                    } else {
                        Ok(42)
                    }
                },
                || {
                    starts.set(starts.get() + 1);
                    Ok(())
                },
                Duration::from_secs(1),
            )
            .unwrap();
            assert_eq!(value, 42);
            assert_eq!(starts.get(), 1);
            assert_eq!(attempts.get(), 3);
        }
    }

    #[test]
    fn preflight_failure_is_returned_without_further_connections() {
        let attempts = Cell::new(0);
        let error = connect_or_start::<()>(
            || {
                attempts.set(attempts.get() + 1);
                Err(io::ErrorKind::NotFound.into())
            },
            || {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "WebView2 installation link",
                ))
            },
            Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(attempts.get(), 1);
        assert_eq!(error.to_string(), "WebView2 installation link");
    }

    #[test]
    fn startup_has_a_bounded_connection_wait() {
        let error = connect_or_start::<()>(
            || Err(io::ErrorKind::NotFound.into()),
            || Ok(()),
            Duration::ZERO,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Host did not become ready"));
    }
}
