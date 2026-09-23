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

fn default_screen() -> Screen {
    Screen::Cursor
}

fn default_area() -> ScreenArea {
    ScreenArea::WorkArea
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Start,
    Center,
}

impl Default for VerticalAlign {
    fn default() -> Self {
        Self::Center
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowOptions {
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default = "default_screen")]
    pub screen: Screen,
    #[serde(default = "default_area")]
    pub area: ScreenArea,
    #[serde(default)]
    pub width: Option<WindowDimension>,
    #[serde(default)]
    pub height: Option<WindowDimension>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            always_on_top: true,
            screen: Screen::Cursor,
            area: ScreenArea::WorkArea,
            width: None,
            height: None,
        }
    }
}

/// Identifies the display used to resolve window dimensions and placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Cursor,
    Primary,
}

/// Selects whether dimensions use the available work area or the full display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenArea {
    WorkArea,
    Screen,
}

/// A window dimension in logical pixels or as a percentage of its target area.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WindowDimension {
    Pixels(f64),
    Percentage(String),
}

impl WindowDimension {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Percentage(value) if parse_percentage(value).is_ok_and(|percent| percent == 100.0))
    }

    pub fn resolve(&self, limit: u32) -> u32 {
        match self {
            Self::Pixels(value) => value.round().max(1.0).min(limit as f64) as u32,
            Self::Percentage(value) => {
                let percent = parse_percentage(value).expect("percentage validated");
                ((limit as f64 * percent / 100.0).round() as u32).clamp(1, limit)
            }
        }
    }

    fn validate(&self, name: &str) -> Result<(), ProtocolError> {
        match self {
            Self::Pixels(value) if value.is_finite() && *value > 0.0 => Ok(()),
            Self::Pixels(_) => Err(ProtocolError::Invalid(format!(
                "{name} must be a positive finite pixel value"
            ))),
            Self::Percentage(value) => {
                let percent = parse_percentage(value).map_err(|message| {
                    ProtocolError::Invalid(format!("{name} percentage {message}"))
                })?;
                if (0.0..=100.0).contains(&percent) && percent > 0.0 {
                    Ok(())
                } else {
                    Err(ProtocolError::Invalid(format!(
                        "{name} percentage must be within 0%-100%"
                    )))
                }
            }
        }
    }
}

impl WindowOptions {
    pub fn is_maximized(&self) -> bool {
        self.width.as_ref().is_some_and(WindowDimension::is_full)
            && self.height.as_ref().is_some_and(WindowDimension::is_full)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyItem {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentBlock {
    Paragraph {
        #[serde(default)]
        title: Option<String>,
        text: String,
    },
    Properties {
        #[serde(default)]
        title: Option<String>,
        items: Vec<PropertyItem>,
    },
    UnorderedList {
        #[serde(default)]
        title: Option<String>,
        items: Vec<String>,
    },
    OrderedList {
        #[serde(default)]
        title: Option<String>,
        items: Vec<String>,
    },
    Progress {
        #[serde(default)]
        title: Option<String>,
        value: f64,
    },
    Form {
        #[serde(default)]
        title: Option<String>,
        fields: Vec<Field>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    #[serde(default)]
    pub severity: Severity,
    pub title: String,
    #[serde(default)]
    pub vertical_align: VerticalAlign,
    #[serde(default)]
    pub blocks: Vec<ContentBlock>,
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
    #[serde(default)]
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
    Confirmed,
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
    #[serde(default)]
    pub exit_code: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: String,
    pub mode: Mode,
    /// Coalesces live requests in the same mode. Identity remains Host-owned.
    #[serde(default)]
    pub coalesce_key: Option<String>,
    #[serde(default = "default_template")]
    pub template: String,
    #[serde(default = "default_position")]
    pub position: Position,
    #[serde(default)]
    pub window: WindowOptions,
    pub content: Content,
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
        if self.coalesce_key.as_ref().is_some_and(|key| {
            key.trim().is_empty() || key.chars().count() > 200
        }) {
            return Err(ProtocolError::Invalid(
                "coalesce_key must contain 1-200 characters".into(),
            ));
        }
        if self.content.title.trim().is_empty() || self.content.title.chars().count() > 200 {
            return Err(ProtocolError::Invalid(
                "title must contain 1-200 characters".into(),
            ));
        }
        for block in &self.content.blocks {
            validate_content_block(block)?;
        }

        if let Some(width) = &self.window.width {
            width.validate("width")?;
        }
        if let Some(height) = &self.window.height {
            height.validate("height")?;
        }

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
                let has_form_block = self
                    .content
                    .blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Form { .. }));
                if has_form_block || !self.actions.is_empty() {
                    return Err(ProtocolError::Invalid(
                        "notify cannot contain form blocks or actions".into(),
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
        for field in self.fields() {
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
            if action.label.trim().is_empty() || action.label.chars().count() > 200 {
                return Err(ProtocolError::Invalid(format!(
                    "action {} label must contain 1-200 characters",
                    action.id
                )));
            }
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
            if matches!(
                action.result_status,
                Some(ResponseStatus::Submitted | ResponseStatus::TimedOut)
            ) {
                return Err(ProtocolError::Invalid(
                    "result_status must be confirmed, cancelled, or closed".into(),
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

    pub fn window_size(&self, bounds: (u32, u32)) -> (u32, u32) {
        let defaults = self.default_window_size();
        (
            self.window.width.as_ref().map_or_else(
                || defaults.0.round() as u32,
                |value| value.resolve(bounds.0),
            ),
            self.window.height.as_ref().map_or_else(
                || defaults.1.round() as u32,
                |value| value.resolve(bounds.1),
            ),
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
            .fields()
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

    fn fields(&self) -> impl Iterator<Item = &Field> {
        self.content
            .blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Form { fields, .. } => Some(fields),
                _ => None,
            })
            .flatten()
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

fn validate_content_block(block: &ContentBlock) -> Result<(), ProtocolError> {
    let title = match block {
        ContentBlock::Paragraph { title, .. }
        | ContentBlock::Properties { title, .. }
        | ContentBlock::UnorderedList { title, .. }
        | ContentBlock::OrderedList { title, .. }
        | ContentBlock::Progress { title, .. }
        | ContentBlock::Form { title, .. } => title,
    };
    if title
        .as_ref()
        .is_some_and(|value| value.chars().count() > 200)
    {
        return Err(ProtocolError::Invalid(
            "block title must contain at most 200 characters".into(),
        ));
    }
    let text_ok = |value: &str| !value.trim().is_empty() && value.chars().count() <= 20_000;
    match block {
        ContentBlock::Paragraph { text, .. } if !text_ok(text) => Err(ProtocolError::Invalid(
            "paragraph text must contain 1-20000 characters".into(),
        )),
        ContentBlock::Properties { items, .. }
            if items.is_empty()
                || items.iter().any(|item| {
                    item.label.chars().count() > 200
                        || item.text.chars().count() > 20_000
                        || (item.label.trim().is_empty() && item.text.trim().is_empty())
                }) =>
        {
            Err(ProtocolError::Invalid(
                "properties require non-empty valid items".into(),
            ))
        }
        ContentBlock::UnorderedList { items, .. } | ContentBlock::OrderedList { items, .. }
            if items.is_empty() || items.iter().any(|item| !text_ok(item)) =>
        {
            Err(ProtocolError::Invalid(
                "lists require non-empty valid items".into(),
            ))
        }
        ContentBlock::Progress { value, .. }
            if !value.is_finite() || !(0.0..=100.0).contains(value) =>
        {
            Err(ProtocolError::Invalid(
                "progress value must be between 0 and 100".into(),
            ))
        }
        ContentBlock::Form { fields, .. } if fields.is_empty() => Err(ProtocolError::Invalid(
            "form blocks require at least one field".into(),
        )),
        _ => Ok(()),
    }
}
fn parse_percentage(value: &str) -> Result<f64, &'static str> {
    let Some(number) = value.strip_suffix('%') else {
        return Err("must end with '%' ");
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or("must contain a finite number")
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
    if field.label.chars().count() > 200 {
        return Err(ProtocolError::Invalid(format!(
            "field {} label exceeds 200 characters",
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
                .any(|option| option.value.trim().is_empty() || option.label.trim().is_empty())
            {
                return Err(ProtocolError::Invalid(format!(
                    "field {} options require non-empty values and labels",
                    field.id
                )));
            }
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
              "content":{"title":"Confirm","blocks":[
                {"type":"paragraph","text":"Continue?"},
                {"type":"form","fields":[{"id":"confirmed","type":"checkbox","label":"I understand","required":true}]}
              ]},
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
        assert_eq!(request.window.screen, Screen::Cursor);
        assert_eq!(request.window.area, ScreenArea::WorkArea);
        assert_eq!(request.content.vertical_align, VerticalAlign::Center);
        assert_eq!(request.window_size((1920, 1040)), (440, 300));
    }

    #[test]
    fn accepts_action_outcomes_alignment_and_exit_code_boundaries() {
        for exit_code in [0, 255] {
            let request = Request::parse(&format!(
                r#"{{"version":"1.0","mode":"dialog","content":{{"title":"Outcome","vertical_align":"start"}},"actions":[{{"id":"ok","label":"OK","result_status":"confirmed","exit_code":{exit_code}}}]}}"#
            ))
            .unwrap();
            assert_eq!(request.content.vertical_align, VerticalAlign::Start);
            assert_eq!(
                request.actions[0].result_status,
                Some(ResponseStatus::Confirmed)
            );
            assert_eq!(request.actions[0].exit_code, Some(exit_code));
            let response = request.terminal_response(ResponseStatus::Confirmed, Some("ok".into()));
            assert_eq!(response.status, ResponseStatus::Confirmed);
            assert!(response.values.is_empty());
        }

        for invalid in [
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Invalid","vertical_align":"middle"},"actions":[{"id":"ok","label":"OK","result_status":"confirmed"}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Invalid"},"actions":[{"id":"ok","label":"OK","result_status":"confirmed","exit_code":-1}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Invalid"},"actions":[{"id":"ok","label":"OK","result_status":"confirmed","exit_code":256}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Invalid"},"actions":[{"id":"ok","label":"OK","result_status":"confirmed","exit_code":1.5}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Invalid"},"actions":[{"id":"ok","label":"OK","result_status":"timed_out"}]}"#,
        ] {
            assert!(
                Request::parse(invalid).is_err(),
                "{invalid} should be invalid"
            );
        }
    }

    #[test]
    fn accepts_percentage_dimensions_and_rejects_invalid_ones() {
        let request = Request::parse(
            r#"{"version":"1.0","mode":"dialog","window":{"screen":"primary","area":"screen","width":"80%","height":"100%"},"content":{"title":"Percent"},"actions":[{"id":"ok","label":"OK","submit":true}]}"#,
        )
        .unwrap();
        assert_eq!(request.window.screen, Screen::Primary);
        assert_eq!(request.window.area, ScreenArea::Screen);
        assert_eq!(request.window_size((1920, 1080)), (1536, 1080));

        for dimension in [r#""0%""#, r#""101%""#, r#""large""#] {
            let error = Request::parse(&format!(
                r#"{{"version":"1.0","mode":"dialog","window":{{"width":{dimension}}},"content":{{"title":"Invalid"}},"actions":[{{"id":"ok","label":"OK","submit":true}}]}}"#
            ))
            .unwrap_err();
            assert!(error.to_string().contains("width percentage"));
        }
    }

    #[test]
    fn only_two_full_percentage_dimensions_are_maximized() {
        let full = Request::parse(
            r#"{"version":"1.0","mode":"dialog","window":{"width":"100%","height":"100%"},"content":{"title":"Full"},"actions":[{"id":"ok","label":"OK","submit":true}]}"#,
        )
        .unwrap();
        assert!(full.window.is_maximized());

        let partial = Request::parse(
            r#"{"version":"1.0","mode":"dialog","window":{"width":"100%","height":"80%"},"content":{"title":"Partial"},"actions":[{"id":"ok","label":"OK","submit":true}]}"#,
        )
        .unwrap();
        assert!(!partial.window.is_maximized());
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
              "version":"1.0","mode":"notify","content":{"title":"Done","blocks":[
                {"type":"form","fields":[{"id":"name","type":"text","label":"Name"}]}
              ]}
            }"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("notify cannot contain"));
    }

    #[test]
    fn optional_select_accepts_no_value() {
        let request = Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","content":{"title":"Choose","blocks":[
                {"type":"form","fields":[{"id":"choice","type":"select","label":"Choice","options":[{"value":"a","label":"A"}]}]}
              ]},
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
              "version":"1.0","mode":"dialog","content":{"title":"Number","blocks":[
                {"type":"form","fields":[{"id":"count","type":"number","label":"Count","min":1,"step":2}]}
              ]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
        let mut values = Map::new();
        values.insert("count".into(), json!(2));
        assert!(request.submitted_response("ok", values).is_err());
    }

    #[test]
    fn accepts_progress_boundaries_and_rejects_out_of_range_values() {
        for value in [0, 100] {
            Request::parse(&format!(
                r#"{{"version":"1.0","mode":"notify","content":{{"title":"Progress","blocks":[{{"type":"progress","value":{value}}}]}}}}"#
            ))
            .unwrap();
        }
        for value in [-1, 101] {
            let error = Request::parse(&format!(
                r#"{{"version":"1.0","mode":"notify","content":{{"title":"Progress","blocks":[{{"type":"progress","value":{value}}}]}}}}"#
            ))
            .unwrap_err();
            assert!(error.to_string().contains("progress value"));
        }
    }

    #[test]
    fn multiple_form_blocks_share_validation_and_response_values() {
        let request = Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","content":{"title":"Forms","blocks":[
                {"type":"form","fields":[{"id":"name","type":"text","label":"Name","required":true}]},
                {"type":"paragraph","text":"Fields may be separated by display content."},
                {"type":"form","fields":[{"id":"confirmed","type":"checkbox","label":"Confirmed","required":true}]}
              ]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
        let mut values = Map::new();
        values.insert("name".into(), json!("Release"));
        values.insert("confirmed".into(), json!(true));
        assert!(request.submitted_response("ok", values).is_ok());
    }

    #[test]
    fn rejects_empty_block_payloads_and_legacy_top_level_form() {
        for block in [
            r#"{"type":"paragraph","text":"   "}"#,
            r#"{"type":"properties","items":[{"label":" ","text":" "} ]}"#,
            r#"{"type":"properties","items":[{}]}"#,
            r#"{"type":"unordered_list","items":[""]}"#,
            r#"{"type":"ordered_list","items":["   "]}"#,
            r#"{"type":"form","fields":[]}"#,
        ] {
            let text = format!(
                r#"{{"version":"1.0","mode":"dialog","content":{{"title":"Invalid","blocks":[{block}]}},"actions":[{{"id":"ok","label":"OK","submit":true}}]}}"#
            );
            assert!(Request::parse(&text).is_err(), "{block} should be invalid");
        }
        let legacy = r#"{
          "version":"1.0","mode":"dialog","content":{"title":"Legacy"},
          "form":{"fields":[]},"actions":[{"id":"ok","label":"OK","submit":true}]
        }"#;
        assert!(Request::parse(legacy).is_err());
    }

    #[test]
    fn optional_labels_and_one_sided_properties_are_valid() {
        Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","content":{"title":"Optional labels","blocks":[
                {"type":"properties","title":" ","items":[
                  {"label":"Label only"},
                  {"text":"Text only"},
                  {"label":"","text":"Visible text"}
                ]},
                {"type":"form","fields":[
                  {"id":"without_label","type":"text"},
                  {"id":"empty_label","type":"checkbox","label":""}
                ]}
              ]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
    }

    #[test]
    fn required_display_values_reject_blank_text() {
        for text in [
            r#"{"version":"1.0","mode":"dialog","content":{"title":" "},"actions":[{"id":"ok","label":"OK","submit":true}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Actions"},"actions":[{"id":"ok","label":" ","submit":true}]}"#,
            r#"{"version":"1.0","mode":"dialog","content":{"title":"Options","blocks":[{"type":"form","fields":[{"id":"choice","type":"select","options":[{"value":"a","label":" "}]}]}]},"actions":[{"id":"ok","label":"OK","submit":true}]}"#,
        ] {
            assert!(Request::parse(text).is_err(), "{text} should be invalid");
        }
    }

    #[test]
    fn parses_content_length_layout_examples() {
        for example in [
            include_str!("../../protocol/examples/dialog-short-content.json"),
            include_str!("../../protocol/examples/dialog-long-content.json"),
            include_str!("../../protocol/examples/dialog-ultra-long-content.json"),
            include_str!("../../protocol/examples/dialog-fullscreen-screen.json"),
        ] {
            Request::parse(example).unwrap();
        }
    }
}
