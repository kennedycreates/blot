use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SUPPORTED_FORMAT_NAME: &str = "watercolor.workspace";
pub const SUPPORTED_FORMAT_VERSION: &str = "0.1";
pub const SUPPORTED_OBJECT_TYPES: &[&str] = &["note", "asset", "task"];
pub const SUPPORTED_RELATIONSHIP_TYPES: &[&str] = &["references", "explained_by", "implemented_in"];
pub const SUPPORTED_TARGET_KINDS: &[&str] = &["object", "path"];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WaterWorkspace {
    pub format_name: String,
    pub format_version: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub palettes: Vec<WaterPalette>,
    pub objects: Vec<WaterObject>,
    pub relationships: Vec<WaterRelationship>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WaterPalette {
    pub palette_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub object_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WaterObject {
    pub object_id: String,
    pub object_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub app_origin: Option<String>,
    pub file_refs: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WaterRelationship {
    pub relationship_id: String,
    pub source_object_id: String,
    pub target_kind: String,
    pub target_object_id: Option<String>,
    pub target_path: Option<PathBuf>,
    pub relationship_type: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum WaterFileError {
    Io(std::io::Error),
    Malformed(serde_json::Error),
    UnsupportedFormat {
        format_name: String,
        format_version: String,
    },
    Validation(String),
}

impl fmt::Display for WaterFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to read .water file: {error}"),
            Self::Malformed(error) => write!(formatter, "malformed .water JSON: {error}"),
            Self::UnsupportedFormat {
                format_name,
                format_version,
            } => write!(
                formatter,
                "unsupported .water format {format_name} version {format_version}"
            ),
            Self::Validation(message) => write!(formatter, "invalid .water file: {message}"),
        }
    }
}

impl Error for WaterFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Malformed(error) => Some(error),
            Self::UnsupportedFormat { .. } | Self::Validation(_) => None,
        }
    }
}

impl From<std::io::Error> for WaterFileError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for WaterFileError {
    fn from(error: serde_json::Error) -> Self {
        Self::Malformed(error)
    }
}

pub fn parse_water_file(path: &Path) -> Result<WaterWorkspace, WaterFileError> {
    let content = fs::read_to_string(path)?;
    let workspace: WaterWorkspace = serde_json::from_str(&content)?;
    validate_workspace(&workspace)?;
    Ok(workspace)
}

pub fn note_objects(workspace: &WaterWorkspace) -> Vec<&WaterObject> {
    workspace
        .objects
        .iter()
        .filter(|object| object.object_type == "note")
        .collect()
}

pub fn note_body(object: &WaterObject) -> Option<&str> {
    object.body.as_deref().or_else(|| object.content.as_deref())
}

pub fn update_note_body(
    workspace: &mut WaterWorkspace,
    object_id: &str,
    body: String,
) -> Result<(), WaterFileError> {
    let Some(object) = workspace
        .objects
        .iter_mut()
        .find(|object| object.object_id == object_id)
    else {
        return Err(WaterFileError::Validation(format!(
            "note object {object_id} not found"
        )));
    };

    if object.object_type != "note" {
        return Err(WaterFileError::Validation(format!(
            "object {object_id} is not a note"
        )));
    }

    object.body = Some(body);
    Ok(())
}

pub fn save_water_file(path: &Path, workspace: &WaterWorkspace) -> Result<PathBuf, WaterFileError> {
    validate_workspace(workspace)?;
    let serialized = serde_json::to_string_pretty(workspace)?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace.water".to_string());
    let stamp = unix_timestamp();
    let backup_path = parent.join(format!("{file_name}.{stamp}.bak"));
    let temp_path = parent.join(format!("{file_name}.tmp.{}.{}", std::process::id(), stamp));

    fs::write(&temp_path, serialized)?;
    if path.exists() {
        fs::copy(path, &backup_path)?;
    }
    fs::rename(&temp_path, path)?;
    Ok(backup_path)
}

pub fn validate_workspace(workspace: &WaterWorkspace) -> Result<(), WaterFileError> {
    if workspace.format_name != SUPPORTED_FORMAT_NAME
        || workspace.format_version != SUPPORTED_FORMAT_VERSION
    {
        return Err(WaterFileError::UnsupportedFormat {
            format_name: workspace.format_name.clone(),
            format_version: workspace.format_version.clone(),
        });
    }

    validate_prefixed_id("workspace_id", &workspace.workspace_id, "wcw_")?;
    validate_non_empty("workspace_name", &workspace.workspace_name)?;

    let mut all_ids = HashSet::new();
    insert_unique_id("workspace_id", &workspace.workspace_id, &mut all_ids)?;

    let mut object_ids = HashSet::new();
    for object in &workspace.objects {
        validate_prefixed_id("object_id", &object.object_id, "wco_")?;
        validate_non_empty("object_type", &object.object_type)?;
        validate_supported("object_type", &object.object_type, SUPPORTED_OBJECT_TYPES)?;
        validate_non_empty("title", &object.title)?;
        insert_unique_id("object_id", &object.object_id, &mut all_ids)?;
        if !object_ids.insert(object.object_id.as_str()) {
            return Err(WaterFileError::Validation(format!(
                "duplicate object_id {}",
                object.object_id
            )));
        }
        for file_ref in &object.file_refs {
            validate_path_value("file_refs[]", file_ref)?;
        }
    }

    let mut palette_ids = HashSet::new();
    for palette in &workspace.palettes {
        validate_prefixed_id("palette_id", &palette.palette_id, "wcp_")?;
        validate_non_empty("name", &palette.name)?;
        insert_unique_id("palette_id", &palette.palette_id, &mut all_ids)?;
        if !palette_ids.insert(palette.palette_id.as_str()) {
            return Err(WaterFileError::Validation(format!(
                "duplicate palette_id {}",
                palette.palette_id
            )));
        }
        for object_id in &palette.object_ids {
            if !object_ids.contains(object_id.as_str()) {
                return Err(WaterFileError::Validation(format!(
                    "palette {} references unknown object_id {}",
                    palette.palette_id, object_id
                )));
            }
        }
    }

    let mut relationship_ids = HashSet::new();
    for relationship in &workspace.relationships {
        validate_prefixed_id("relationship_id", &relationship.relationship_id, "wcr_")?;
        validate_non_empty("source_object_id", &relationship.source_object_id)?;
        validate_non_empty("target_kind", &relationship.target_kind)?;
        validate_supported(
            "target_kind",
            &relationship.target_kind,
            SUPPORTED_TARGET_KINDS,
        )?;
        validate_non_empty("relationship_type", &relationship.relationship_type)?;
        validate_supported(
            "relationship_type",
            &relationship.relationship_type,
            SUPPORTED_RELATIONSHIP_TYPES,
        )?;
        insert_unique_id(
            "relationship_id",
            &relationship.relationship_id,
            &mut all_ids,
        )?;
        if !relationship_ids.insert(relationship.relationship_id.as_str()) {
            return Err(WaterFileError::Validation(format!(
                "duplicate relationship_id {}",
                relationship.relationship_id
            )));
        }
        if !object_ids.contains(relationship.source_object_id.as_str()) {
            return Err(WaterFileError::Validation(format!(
                "relationship {} references unknown source_object_id {}",
                relationship.relationship_id, relationship.source_object_id
            )));
        }
        match relationship.target_kind.as_str() {
            "object" => {
                let Some(target_object_id) = &relationship.target_object_id else {
                    return Err(WaterFileError::Validation(format!(
                        "relationship {} target_kind object requires target_object_id",
                        relationship.relationship_id
                    )));
                };
                if !object_ids.contains(target_object_id.as_str()) {
                    return Err(WaterFileError::Validation(format!(
                        "relationship {} references unknown target_object_id {}",
                        relationship.relationship_id, target_object_id
                    )));
                }
            }
            "path" => {
                let Some(target_path) = &relationship.target_path else {
                    return Err(WaterFileError::Validation(format!(
                        "relationship {} target_kind path requires target_path",
                        relationship.relationship_id
                    )));
                };
                validate_path_value("target_path", target_path)?;
            }
            _ => {}
        }
        if let Some(target_path) = &relationship.target_path {
            validate_path_value("target_path", target_path)?;
        }
    }

    Ok(())
}

fn insert_unique_id<'a>(
    field: &str,
    value: &'a str,
    all_ids: &mut HashSet<&'a str>,
) -> Result<(), WaterFileError> {
    if !all_ids.insert(value) {
        return Err(WaterFileError::Validation(format!(
            "{field} {value} duplicates another workspace ID"
        )));
    }
    Ok(())
}

fn validate_prefixed_id(field: &str, value: &str, prefix: &str) -> Result<(), WaterFileError> {
    validate_non_empty(field, value)?;
    if !value.starts_with(prefix) {
        return Err(WaterFileError::Validation(format!(
            "{field} must start with {prefix}"
        )));
    }
    Ok(())
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), WaterFileError> {
    if value.trim().is_empty() {
        return Err(WaterFileError::Validation(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_supported(field: &str, value: &str, allowed: &[&str]) -> Result<(), WaterFileError> {
    if !allowed.contains(&value) {
        return Err(WaterFileError::Validation(format!(
            "{field} '{value}' is not supported; expected one of: {}",
            allowed.join(", ")
        )));
    }
    Ok(())
}

fn validate_path_value(field: &str, value: &Path) -> Result<(), WaterFileError> {
    let text = value.to_string_lossy();
    if text.trim().is_empty() {
        return Err(WaterFileError::Validation(format!(
            "{field} must be a non-empty path string"
        )));
    }
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_fixture() {
        let workspace = fixture("basic.water").expect("basic fixture");

        assert_eq!(workspace.workspace_id, "wcw_basic");
        assert_eq!(workspace.workspace_name, "Basic Workspace");
        assert_eq!(workspace.palettes.len(), 1);
        assert_eq!(workspace.objects.len(), 2);
        assert_eq!(workspace.relationships.len(), 1);
    }

    #[test]
    fn lists_note_objects() {
        let workspace = fixture("basic.water").expect("basic fixture");
        let notes = note_objects(&workspace);

        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].object_id, "wco_note");
    }

    #[test]
    fn edits_note_body_in_memory() {
        let mut workspace = fixture("basic.water").expect("basic fixture");

        update_note_body(&mut workspace, "wco_note", "New body".to_string()).unwrap();

        let note = note_objects(&workspace).remove(0);
        assert_eq!(note_body(note), Some("New body"));
    }

    #[test]
    fn parses_multiple_palettes_fixture() {
        let workspace = fixture("multiple_palettes.water").expect("fixture");

        assert_eq!(workspace.palettes.len(), 2);
        assert!(workspace
            .objects
            .iter()
            .any(|object| object.object_type == "task"));
        assert!(workspace
            .relationships
            .iter()
            .any(|relationship| relationship.relationship_type == "implemented_in"));
    }

    #[test]
    fn rejects_broken_references_fixture() {
        let error = fixture("broken_refs.water").expect_err("broken refs should fail");

        assert!(matches!(error, WaterFileError::Validation(_)));
    }

    #[test]
    fn rejects_unsupported_version_fixture() {
        let error =
            fixture("unsupported_version.water").expect_err("unsupported version should fail");

        assert!(matches!(error, WaterFileError::UnsupportedFormat { .. }));
    }

    #[test]
    fn rejects_malformed_fixture_without_panic() {
        let error = fixture("malformed.water").expect_err("malformed fixture should fail");

        assert!(matches!(error, WaterFileError::Malformed(_)));
    }

    #[test]
    fn saves_and_reloads_note_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.water");
        let mut workspace = fixture("basic.water").expect("basic fixture");
        update_note_body(&mut workspace, "wco_note", "Saved body".to_string()).unwrap();

        std::fs::write(&path, serde_json::to_string_pretty(&workspace).unwrap()).unwrap();
        update_note_body(&mut workspace, "wco_note", "Reloaded body".to_string()).unwrap();
        let backup = save_water_file(&path, &workspace).unwrap();
        let reloaded = parse_water_file(&path).unwrap();

        assert!(backup.exists());
        assert_eq!(note_body(note_objects(&reloaded)[0]), Some("Reloaded body"));
    }

    #[test]
    fn malformed_water_does_not_overwrite_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("malformed.water");
        let original = "{not valid json";
        std::fs::write(&path, original).unwrap();

        let error = parse_water_file(&path).expect_err("malformed fixture should fail");

        assert!(matches!(error, WaterFileError::Malformed(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn unsupported_version_does_not_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unsupported.water");
        let original = "original";
        std::fs::write(&path, original).unwrap();
        let mut workspace = fixture("basic.water").expect("basic fixture");
        workspace.format_version = "9.9".to_string();

        let error = save_water_file(&path, &workspace).expect_err("unsupported should fail");

        assert!(matches!(error, WaterFileError::UnsupportedFormat { .. }));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn rejects_unsupported_object_type() {
        let mut workspace = fixture("basic.water").expect("basic fixture");
        workspace.objects[0].object_type = "calendar".to_string();

        let error = validate_workspace(&workspace).expect_err("object type should fail");

        assert!(error.to_string().contains("object_type 'calendar'"));
    }

    #[test]
    fn rejects_unsupported_target_kind() {
        let mut workspace = fixture("basic.water").expect("basic fixture");
        workspace.relationships[0].target_kind = "calendar".to_string();

        let error = validate_workspace(&workspace).expect_err("target kind should fail");

        assert!(error.to_string().contains("target_kind 'calendar'"));
    }

    #[test]
    fn path_relationship_requires_target_path() {
        let mut workspace = fixture("multiple_palettes.water").expect("fixture");
        let relationship = workspace
            .relationships
            .iter_mut()
            .find(|relationship| relationship.target_kind == "path")
            .expect("path relationship");
        relationship.target_path = None;

        let error = validate_workspace(&workspace).expect_err("missing path should fail");

        assert!(error.to_string().contains("requires target_path"));
    }

    fn fixture(name: &str) -> Result<WaterWorkspace, WaterFileError> {
        parse_water_file(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(name),
        )
    }
}
