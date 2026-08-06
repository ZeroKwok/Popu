use std::fs;
use std::path::{Path, PathBuf};

use popu_lib::protocol::Request;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol/examples")
}

fn parse(path: &Path) -> Result<Request, popu_lib::protocol::ProtocolError> {
    Request::parse(&fs::read_to_string(path).expect("read fixture"))
}

#[test]
fn valid_protocol_examples_are_accepted() {
    for entry in fs::read_dir(examples_dir()).expect("read examples") {
        let path = entry.expect("example entry").path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            parse(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
    }
}

#[test]
fn invalid_protocol_examples_are_rejected() {
    let invalid = examples_dir().join("invalid");
    for entry in fs::read_dir(invalid).expect("read invalid examples") {
        let path = entry.expect("invalid example entry").path();
        assert!(
            parse(&path).is_err(),
            "{} should be invalid",
            path.display()
        );
    }
}
