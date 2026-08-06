use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub const PROTOCOL_VERSION: &str = "1.0";

fn default_template() -> String {
    "confirm".to_owned()
}

fn default_position() -> Position {
    Position::CursorScreenCenter
}

fn default_always_on_top() -> bool {
    true
}

fn default_dialog_timeout() -> u64 {
    300
}

fn default_notification_duration() -> u64 {
    8_000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Dialog,
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    BottomRight,
    CursorScreenCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Default for Severity {
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowOptions {
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            always_on_top: true,
            width: None,
            height: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    #[serde(default)]
    pub severity: Severity,
    pub title: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub details: Vec<Detail>,
    #[serde(default)]
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Textarea,
    Checkbox,
    Select,
    RadioGroup,
    Number,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Vec<FieldOption>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    #[serde(default)]
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStyle {
    Primary,
    Secondary,
    Danger,
}

impl Default for ActionStyle {
    fn default() -> Self {
        Self::Primary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Submitted,
    Cancelled,
    Closed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: ActionStyle,
    #[serde(default)]
    pub submit: bool,
    #[serde(default)]
    pub result_status: Option<ResponseStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: String,
    pub mode: Mode,
    #[serde(default = "default_template")]
    pub template: String,
    #[serde(default = "default_position")]
    pub position: Position,
    #[serde(default)]
    pub window: WindowOptions,
    pub content: Content,
    #[serde(default)]
    pub form: Option<Form>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default = "default_dialog_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_notification_duration")]
    pub duration_ms: u64,
}

impl Request {
    pub fn parse(text: &str) -> Result<Self, ProtocolError> {
        let request: Self = serde_json::from_str(text)?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.version.clone()));
        }
        if self.content.title.trim().is_empty() || self.content.title.chars().count() > 200 {
            return Err(ProtocolError::Invalid(
                "title must contain 1-200 characters".into(),
            ));
        }
        if self.content.message.chars().count() > 20_000 {
            return Err(ProtocolError::Invalid(
                "message exceeds 20000 characters".into(),
            ));
        }
        if let Some(progress) = self.content.progress {
            if !(0.0..=100.0).contains(&progress) {
                return Err(ProtocolError::Invalid(
                    "progress must be between 0 and 100".into(),
                ));
            }
        }

        let (default_width, default_height) = self.default_window_size();
        validate_dimension(self.window.width.unwrap_or(default_width), "width")?;
        validate_dimension(self.window.height.unwrap_or(default_height), "height")?;

        match self.mode {
            Mode::Dialog => {
                if self.actions.is_empty() {
                    return Err(ProtocolError::Invalid(
                        "dialog requires at least one action".into(),
                    ));
                }
                if self.timeout_seconds == 0 || self.timeout_seconds > 86_400 {
                    return Err(ProtocolError::Invalid(
                        "timeout_seconds must be within 1-86400".into(),
                    ));
                }
            }
            Mode::Notify => {
                if self.form.is_some() || !self.actions.is_empty() {
                    return Err(ProtocolError::Invalid(
                        "notify cannot contain form or actions".into(),
                    ));
                }
                if !(500..=86_400_000).contains(&self.duration_ms) {
                    return Err(ProtocolError::Invalid(
                        "duration_ms must be within 500-86400000".into(),
                    ));
                }
            }
        }

        let mut field_ids = HashSet::new();
        for field in self.form.iter().flat_map(|form| &form.fields) {
            validate_identifier(&field.id, "field")?;
            if !field_ids.insert(field.id.as_str()) {
                return Err(ProtocolError::Invalid(format!(
                    "duplicate field id: {}",
                    field.id
                )));
            }
            validate_field(field)?;
        }

        let mut action_ids = HashSet::new();
        for action in &self.actions {
            validate_identifier(&action.id, "action")?;
            if !action_ids.insert(action.id.as_str()) {
                return Err(ProtocolError::Invalid(format!(
                    "duplicate action id: {}",
                    action.id
                )));
            }
            let terminal_count =
                usize::from(action.submit) + usize::from(action.result_status.is_some());
            if terminal_count != 1 {
                return Err(ProtocolError::Invalid(format!(
                    "action {} must define exactly one of submit or result_status",
                    action.id
                )));
            }
            if matches!(action.result_status, Some(ResponseStatus::Submitted)) {
                return Err(ProtocolError::Invalid(
                    "submitted actions must use submit: true".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn default_window_size(&self) -> (f64, f64) {
        match self.mode {
            Mode::Dialog => (440.0, 300.0),
            Mode::Notify => (360.0, 180.0),
        }
    }

    pub fn window_size(&self) -> (f64, f64) {
        let defaults = self.default_window_size();
        (
            self.window.width.unwrap_or(defaults.0),
            self.window.height.unwrap_or(defaults.1),
        )
    }

    pub fn submitted_response(
        &self,
        action_id: &str,
        values: Map<String, Value>,
    ) -> Result<Response, ProtocolError> {
        let action = self
            .actions
            .iter()
            .find(|action| action.id == action_id && action.submit)
            .ok_or_else(|| ProtocolError::Invalid(format!("unknown submit action: {action_id}")))?;
        let _ = action;
        self.validate_values(&values)?;
        Ok(Response {
            version: PROTOCOL_VERSION.to_owned(),
            status: ResponseStatus::Submitted,
            action: Some(action_id.to_owned()),
            values,
        })
    }

    pub fn terminal_response(&self, status: ResponseStatus, action: Option<String>) -> Response {
        Response {
            version: PROTOCOL_VERSION.to_owned(),
            status,
            action,
            values: Map::new(),
        }
    }

    fn validate_values(&self, values: &Map<String, Value>) -> Result<(), ProtocolError> {
        let fields: HashMap<&str, &Field> = self
            .form
            .iter()
            .flat_map(|form| &form.fields)
            .map(|field| (field.id.as_str(), field))
            .collect();
        for key in values.keys() {
            if !fields.contains_key(key.as_str()) {
                return Err(ProtocolError::Invalid(format!(
                    "unknown response field: {key}"
                )));
            }
        }
        for field in fields.values() {
            validate_value(field, values.get(&field.id))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub version: String,
    pub status: ResponseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default)]
    pub values: Map<String, Value>,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(String),
    #[error("invalid request: {0}")]
    Invalid(String),
}

fn validate_dimension(value: f64, name: &str) -> Result<(), ProtocolError> {
    if !(160.0..=2_000.0).contains(&value) {
        return Err(ProtocolError::Invalid(format!(
            "{name} must be within 160-2000"
        )));
    }
    Ok(())
}

fn validate_identifier(value: &str, kind: &str) -> Result<(), ProtocolError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !valid {
        return Err(ProtocolError::Invalid(format!(
            "invalid {kind} id: {value}"
        )));
    }
    Ok(())
}

fn validate_field(field: &Field) -> Result<(), ProtocolError> {
    if field.label.trim().is_empty() {
        return Err(ProtocolError::Invalid(format!(
            "field {} has an empty label",
            field.id
        )));
    }
    if let Some(pattern) = &field.pattern {
        Regex::new(pattern).map_err(|error| {
            ProtocolError::Invalid(format!("field {} has invalid pattern: {error}", field.id))
        })?;
    }
    if let (Some(min), Some(max)) = (field.min_length, field.max_length) {
        if min > max {
            return Err(ProtocolError::Invalid(format!(
                "field {} min_length exceeds max_length",
                field.id
            )));
        }
    }
    if let (Some(min), Some(max)) = (field.min, field.max) {
        if min > max {
            return Err(ProtocolError::Invalid(format!(
                "field {} min exceeds max",
                field.id
            )));
        }
    }
    if field
        .step
        .is_some_and(|step| !step.is_finite() || step <= 0.0)
    {
        return Err(ProtocolError::Invalid(format!(
            "field {} step must be positive",
            field.id
        )));
    }
    match field.field_type {
        FieldType::Select | FieldType::RadioGroup => {
            if field.options.is_empty() {
                return Err(ProtocolError::Invalid(format!(
                    "field {} requires options",
                    field.id
                )));
            }
            let mut values = HashSet::new();
            if field
                .options
                .iter()
                .any(|option| !values.insert(option.value.as_str()))
            {
                return Err(ProtocolError::Invalid(format!(
                    "field {} has duplicate options",
                    field.id
                )));
            }
        }
        _ if !field.options.is_empty() => {
            return Err(ProtocolError::Invalid(format!(
                "field {} cannot contain options",
                field.id
            )));
        }
        _ => {}
    }
    if let Some(default) = &field.default {
        validate_value(field, Some(default))?;
    }
    Ok(())
}

fn validate_value(field: &Field, value: Option<&Value>) -> Result<(), ProtocolError> {
    let missing = value.is_none() || value == Some(&Value::Null);
    if missing {
        return if field.required {
            Err(ProtocolError::Invalid(format!(
                "field {} is required",
                field.id
            )))
        } else {
            Ok(())
        };
    }
    let value = value.expect("value checked above");
    match field.field_type {
        FieldType::Text | FieldType::Textarea => {
            let text = value.as_str().ok_or_else(|| type_error(field, "string"))?;
            let length = text.chars().count();
            if field.required && text.trim().is_empty() {
                return Err(ProtocolError::Invalid(format!(
                    "field {} is required",
                    field.id
                )));
            }
            if field.min_length.is_some_and(|min| length < min)
                || field.max_length.is_some_and(|max| length > max)
            {
                return Err(ProtocolError::Invalid(format!(
                    "field {} has invalid length",
                    field.id
                )));
            }
            if let Some(pattern) = &field.pattern {
                if !Regex::new(pattern)
                    .expect("pattern validated")
                    .is_match(text)
                {
                    return Err(ProtocolError::Invalid(format!(
                        "field {} does not match pattern",
                        field.id
                    )));
                }
            }
        }
        FieldType::Checkbox => {
            let checked = value
                .as_bool()
                .ok_or_else(|| type_error(field, "boolean"))?;
            if field.required && !checked {
                return Err(ProtocolError::Invalid(format!(
                    "field {} must be checked",
                    field.id
                )));
            }
        }
        FieldType::Select | FieldType::RadioGroup => {
            let selected = value.as_str().ok_or_else(|| type_error(field, "string"))?;
            if !field.options.iter().any(|option| option.value == selected) {
                return Err(ProtocolError::Invalid(format!(
                    "field {} has an unknown option",
                    field.id
                )));
            }
        }
        FieldType::Number => {
            let number = value.as_f64().ok_or_else(|| type_error(field, "number"))?;
            if field.min.is_some_and(|min| number < min)
                || field.max.is_some_and(|max| number > max)
            {
                return Err(ProtocolError::Invalid(format!(
                    "field {} is outside its range",
                    field.id
                )));
            }
            if let Some(step) = field.step {
                let steps = (number - field.min.unwrap_or(0.0)) / step;
                let tolerance = 1e-9 * steps.abs().max(1.0);
                if (steps - steps.round()).abs() > tolerance {
                    return Err(ProtocolError::Invalid(format!(
                        "field {} does not match its step",
                        field.id
                    )));
                }
            }
        }
    }
    Ok(())
}

fn type_error(field: &Field, expected: &str) -> ProtocolError {
    ProtocolError::Invalid(format!("field {} must be a {expected}", field.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> Request {
        Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","template":"confirm",
              "content":{"title":"Confirm","message":"Continue?"},
              "form":{"fields":[{"id":"confirmed","type":"checkbox","label":"I understand","required":true}]},
              "actions":[
                {"id":"confirm","label":"Continue","style":"danger","submit":true},
                {"id":"cancel","label":"Cancel","style":"secondary","result_status":"cancelled"}
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_minimal_dialog_and_applies_defaults() {
        let request = request();
        assert_eq!(request.position, Position::CursorScreenCenter);
        assert_eq!(request.window_size(), (440.0, 300.0));
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = Request::parse(
            r#"{"version":"1.0","mode":"notify","content":{"title":"Done"},"unknown":true}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn required_checkbox_cannot_be_bypassed_by_action() {
        let request = request();
        let error = request
            .submitted_response("confirm", Map::new())
            .unwrap_err();
        assert!(error.to_string().contains("required"));

        let mut values = Map::new();
        values.insert("confirmed".into(), json!(false));
        assert!(request.submitted_response("confirm", values).is_err());
    }

    #[test]
    fn submitted_values_are_revalidated() {
        let request = request();
        let mut values = Map::new();
        values.insert("confirmed".into(), json!(true));
        let response = request.submitted_response("confirm", values).unwrap();
        assert_eq!(response.status, ResponseStatus::Submitted);
    }

    #[test]
    fn notify_rejects_form_and_actions() {
        let error = Request::parse(
            r#"{
              "version":"1.0","mode":"notify","content":{"title":"Done"},
              "form":{"fields":[]}
            }"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("notify cannot contain"));
    }

    #[test]
    fn optional_select_accepts_no_value() {
        let request = Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","content":{"title":"Choose"},
              "form":{"fields":[{"id":"choice","type":"select","label":"Choice","options":[{"value":"a","label":"A"}]}]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
        let mut values = Map::new();
        values.insert("choice".into(), Value::Null);
        assert!(request.submitted_response("ok", values).is_ok());
    }

    #[test]
    fn number_step_is_revalidated_in_rust() {
        let request = Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","content":{"title":"Number"},
              "form":{"fields":[{"id":"count","type":"number","label":"Count","min":1,"step":2}]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
        let mut values = Map::new();
        values.insert("count".into(), json!(2));
        assert!(request.submitted_response("ok", values).is_err());
    }
}
