use crate::tmux::PaneInfo;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SessionRuntime {
    pub status: String,
    pub panes: Vec<PaneInfo>,
    pub polled_at: Option<String>,
    pub last_error: Option<String>,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            status: "stopped".to_string(),
            panes: Vec::new(),
            polled_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CiRuntimeSummary {
    pub provider: String,
    pub status: String,
    pub data_json: Option<serde_json::Value>,
    pub tool_message: Option<String>,
    pub polled_at: Option<String>,
    pub next_poll_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AtmRuntimeSummary {
    pub state: String,
    pub last_transition: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AtmRuntimeUpdate {
    pub session_name: String,
    pub state: String,
    pub last_transition: Option<String>,
}

#[derive(Debug, Default)]
pub struct RuntimeProjection {
    sessions: HashMap<String, SessionRuntime>,
    discovery: HashMap<String, Vec<PaneInfo>>,
    ci_by_session: HashMap<String, Vec<CiRuntimeSummary>>,
    ci_next_due: HashMap<(i64, String), DateTime<Utc>>,
    atm_by_session: HashMap<String, AtmRuntimeSummary>,
}

impl RuntimeProjection {
    pub fn mark_starting(&mut self, session_name: &str) {
        let entry = self.sessions.entry(session_name.to_string()).or_default();
        entry.status = "starting".to_string();
        entry.last_error = None;
        entry.polled_at = Some(Utc::now().to_rfc3339());
    }

    pub fn mark_start_failed(&mut self, session_name: &str, error: String) {
        let entry = self.sessions.entry(session_name.to_string()).or_default();
        entry.status = "stopped".to_string();
        entry.last_error = Some(error);
        entry.polled_at = Some(Utc::now().to_rfc3339());
        entry.panes.clear();
    }

    pub fn mark_stopped(&mut self, session_name: &str) {
        let entry = self.sessions.entry(session_name.to_string()).or_default();
        entry.status = "stopped".to_string();
        entry.polled_at = Some(Utc::now().to_rfc3339());
        entry.panes.clear();
    }

    pub fn apply_tmux_snapshot(
        &mut self,
        defined_sessions: &[String],
        live_sessions: &HashMap<String, Vec<PaneInfo>>,
        polled_at: &str,
    ) {
        self.discovery = live_sessions.clone();

        for session_name in defined_sessions {
            let derived_status = derive_live_status(&self.atm_by_session, session_name);
            let entry = self.sessions.entry(session_name.clone()).or_default();
            if let Some(panes) = live_sessions.get(session_name) {
                entry.panes = panes.clone();
                entry.status = derived_status;
                entry.last_error = None;
            } else if entry.status != "starting" {
                entry.status = "stopped".to_string();
                entry.panes.clear();
            }
            entry.polled_at = Some(polled_at.to_string());
        }
    }

    pub fn ci_due(&self, session_id: i64, provider: &str, now: DateTime<Utc>) -> bool {
        let key = (session_id, provider.to_string());
        self.ci_next_due.get(&key).is_none_or(|due| *due <= now)
    }

    pub fn upsert_ci(
        &mut self,
        session_name: &str,
        session_id: i64,
        entry: CiRuntimeSummary,
        next_due: DateTime<Utc>,
    ) {
        let key = (session_id, entry.provider.clone());
        self.ci_next_due.insert(key, next_due);

        let items = self
            .ci_by_session
            .entry(session_name.to_string())
            .or_default();
        if let Some(existing) = items
            .iter_mut()
            .find(|item| item.provider == entry.provider)
        {
            *existing = entry;
            return;
        }
        items.push(entry);
        items.sort_by(|left, right| left.provider.cmp(&right.provider));
    }

    pub fn apply_atm_updates(&mut self, updates: Vec<AtmRuntimeUpdate>) {
        let mut aggregated: HashMap<String, AtmRuntimeSummary> = HashMap::new();

        for update in updates {
            let key = update.session_name;
            let candidate = AtmRuntimeSummary {
                state: normalize_atm_state(&update.state).to_string(),
                last_transition: update.last_transition,
            };

            let current = aggregated.entry(key).or_default();
            if state_priority(&candidate.state) > state_priority(&current.state) {
                *current = candidate;
                continue;
            }
            if current.last_transition.is_none() {
                current.last_transition = candidate.last_transition;
            }
        }

        self.atm_by_session = aggregated;

        // Recompute live status after ATM updates.
        for (session_name, entry) in &mut self.sessions {
            if entry.status == "stopped" || entry.status == "starting" {
                continue;
            }
            if !entry.panes.is_empty() {
                entry.status = derive_live_status(&self.atm_by_session, session_name);
            }
        }
    }

    pub fn clear_atm(&mut self) {
        self.atm_by_session.clear();
    }

    pub fn session(&self, session_name: &str) -> Option<&SessionRuntime> {
        self.sessions.get(session_name)
    }

    pub fn ci_for_session(&self, session_name: &str) -> Vec<CiRuntimeSummary> {
        self.ci_by_session
            .get(session_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn atm_for_session(&self, session_name: &str) -> Option<AtmRuntimeSummary> {
        self.atm_by_session.get(session_name).cloned()
    }

    pub fn discovery_rows(&self) -> Vec<DiscoverySession> {
        let mut rows = self
            .discovery
            .iter()
            .map(|(name, panes)| DiscoverySession {
                name: name.clone(),
                panes: panes.clone(),
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.name.cmp(&right.name));
        rows
    }

    pub fn has_live_sessions(&self) -> bool {
        self.sessions
            .values()
            .any(|entry| matches!(entry.status.as_str(), "starting" | "running" | "idle"))
    }

    pub fn live_session_count(&self) -> i64 {
        self.sessions
            .values()
            .filter(|entry| matches!(entry.status.as_str(), "starting" | "running" | "idle"))
            .count() as i64
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiscoverySession {
    pub name: String,
    pub panes: Vec<PaneInfo>,
}

fn normalize_atm_state(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "active" => "active",
        "stuck" => "stuck",
        "idle" => "idle",
        "offline" => "offline",
        _ => "unknown",
    }
}

fn state_priority(state: &str) -> u8 {
    match normalize_atm_state(state) {
        "active" => 5,
        "stuck" => 4,
        "idle" => 3,
        "offline" => 2,
        _ => 1,
    }
}

fn derive_live_status(
    atm_by_session: &HashMap<String, AtmRuntimeSummary>,
    session_name: &str,
) -> String {
    let Some(atm) = atm_by_session.get(session_name) else {
        return "running".to_string();
    };
    match normalize_atm_state(&atm.state) {
        "idle" | "offline" => "idle".to_string(),
        _ => "running".to_string(),
    }
}
