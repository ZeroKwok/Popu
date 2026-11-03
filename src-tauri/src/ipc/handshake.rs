//! One handshake and one business request on the same connection.
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use interprocess::local_socket::{prelude::*, Stream};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{exchange, read_json, write_frame, RpcRequest, RpcResponse, RPC_VERSION};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const RECOVERY: &str = "Inspect `popu host status`, then exit the old Host from its tray menu or explicitly run `popu host stop` and retry. The Host was not automatically stopped.";

#[derive(Debug, Deserialize, Serialize)]
pub struct HostInfo {
    pub app_version: String,
    pub rpc_version: String,
    pub pid: u32,
    pub executable: PathBuf,
}

impl HostInfo {
    pub fn current() -> io::Result<Self> {
        Ok(Self {
            app_version: APP_VERSION.into(),
            rpc_version: RPC_VERSION.into(),
            pid: std::process::id(),
            executable: std::env::current_exe()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientHello {
    app_version: String,
}

fn hello_response(request: &RpcRequest) -> RpcResponse {
    let hello = serde_json::from_value::<ClientHello>(request.params.clone());
    let result = match hello {
        Ok(hello) if request.version == RPC_VERSION && hello.app_version == APP_VERSION => {
            HostInfo::current().map_err(|error| error.to_string())
        }
        Ok(hello) => Err(format!(
            "CLI app/RPC {}/{} does not match Host {APP_VERSION}/{RPC_VERSION}",
            hello.app_version, request.version
        )),
        Err(error) => Err(format!("invalid Host handshake: {error}")),
    };
    match result {
        Ok(info) => RpcResponse::success(info).unwrap(),
        Err(message) => {
            RpcResponse::failure(json!({"code":"incompatible_host", "message":message})).unwrap()
        }
    }
}

/// Management commands remain available without a handshake for recovery.
pub fn serve_connection<S: Read + Write>(
    stream: &mut S,
    mut dispatch: impl FnMut(RpcRequest) -> RpcResponse,
) -> io::Result<()> {
    let first: RpcRequest = match read_json(stream) {
        Ok(request) => request,
        Err(error) => {
            return write_frame(
                stream,
                &RpcResponse::failure(
                    json!({"code":"invalid_request", "message":error.to_string()}),
                )
                .unwrap(),
            )
        }
    };
    if first.method == "host.hello" {
        let response = hello_response(&first);
        let accepted = response.ok;
        write_frame(stream, &response)?;
        if !accepted {
            return Ok(());
        }
        let request = read_json(stream)?;
        write_frame(stream, &dispatch(request))
    } else if matches!(first.method.as_str(), "host.status" | "host.stop") {
        write_frame(stream, &dispatch(first))
    } else {
        write_frame(stream, &RpcResponse::failure(json!({"code":"handshake_required", "message":"Send host.hello before a business request; update the Popu CLI."})).unwrap())
    }
}

/// Handshake time is bounded; request.wait retains its unlimited business wait.
pub fn call_connected(mut stream: Stream, request: &RpcRequest) -> io::Result<RpcResponse> {
    handshake(&mut stream, HANDSHAKE_TIMEOUT)?;
    exchange(&mut stream, request)
}

fn handshake(stream: &mut Stream, timeout: Duration) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    let response = exchange(
        &mut DeadlineIo {
            stream,
            deadline: Instant::now() + timeout,
        },
        &RpcRequest {
            version: RPC_VERSION.into(),
            method: "host.hello".into(),
            params: json!({"app_version": APP_VERSION}),
        },
    );
    let reset = stream.set_nonblocking(false);
    let checked = (|| {
        let response = response?;
        reset?;
        if !response.ok {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Host rejected handshake: {}", response.error.unwrap()),
            ));
        }
        let info: HostInfo = serde_json::from_value(
            response.result.unwrap_or(serde_json::Value::Null),
        )
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid Host identity: {error}"),
            )
        })?;
        if info.app_version != APP_VERSION
            || info.rpc_version != RPC_VERSION
            || info.pid == 0
            || info.executable.as_os_str().is_empty()
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Host app/RPC {}/{} does not match CLI {APP_VERSION}/{RPC_VERSION}, or its identity is incomplete", info.app_version, info.rpc_version)));
        }
        Ok(())
    })();
    checked.map_err(|error: io::Error| {
        io::Error::new(
            error.kind(),
            format!("Host compatibility handshake failed: {error}. {RECOVERY}"),
        )
    })
}

// Named pipes lack receive/send timeouts. One nonblocking deadline preserves
// partial frames without leaving a blocked reader thread after timeout.
struct DeadlineIo<'a> {
    stream: &'a mut Stream,
    deadline: Instant,
}

impl DeadlineIo<'_> {
    fn retry<T>(
        &mut self,
        mut operation: impl FnMut(&mut Stream) -> io::Result<T>,
    ) -> io::Result<T> {
        loop {
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Host handshake timed out",
                ));
            }
            match operation(self.stream) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                result => return result,
            }
        }
    }
}

impl Read for DeadlineIo<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.retry(|stream| match stream.read(bytes) {
            // PIPE_NOWAIT byte pipes can report zero bytes while still open.
            // Do not let Read::read_exact mistake that for a truncated frame.
            #[cfg(windows)]
            Ok(0) if !bytes.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        })
    }
}

impl Write for DeadlineIo<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.retry(|stream| match stream.write(bytes) {
            #[cfg(windows)]
            Ok(0) if !bytes.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        self.retry(Write::flush)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };

    fn pair(server: impl FnOnce(Stream) + Send + 'static) -> (Stream, std::thread::JoinHandle<()>) {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = if GenericNamespaced::is_supported() {
            format!("popu-handshake-test-{}", uuid::Uuid::new_v4())
        } else {
            directory
                .path()
                .join("host.sock")
                .to_str()
                .unwrap()
                .to_owned()
        };
        let name = || {
            if GenericNamespaced::is_supported() {
                endpoint.as_str().to_ns_name::<GenericNamespaced>()
            } else {
                endpoint.as_str().to_fs_name::<GenericFilePath>()
            }
        };
        let listener = ListenerOptions::new()
            .name(name().unwrap())
            .create_sync()
            .unwrap();
        let worker = std::thread::spawn(move || {
            let _directory = directory;
            server(listener.accept().unwrap());
        });
        (Stream::connect(name().unwrap()).unwrap(), worker)
    }

    fn business() -> RpcRequest {
        RpcRequest {
            version: RPC_VERSION.into(),
            method: "request.create".into(),
            params: json!({}),
        }
    }

    #[test]
    fn compatible_session_dispatches_exactly_one_request_and_restores_blocking_io() {
        let (stream, worker) = pair(|mut stream| {
            let mut count = 0;
            serve_connection(&mut stream, |request| {
                count += 1;
                assert_eq!(request.method, "request.create");
                std::thread::sleep(Duration::from_millis(50));
                RpcResponse::success(json!({"accepted":true})).unwrap()
            })
            .unwrap();
            assert_eq!(count, 1);
        });
        let response = call_connected(stream, &business()).unwrap();
        assert_eq!(response.result.unwrap()["accepted"], true);
        worker.join().unwrap();
    }

    #[test]
    fn null_business_results_remain_valid() {
        let (stream, worker) = pair(|mut stream| {
            serve_connection(&mut stream, |_| {
                RpcResponse::success(serde_json::Value::Null).unwrap()
            })
            .unwrap();
        });
        let response = call_connected(stream, &business()).unwrap();
        assert!(response.ok);
        assert_eq!(
            response.result.unwrap_or(serde_json::Value::Null),
            serde_json::Value::Null
        );
        worker.join().unwrap();
    }

    #[test]
    fn host_rejects_incompatible_or_unhandshaken_clients_before_dispatch() {
        for request in [
            business(),
            RpcRequest {
                version: RPC_VERSION.into(),
                method: "host.hello".into(),
                params: json!({"app_version":"older"}),
            },
            RpcRequest {
                version: "future".into(),
                method: "host.hello".into(),
                params: json!({"app_version":APP_VERSION}),
            },
            RpcRequest {
                version: RPC_VERSION.into(),
                method: "host.hello".into(),
                params: json!({}),
            },
        ] {
            let (mut stream, worker) = pair(|mut stream| {
                serve_connection(&mut stream, |_| panic!("business request reached dispatch"))
                    .unwrap();
            });
            write_frame(&mut stream, &request).unwrap();
            let response: RpcResponse = read_json(&mut stream).unwrap();
            assert!(!response.ok);
            worker.join().unwrap();
        }
    }

    #[test]
    fn old_or_malformed_host_never_receives_a_business_request() {
        let info = serde_json::to_value(HostInfo::current().unwrap()).unwrap();
        for field in [
            "legacy",
            "app_version",
            "rpc_version",
            "pid",
            "executable",
            "missing",
            "null",
            "envelope",
            "response_version",
        ] {
            let mut identity = info.clone();
            match field {
                "app_version" | "rpc_version" => identity[field] = json!("older"),
                "pid" => identity[field] = json!(0),
                "executable" => identity[field] = json!(""),
                "missing" => identity = json!({}),
                "null" => identity = serde_json::Value::Null,
                _ => {}
            }
            let mut response = if field == "legacy" {
                RpcResponse::failure(json!({"code":"method_not_found"})).unwrap()
            } else {
                RpcResponse::success(identity).unwrap()
            };
            if field == "envelope" {
                response.error = Some(json!({"unexpected":true}));
            }
            if field == "response_version" {
                response.version = "future".into();
            }
            let (stream, worker) = pair(move |mut stream| {
                let hello: RpcRequest = read_json(&mut stream).unwrap();
                assert_eq!(hello.method, "host.hello");
                write_frame(&mut stream, &response).unwrap();
                assert!(
                    read_json::<_, RpcRequest>(&mut stream).is_err(),
                    "unexpected business request"
                );
            });
            let error = call_connected(stream, &business()).unwrap_err();
            assert!(
                error.to_string().contains("popu host stop"),
                "{field}: {error}"
            );
            worker.join().unwrap();
        }
    }

    #[test]
    fn partial_handshake_frame_has_one_deadline_without_a_blocked_reader() {
        let (release, wait) = mpsc::channel();
        let (mut stream, worker) = pair(move |mut stream| {
            let _: RpcRequest = read_json(&mut stream).unwrap();
            stream.write_all(&[0, 0]).unwrap();
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        let started = Instant::now();
        let error = handshake(&mut stream, Duration::from_millis(100)).unwrap_err();
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn lost_business_reply_does_not_repeat_the_request() {
        let count = Arc::new(AtomicUsize::new(0));
        let received = Arc::clone(&count);
        let (stream, worker) = pair(move |mut stream| {
            let hello: RpcRequest = read_json(&mut stream).unwrap();
            write_frame(&mut stream, &hello_response(&hello)).unwrap();
            let request: RpcRequest = read_json(&mut stream).unwrap();
            assert_eq!(request.method, "request.create");
            received.fetch_add(1, Ordering::SeqCst);
            // Simulate an executed request whose reply was lost.
        });
        assert!(call_connected(stream, &business()).is_err());
        worker.join().unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_management_commands_remain_available_without_handshake() {
        for method in ["host.status", "host.stop"] {
            let (mut stream, worker) = pair(move |mut stream| {
                serve_connection(&mut stream, |request| {
                    assert_eq!(request.method, method);
                    RpcResponse::success(json!({"status":"ok"})).unwrap()
                })
                .unwrap();
            });
            let response = exchange(
                &mut stream,
                &RpcRequest {
                    version: RPC_VERSION.into(),
                    method: method.into(),
                    params: json!({}),
                },
            )
            .unwrap();
            assert!(response.ok);
            worker.join().unwrap();
        }
    }
}
