//! Task 9 — Obsidian vault writer.
//!
//! Writes one human-readable daily markdown note per calendar date under
//! `vault_root/obsidian/{date}.md` (Requirement 3.1), appending one entry
//! per conversation turn. First mentions of an entity within a given note
//! get a bidirectional `[[entity-name]]` wikilink (Requirement 3.2) — both
//! the daily note links out to the entity, and a companion per-entity note
//! (`obsidian/entities/{entity-name}.md`) links back to the daily note,
//! which is what "bidirectional" means in Obsidian's plain-markdown
//! backlink model (there is no separate backlink index file to maintain;
//! Obsidian's own search computes backlinks from the `[[...]]` syntax, but
//! writing the reverse link explicitly here makes the graph inspectable
//! even without Obsidian running, per Requirement 3.4's raw full-text
//! search expectation at 10k+ notes).
//!
//! Every note entry is tagged with the mood state as `#mood/<mood>`
//! (Requirement 3.3).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::entity_extractor::Entity;
use super::mood_classifier::MoodState;

#[derive(Debug, thiserror::Error)]
pub enum ObsidianError {
    #[error("io error writing obsidian note: {0}")]
    Io(#[from] std::io::Error),
}

/// One conversation turn's worth of content to append to the vault.
#[derive(Debug, Clone)]
pub struct NoteEntry<'a> {
    pub event_id: uuid::Uuid,
    pub timestamp: DateTime<Utc>,
    pub user_message: &'a str,
    pub miranda_response: &'a str,
    pub mood_state: MoodState,
    pub entities: &'a [Entity],
}

pub struct ObsidianWriter {
    vault_obsidian_dir: PathBuf,
}

impl ObsidianWriter {
    /// `vault_obsidian_dir` is `vault_root/obsidian/`.
    pub fn new(vault_obsidian_dir: impl Into<PathBuf>) -> Self {
        Self {
            vault_obsidian_dir: vault_obsidian_dir.into(),
        }
    }

    fn entities_dir(&self) -> PathBuf {
        self.vault_obsidian_dir.join("entities")
    }

    fn daily_note_path(&self, date: &str) -> PathBuf {
        self.vault_obsidian_dir.join(format!("{date}.md"))
    }

    /// Sanitizes an entity name into a filesystem- and wikilink-safe slug.
    fn entity_slug(name: &str) -> String {
        name.trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
    }

    /// Appends one conversation turn's entry to the daily note, creating
    /// the note (with a level-1 heading) if it doesn't exist yet, and
    /// creating/updating a per-entity note + backlink for every entity
    /// mentioned. Requirement 3.1-3.3.
    pub async fn append_daily_note(&self, entry: &NoteEntry<'_>) -> Result<(), ObsidianError> {
        let date = entry.timestamp.format("%Y-%m-%d").to_string();
        tokio::fs::create_dir_all(&self.vault_obsidian_dir).await?;
        tokio::fs::create_dir_all(self.entities_dir()).await?;

        let note_path = self.daily_note_path(&date);
        let note_existed = tokio::fs::try_exists(&note_path).await.unwrap_or(false);

        let mut block = String::new();
        if !note_existed {
            block.push_str(&format!("# {date}\n\n"));
        }

        let time_str = entry.timestamp.format("%H:%M:%S");
        block.push_str(&format!("## {time_str} — #mood/{}\n\n", entry.mood_state.as_str()));
        block.push_str(&format!("**User:** {}\n\n", entry.user_message));
        block.push_str(&format!("**Miranda:** {}\n\n", entry.miranda_response));

        if !entry.entities.is_empty() {
            block.push_str("**Mentions:** ");
            let links: Vec<String> = entry
                .entities
                .iter()
                .map(|e| format!("[[{}]]", Self::entity_slug(&e.entity_name)))
                .collect();
            block.push_str(&links.join(", "));
            block.push_str("\n\n");
        }
        block.push_str("---\n\n");

        append_to_file(&note_path, &block).await?;

        for entity in entry.entities {
            self.link_entity_note(entity, &date, entry.event_id, entry.mood_state)
                .await?;
        }

        Ok(())
    }

    /// Creates/updates `obsidian/entities/{slug}.md` with a backlink to
    /// the daily note this mention occurred in — the "bidirectional" half
    /// of Requirement 3.2. Idempotent per (date, event) since each event
    /// is only ever written once by the event writer.
    async fn link_entity_note(
        &self,
        entity: &Entity,
        date: &str,
        event_id: uuid::Uuid,
        mood: MoodState,
    ) -> Result<(), ObsidianError> {
        let slug = Self::entity_slug(&entity.entity_name);
        let path = self.entities_dir().join(format!("{slug}.md"));
        let existed = tokio::fs::try_exists(&path).await.unwrap_or(false);

        let mut block = String::new();
        if !existed {
            block.push_str(&format!(
                "# {}\n\n_Entity type: {}_\n\n## Mentions\n\n",
                entity.entity_name,
                entity.entity_type.as_str()
            ));
        }
        block.push_str(&format!(
            "- [[{date}]] (#mood/{}) — event `{event_id}`\n",
            mood.as_str()
        ));

        append_to_file(&path, &block).await
    }
}

async fn append_to_file(path: &Path, content: &str) -> Result<(), ObsidianError> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    file.sync_data().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::entity_extractor::EntityType;

    fn tmp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("miranda-obsidian-test-{}", uuid::Uuid::new_v4()))
    }

    /// Real test: writes 50 events across a mix of moods/entities into a
    /// real temp directory tree and verifies markdown structure (headings,
    /// mood tags) and link integrity (every entity mentioned in a daily
    /// note has a corresponding entity note that backlinks to that date).
    #[tokio::test]
    async fn writes_fifty_events_with_valid_markdown_and_bidirectional_links() {
        let dir = tmp_dir();
        let writer = ObsidianWriter::new(dir.clone());

        let moods = [
            MoodState::Research,
            MoodState::Curiosity,
            MoodState::Casual,
            MoodState::Excited,
        ];

        for i in 0..50 {
            let entities = vec![Entity {
                entity_name: format!("Entity{}", i % 7),
                entity_type: EntityType::Misc,
                confidence: 0.8,
            }];
            let entry = NoteEntry {
                event_id: uuid::Uuid::new_v4(),
                timestamp: Utc::now(),
                user_message: "test user message",
                miranda_response: "test miranda response",
                mood_state: moods[i % moods.len()],
                entities: &entities,
            };
            writer
                .append_daily_note(&entry)
                .await
                .expect("append_daily_note should succeed");
        }

        let date = Utc::now().format("%Y-%m-%d").to_string();
        let note_path = dir.join(format!("{date}.md"));
        let content = tokio::fs::read_to_string(&note_path)
            .await
            .expect("daily note should exist");

        assert!(content.starts_with(&format!("# {date}")));
        assert_eq!(content.matches("**User:**").count(), 50);
        assert!(content.contains("#mood/research"));
        assert!(content.contains("#mood/curiosity"));
        assert!(content.contains("[[entity0]]"));

        // Link integrity: every referenced entity slug has a real note
        // file, and that note backlinks to today's date.
        for i in 0..7 {
            let slug = format!("entity{}", i);
            let entity_path = dir.join("entities").join(format!("{slug}.md"));
            let entity_content = tokio::fs::read_to_string(&entity_path)
                .await
                .unwrap_or_else(|_| panic!("entity note {slug} should exist"));
            assert!(
                entity_content.contains(&format!("[[{date}]]")),
                "entity note {slug} should backlink to {date}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entity_slug_sanitizes_special_characters() {
        assert_eq!(ObsidianWriter::entity_slug("Sarah O'Brien"), "sarah-o-brien");
        assert_eq!(ObsidianWriter::entity_slug("Neo4j"), "neo4j");
    }
}
