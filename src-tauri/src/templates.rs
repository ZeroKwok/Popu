use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::protocol::{Mode, Request, PROTOCOL_VERSION};

const CONFIRM_HTML: &str = include_str!("../../ui/templates/confirm/template.html");
const CONFIRM_CSS: &str = include_str!("../../ui/templates/confirm/template.css");
const NOTIFICATION_HTML: &str = include_str!("../../ui/templates/notification/template.html");
const NOTIFICATION_CSS: &str = include_str!("../../ui/templates/notification/template.css");

#[derive(Debug, Clone, Serialize)]
pub struct TemplateAssets {
    pub name: String,
    pub html: String,
    pub css: String,
    pub external: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    version: String,
    modes: Vec<Mode>,
    #[serde(default = "default_html")]
    html: String,
    #[serde(default = "default_css")]
    css: String,
}

fn default_html() -> String {
    "template.html".to_owned()
}

fn default_css() -> String {
    "template.css".to_owned()
}

pub fn builtin_names() -> &'static [&'static str] {
    &["confirm", "form", "notification"]
}

pub fn resolve(
    request: &Request,
    template_dir: Option<&Path>,
) -> Result<TemplateAssets, TemplateError> {
    if let Some(directory) = template_dir {
        return resolve_external(request, directory);
    }
    let (html, css, expected_mode) = match request.template.as_str() {
        "confirm" => (CONFIRM_HTML, CONFIRM_CSS, Mode::Dialog),
        "form" => (CONFIRM_HTML, CONFIRM_CSS, Mode::Dialog),
        "notification" => (NOTIFICATION_HTML, NOTIFICATION_CSS, Mode::Notify),
        name => return Err(TemplateError::Unknown(name.to_owned())),
    };
    if request.mode != expected_mode {
        return Err(TemplateError::ModeMismatch(request.template.clone()));
    }
    validate_slots(html, request.mode)?;
    Ok(TemplateAssets {
        name: request.template.clone(),
        html: html.to_owned(),
        css: css.to_owned(),
        external: false,
    })
}

pub fn validate_directory(path: &Path) -> Result<String, TemplateError> {
    let canonical = canonical_directory(path)?;
    let manifest = read_manifest(&canonical)?;
    let html = read_inside(&canonical, &manifest.html)?;
    let _css = read_inside(&canonical, &manifest.css)?;
    if manifest.version != PROTOCOL_VERSION {
        return Err(TemplateError::Version(manifest.version));
    }
    for mode in &manifest.modes {
        validate_slots(&html, *mode)?;
    }
    Ok(manifest.name)
}

fn resolve_external(request: &Request, path: &Path) -> Result<TemplateAssets, TemplateError> {
    let canonical = canonical_directory(path)?;
    let manifest = read_manifest(&canonical)?;
    if manifest.name != request.template {
        return Err(TemplateError::NameMismatch {
            expected: request.template.clone(),
            actual: manifest.name,
        });
    }
    if manifest.version != request.version {
        return Err(TemplateError::Version(manifest.version));
    }
    if !manifest.modes.contains(&request.mode) {
        return Err(TemplateError::ModeMismatch(request.template.clone()));
    }
    let html = read_inside(&canonical, &manifest.html)?;
    let css = read_inside(&canonical, &manifest.css)?;
    validate_slots(&html, request.mode)?;
    Ok(TemplateAssets {
        name: request.template.clone(),
        html,
        css,
        external: true,
    })
}

fn canonical_directory(path: &Path) -> Result<PathBuf, TemplateError> {
    let canonical = path.canonicalize().map_err(|source| TemplateError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(TemplateError::NotDirectory(canonical));
    }
    Ok(canonical)
}

fn read_manifest(directory: &Path) -> Result<Manifest, TemplateError> {
    let path = directory.join("manifest.json");
    let text = fs::read_to_string(&path).map_err(|source| TemplateError::Io { path, source })?;
    Ok(serde_json::from_str(&text)?)
}

fn read_inside(directory: &Path, relative: &str) -> Result<String, TemplateError> {
    let requested = directory.join(relative);
    let canonical = requested
        .canonicalize()
        .map_err(|source| TemplateError::Io {
            path: requested.clone(),
            source,
        })?;
    if !canonical.starts_with(directory) {
        return Err(TemplateError::PathTraversal(relative.to_owned()));
    }
    fs::read_to_string(&canonical).map_err(|source| TemplateError::Io {
        path: canonical,
        source,
    })
}

fn validate_slots(html: &str, mode: Mode) -> Result<(), TemplateError> {
    for slot in ["title", "content"] {
        if !html.contains(&format!("data-popu-slot=\"{slot}\"")) {
            return Err(TemplateError::MissingSlot(slot));
        }
    }
    if mode == Mode::Dialog {
        for slot in ["actions"] {
            if !html.contains(&format!("data-popu-slot=\"{slot}\"")) {
                return Err(TemplateError::MissingSlot(slot));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("unknown template: {0}")]
    Unknown(String),
    #[error("template does not support the request mode: {0}")]
    ModeMismatch(String),
    #[error("template protocol version is unsupported: {0}")]
    Version(String),
    #[error("template name mismatch: expected {expected}, got {actual}")]
    NameMismatch { expected: String, actual: String },
    #[error("template path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("template path escapes its directory: {0}")]
    PathTraversal(String),
    #[error("template is missing required slot: {0}")]
    MissingSlot(&'static str),
    #[error("cannot read template file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid template manifest: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::protocol::Request;

    fn notification() -> Request {
        Request::parse(r#"{"version":"1.0","mode":"notify","template":"notification","content":{"title":"Done"}}"#).unwrap()
    }

    #[test]
    fn resolves_builtin_template() {
        let assets = resolve(&notification(), None).unwrap();
        assert!(assets.html.contains("data-popu-slot=\"title\""));
        assert!(assets.html.contains("data-popu-drag-region"));
        assert!(assets.html.contains("data-popu-selectable"));
        assert!(!assets.external);
    }

    #[test]
    fn dialog_template_names_share_the_content_only_layout() {
        let mut confirm = Request::parse(
            r#"{
              "version":"1.0","mode":"dialog","template":"confirm",
              "content":{"title":"Form","blocks":[
                {"type":"form","fields":[{"id":"name","type":"text","label":"Name"}]}
              ]},
              "actions":[{"id":"ok","label":"OK","submit":true}]
            }"#,
        )
        .unwrap();
        let confirm_assets = resolve(&confirm, None).unwrap();
        confirm.template = "form".into();
        let form_assets = resolve(&confirm, None).unwrap();
        assert_eq!(confirm_assets.html, form_assets.html);
        assert_eq!(confirm_assets.css, form_assets.css);
        assert!(!confirm_assets.html.contains("popu-form"));
        assert!(!confirm_assets.html.contains("popu-progress"));
    }

    #[test]
    fn rejects_mode_mismatch() {
        let mut request = notification();
        request.template = "confirm".into();
        assert!(matches!(
            resolve(&request, None),
            Err(TemplateError::ModeMismatch(_))
        ));
    }

    #[test]
    fn validates_external_template() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("manifest.json"),
            r#"{"name":"custom","version":"1.0","modes":["notify"]}"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("template.html"),
            r#"<h1 data-popu-slot="title"></h1><p data-popu-slot="content"></p>"#,
        )
        .unwrap();
        fs::write(directory.path().join("template.css"), "h1 { color: red; }").unwrap();
        assert_eq!(validate_directory(directory.path()).unwrap(), "custom");
    }

    #[test]
    fn rejects_missing_required_slot() {
        assert!(matches!(
            validate_slots("<h1 data-popu-slot=\"title\"></h1>", Mode::Notify),
            Err(TemplateError::MissingSlot("content"))
        ));
    }
}
