//! Task 2 — Model library registry.
//!
//! Tracks all locally-available models (base models and Forge outputs) by
//! display name, feeding the composer/menu UI hover-overlay (role,
//! source, specs) and enforcing display-name uniqueness (Property 4 /
//! Requirement 3.2 — actual collision *resolution* lives in
//! [`crate::forge::naming`]; this module enforces the invariant at
//! registration time so a caller cannot bypass the naming engine and
//! register a duplicate directly).

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use thiserror::Error;

/// One entry in the Model Library, per `design.md`'s `ModelEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub display_name: String,
    pub path: PathBuf,
    pub family: String,
    pub size: String,
    pub descriptor: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Error, PartialEq)]
pub enum RegistryError {
    #[error("a model named '{0}' is already registered")]
    DuplicateName(String),
    #[error("no model named '{0}' is registered")]
    NotFound(String),
}

/// In-memory model library registry. Real persistence (DuckDB/file-backed)
/// is a swap-in behind this same interface later; the registration and
/// uniqueness logic itself is fully real and tested here.
#[derive(Debug, Default)]
pub struct ModelRegistry {
    entries: HashMap<String, ModelEntry>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a model. Fails with [`RegistryError::DuplicateName`] if
    /// `display_name` already exists — callers should run the name
    /// through [`crate::forge::naming::generate_name`] first, which
    /// already resolves collisions against [`Self::existing_names`].
    pub fn register_model(&mut self, entry: ModelEntry) -> Result<(), RegistryError> {
        if self.entries.contains_key(&entry.display_name) {
            return Err(RegistryError::DuplicateName(entry.display_name.clone()));
        }
        self.entries.insert(entry.display_name.clone(), entry);
        Ok(())
    }

    /// Removes a model by display name, e.g. after a rename (Requirement
    /// 3.3 re-registration flow) or deletion.
    pub fn remove_model(&mut self, display_name: &str) -> Result<ModelEntry, RegistryError> {
        self.entries
            .remove(display_name)
            .ok_or_else(|| RegistryError::NotFound(display_name.to_string()))
    }

    pub fn list_models(&self) -> Vec<ModelEntry> {
        let mut list: Vec<ModelEntry> = self.entries.values().cloned().collect();
        list.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        list
    }

    pub fn get(&self, display_name: &str) -> Option<&ModelEntry> {
        self.entries.get(display_name)
    }

    /// Snapshot of currently-used display names, for feeding into the
    /// naming engine's collision check.
    pub fn existing_names(&self) -> std::collections::HashSet<String> {
        self.entries.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> ModelEntry {
        ModelEntry {
            display_name: name.to_string(),
            path: PathBuf::from(format!("/models/{name}")),
            family: "GLM".to_string(),
            size: "9B".to_string(),
            descriptor: "Uncensored".to_string(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn register_and_list() {
        let mut reg = ModelRegistry::new();
        reg.register_model(entry("Erica GLM-9B Uncensored")).unwrap();
        reg.register_model(entry("Nadia GLM-9B Assistant")).unwrap();

        let list = reg.list_models();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].display_name, "Erica GLM-9B Uncensored");
        assert_eq!(list[1].display_name, "Nadia GLM-9B Assistant");
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut reg = ModelRegistry::new();
        reg.register_model(entry("Erica GLM-9B Uncensored")).unwrap();
        let err = reg.register_model(entry("Erica GLM-9B Uncensored")).unwrap_err();
        assert_eq!(err, RegistryError::DuplicateName("Erica GLM-9B Uncensored".to_string()));
    }

    #[test]
    fn remove_and_not_found() {
        let mut reg = ModelRegistry::new();
        reg.register_model(entry("Erica GLM-9B Uncensored")).unwrap();
        reg.remove_model("Erica GLM-9B Uncensored").unwrap();
        assert!(reg.list_models().is_empty());

        let err = reg.remove_model("Ghost Model").unwrap_err();
        assert_eq!(err, RegistryError::NotFound("Ghost Model".to_string()));
    }

    #[test]
    fn existing_names_snapshot() {
        let mut reg = ModelRegistry::new();
        reg.register_model(entry("Erica GLM-9B Uncensored")).unwrap();
        let names = reg.existing_names();
        assert!(names.contains("Erica GLM-9B Uncensored"));
        assert_eq!(names.len(), 1);
    }
}
