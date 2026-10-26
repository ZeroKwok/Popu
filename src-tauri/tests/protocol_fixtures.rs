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

#[test]
fn reading_requirement_is_a_notification_only_boolean() {
    let base = serde_json::json!({"version":"1.0","mode":"notify","content":{"title":"Read me"}});
    assert_eq!(Request::parse(&base.to_string()).unwrap().requires_read.unwrap_or(false), false);
    for required in [false, true] {
        let mut notification = base.clone();
        notification["requires_read"] = required.into();
        let parsed = Request::parse(&notification.to_string()).unwrap();
        assert_eq!(parsed.requires_read, Some(required));
        assert_eq!(serde_json::to_value(parsed).unwrap()["requires_read"], required);
        notification["mode"] = "dialog".into();
        notification["actions"] = serde_json::json!([{"id":"ok","label":"OK","submit":true}]);
        assert!(Request::parse(&notification.to_string()).is_err());
        notification.as_object_mut().unwrap().remove("requires_read");
        let dialog = Request::parse(&notification.to_string()).unwrap();
        assert!(Request::parse(&serde_json::to_string(&dialog).unwrap()).is_ok());
    }
    for invalid in [serde_json::Value::Null, serde_json::json!("true"), serde_json::json!(1)] {
        let mut notification = base.clone();
        notification["requires_read"] = invalid;
        assert!(Request::parse(&notification.to_string()).is_err());
    }
}
