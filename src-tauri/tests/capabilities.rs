use serde_json::Value;

#[test]
fn request_uuid_windows_can_start_dragging() {
    let capability: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
        .expect("valid capability JSON");
    assert_eq!(capability["windows"], serde_json::json!(["*-*-*-*-*", "history"]));
    assert_eq!(
        capability["permissions"],
        serde_json::json!(["core:window:allow-start-dragging", "core:event:default"])
    );
}
