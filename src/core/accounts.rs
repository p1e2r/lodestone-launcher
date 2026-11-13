//! Offline accounts: just usernames.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use super::paths::paths;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Account {
    pub username: String,
    #[serde(default)]
    pub created: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    pub selected: Option<String>,
}

fn file() -> PathBuf {
    paths().root.join("accounts.json")
}

pub fn valid_username(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Accounts {
    pub fn load() -> Accounts {
        std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::write(file(), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn selected(&self) -> Option<&Account> {
        let sel = self.selected.as_deref();
        self.accounts.iter().find(|a| Some(a.username.as_str()) == sel).or(self.accounts.first())
    }

    pub fn add(&mut self, username: &str) -> Result<&Account> {
        if !valid_username(username) {
            return Err(anyhow!("Usernames are 3–16 letters, numbers or _"));
        }
        if self.accounts.iter().any(|a| a.username.eq_ignore_ascii_case(username)) {
            return Err(anyhow!("{username} already exists"));
        }
        self.accounts.push(Account { username: username.to_string(), created: chrono::Utc::now().timestamp() });
        self.selected = Some(username.to_string());
        self.save()?;
        Ok(self.accounts.last().unwrap())
    }

    pub fn remove(&mut self, username: &str) -> Result<()> {
        self.accounts.retain(|a| a.username != username);
        if self.selected.as_deref() == Some(username) {
            self.selected = self.accounts.first().map(|a| a.username.clone());
        }
        self.save()
    }
}

impl Account {
    /// The UUID offline-mode servers give this player.
    pub fn offline_uuid(&self) -> String {
        super::launch::dashed_uuid(&super::launch::offline_uuid(&self.username))
    }
}
