//! File locale des snapshots non envoyés (backend injoignable, pas de
//! connexion, mise à jour requise), persistée sur disque.
//!
//! Un snapshot est l'état complet d'un compte : un nouveau snapshot du même
//! compte remplace l'ancien en attente. La file est bornée (les plus anciens
//! sont abandonnés).

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil::{read_json, write_json};
use crate::snapshot::Snapshot;

/// Capacité par défaut.
pub const DEFAULT_CAPACITY: usize = 20;

/// Élément en attente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedSnapshot {
    /// Clé du compte (cf. [`crate::snapshot::account_key`]).
    pub key: String,
    pub queued_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub snapshot: Snapshot,
}

/// File persistée dans un fichier JSON.
#[derive(Debug)]
pub struct RetryQueue {
    path: PathBuf,
    capacity: usize,
    items: VecDeque<QueuedSnapshot>,
}

impl RetryQueue {
    /// Charge la file ; un fichier corrompu donne une file vide et l'erreur.
    pub fn load(path: &Path, capacity: usize) -> (Self, Option<io::Error>) {
        let (items, err) = match read_json::<VecDeque<QueuedSnapshot>>(path) {
            Ok(items) => (items.unwrap_or_default(), None),
            Err(e) => (VecDeque::new(), Some(e)),
        };
        let mut q = RetryQueue {
            path: path.to_path_buf(),
            capacity: capacity.max(1),
            items,
        };
        while q.items.len() > q.capacity {
            q.items.pop_front();
        }
        (q, err)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> impl Iterator<Item = &QueuedSnapshot> + '_ {
        self.items.iter()
    }

    pub fn front(&self) -> Option<&QueuedSnapshot> {
        self.items.front()
    }

    /// Hash en attente pour un compte.
    pub fn pending_hash(&self, key: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|i| i.key == key)
            .map(|i| i.snapshot.snapshot_hash.as_str())
    }

    /// Ajoute (remplace l'élément du même compte) et sauvegarde. Renvoie les
    /// éléments abandonnés faute de place.
    pub fn push(&mut self, item: QueuedSnapshot) -> io::Result<Vec<QueuedSnapshot>> {
        self.items.retain(|i| i.key != item.key);
        self.items.push_back(item);
        let mut dropped = Vec::new();
        while self.items.len() > self.capacity {
            dropped.extend(self.items.pop_front());
        }
        self.save()?;
        Ok(dropped)
    }

    /// Retire l'élément d'un compte (un snapshot plus récent vient d'être
    /// accepté : l'ancien ne doit jamais repasser après lui).
    pub fn remove_key(&mut self, key: &str) -> io::Result<bool> {
        let before = self.items.len();
        self.items.retain(|i| i.key != key);
        if self.items.len() == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// Retire l'élément de tête (envoyé ou abandonné) et sauvegarde.
    pub fn pop_front(&mut self) -> io::Result<Option<QueuedSnapshot>> {
        let item = self.items.pop_front();
        self.save()?;
        Ok(item)
    }

    /// Note un échec sur l'élément de tête et sauvegarde.
    pub fn mark_front_failed(&mut self, error: &str) -> io::Result<()> {
        if let Some(front) = self.items.front_mut() {
            front.attempts = front.attempts.saturating_add(1);
            front.last_error = Some(error.to_owned());
        }
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        write_json(&self.path, &self.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SnapshotSource;

    fn item(key: &str, hash: &str) -> QueuedSnapshot {
        QueuedSnapshot {
            key: key.into(),
            queued_at: 1,
            attempts: 0,
            last_error: None,
            snapshot: Snapshot {
                model_version: 1,
                bridge_version: "0.1.0".into(),
                snapshot_hash: hash.into(),
                source: SnapshotSource {
                    account_file: "WTF/Account/A/SavedVariables/GuildSelfForever.lua".into(),
                    account_mtime: Some(10),
                    character_files: vec![],
                    game_running: false,
                },
                guilds: serde_json::json!([{"guild_key_normalized": "royaume#guilde", "nom": "Fumée"}]),
            },
        }
    }

    #[test]
    fn persists_across_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        let (mut q, err) = RetryQueue::load(&path, 3);
        assert!(err.is_none() && q.is_empty());
        q.push(item("a", "h1")).unwrap();
        q.push(item("b", "h2")).unwrap();
        q.mark_front_failed("backend injoignable").unwrap();
        let (q2, _) = RetryQueue::load(&path, 3);
        assert_eq!(q2.len(), 2);
        let front = q2.front().unwrap();
        assert_eq!((front.key.as_str(), front.attempts), ("a", 1));
        assert_eq!(front.last_error.as_deref(), Some("backend injoignable"));
        assert_eq!(front.snapshot, item("a", "h1").snapshot);
    }

    #[test]
    fn same_account_replaces_and_capacity_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        let (mut q, _) = RetryQueue::load(&path, 2);
        q.push(item("a", "h1")).unwrap();
        q.push(item("a", "h2")).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q.pending_hash("a"), Some("h2"));
        q.push(item("b", "h3")).unwrap();
        let dropped = q.push(item("c", "h4")).unwrap();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].key, "a");
        let keys: Vec<_> = q.items().map(|i| i.key.clone()).collect();
        assert_eq!(keys, ["b", "c"]);
        assert!(q.remove_key("c").unwrap());
        assert!(!q.remove_key("c").unwrap());
        assert_eq!(q.pop_front().unwrap().unwrap().key, "b");
        assert_eq!(q.pop_front().unwrap(), None);
        let (q2, _) = RetryQueue::load(&path, 2);
        assert!(q2.is_empty());
    }

    #[test]
    fn corrupt_file_gives_empty_queue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        std::fs::write(&path, b"[{").unwrap();
        let (q, err) = RetryQueue::load(&path, 2);
        assert!(q.is_empty());
        assert!(err.is_some());
    }
}
