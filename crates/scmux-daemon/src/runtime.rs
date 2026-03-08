use crate::tmux::PaneInfo;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

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
    pub team: String,
    pub agent: String,
    pub state: String,
    pub last_transition: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfiguredPane {
    pub name: Option<String>,
    pub command: Option<String>,
    pub atm_team: Option<String>,
    pub atm_agent: Option<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AtmPaneKey {
    team: String,
    agent: String,
}

#[derive(Debug, Default)]
pub struct RuntimeProjection {
    sessions: HashMap<String, SessionRuntime>,
    discovery: HashMap<String, Vec<PaneInfo>>,
    ci_by_session: HashMap<String, Vec<CiRuntimeSummary>>,
    ci_next_due: HashMap<(i64, String), DateTime<Utc>>,
    atm_by_pane: HashMap<AtmPaneKey, AtmRuntimeSummary>,
    pane_keys_by_session: HashMap<String, Vec<Option<AtmPaneKey>>>,
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
        self.pane_keys_by_session
            .insert(session_name.to_string(), Vec::new());
    }

    pub fn mark_stopped(&mut self, session_name: &str) {
        let entry = self.sessions.entry(session_name.to_string()).or_default();
        entry.status = "stopped".to_string();
        entry.polled_at = Some(Utc::now().to_rfc3339());
        entry.panes.clear();
        self.pane_keys_by_session
            .insert(session_name.to_string(), Vec::new());
    }

    pub fn apply_tmux_snapshot(
        &mut self,
        defined_sessions: &[String],
        live_sessions: &HashMap<String, Vec<PaneInfo>>,
        pane_configs: &HashMap<String, Vec<ConfiguredPane>>,
        polled_at: &str,
    ) {
        self.discovery = live_sessions.clone();
        let defined = defined_sessions
            .iter()
            .cloned()
            .collect::<HashSet<String>>();
        self.sessions.retain(|name, _| defined.contains(name));
        self.pane_keys_by_session
            .retain(|name, _| defined.contains(name));
        self.ci_by_session.retain(|name, _| defined.contains(name));

        for session_name in defined_sessions {
            let live = live_sessions.get(session_name);
            let configured = pane_configs.get(session_name).cloned().unwrap_or_default();
            let (projected_panes, projected_keys) =
                project_panes(live, &configured, &self.atm_by_pane);
            self.pane_keys_by_session
                .insert(session_name.clone(), projected_keys);

            let entry = self.sessions.entry(session_name.clone()).or_default();
            if live.is_some() {
                entry.panes = projected_panes;
                entry.status = derive_live_status(&entry.panes);
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
        let mut mapped = HashMap::new();
        for update in updates {
            let key = AtmPaneKey {
                team: update.team.trim().to_string(),
                agent: update.agent.trim().to_string(),
            };
            if key.team.is_empty() || key.agent.is_empty() {
                continue;
            }
            let candidate = AtmRuntimeSummary {
                state: normalize_atm_state(&update.state).to_string(),
                last_transition: update.last_transition,
            };
            let existing = mapped.entry(key).or_insert_with(|| candidate.clone());
            if state_priority(&candidate.state) > state_priority(&existing.state) {
                *existing = candidate;
            } else if existing.last_transition.is_none() {
                existing.last_transition = candidate.last_transition;
            }
        }
        self.atm_by_pane = mapped;

        // Recompute pane/session status using canonical ATM pane keys.
        for (session_name, entry) in &mut self.sessions {
            if let Some(keys) = self.pane_keys_by_session.get(session_name) {
                for (idx, pane) in entry.panes.iter_mut().enumerate() {
                    let Some(key) = keys.get(idx).and_then(|k| k.as_ref()) else {
                        continue;
                    };
                    if let Some(atm) = self.atm_by_pane.get(key) {
                        pane.status = normalize_atm_state(&atm.state).to_string();
                    }
                }
            }
            if entry.status != "stopped" && entry.status != "starting" && !entry.panes.is_empty() {
                entry.status = derive_live_status(&entry.panes);
            }
        }
    }

    pub fn clear_atm(&mut self) {
        self.atm_by_pane.clear();
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
        let keys = self.pane_keys_by_session.get(session_name)?;
        let mut selected: Option<AtmRuntimeSummary> = None;
        for key in keys.iter().flatten() {
            let Some(candidate) = self.atm_by_pane.get(key) else {
                continue;
            };
            match selected.as_mut() {
                Some(current) => {
                    if state_priority(&candidate.state) > state_priority(&current.state) {
                        *current = candidate.clone();
                    } else if current.last_transition.is_none() {
                        current.last_transition = candidate.last_transition.clone();
                    }
                }
                None => {
                    selected = Some(candidate.clone());
                }
            }
        }
        selected
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

fn make_atm_key(team: Option<&str>, agent: Option<&str>) -> Option<AtmPaneKey> {
    let team = team?.trim();
    let agent = agent?.trim();
    if team.is_empty() || agent.is_empty() {
        return None;
    }
    Some(AtmPaneKey {
        team: team.to_string(),
        agent: agent.to_string(),
    })
}

fn project_panes(
    live: Option<&Vec<PaneInfo>>,
    configured: &[ConfiguredPane],
    atm_by_pane: &HashMap<AtmPaneKey, AtmRuntimeSummary>,
) -> (Vec<PaneInfo>, Vec<Option<AtmPaneKey>>) {
    let Some(live_panes) = live else {
        return (Vec::new(), Vec::new());
    };

    if configured.is_empty() {
        return (live_panes.clone(), vec![None; live_panes.len()]);
    }

    let mut panes = Vec::new();
    let mut keys = Vec::new();

    for (idx, definition) in configured.iter().enumerate() {
        let live_pane = live_panes.get(idx);
        let key = make_atm_key(
            definition.atm_team.as_deref(),
            definition.atm_agent.as_deref(),
        );
        let status = key
            .as_ref()
            .and_then(|atm_key| atm_by_pane.get(atm_key).map(|atm| atm.state.clone()))
            .or_else(|| live_pane.map(|pane| normalize_atm_state(&pane.status).to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        panes.push(PaneInfo {
            index: live_pane.map(|pane| pane.index).unwrap_or(idx as u32),
            name: definition
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| live_pane.map(|pane| pane.name.clone()))
                .unwrap_or_else(|| format!("pane-{idx}")),
            status,
            last_activity: live_pane
                .map(|pane| pane.last_activity.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            current_command: definition
                .command
                .as_deref()
                .filter(|command| !command.trim().is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| live_pane.map(|pane| pane.current_command.clone()))
                .unwrap_or_default(),
        });
        keys.push(key);
    }

    if live_panes.len() > configured.len() {
        for pane in &live_panes[configured.len()..] {
            panes.push(pane.clone());
            keys.push(None);
        }
    }

    (panes, keys)
}

fn derive_live_status(panes: &[PaneInfo]) -> String {
    if panes.is_empty() {
        return "running".to_string();
    }

    if panes
        .iter()
        .any(|pane| matches!(normalize_atm_state(&pane.status), "active" | "stuck"))
    {
        return "running".to_string();
    }

    if panes
        .iter()
        .all(|pane| matches!(normalize_atm_state(&pane.status), "idle" | "offline"))
    {
        return "idle".to_string();
    }

    "running".to_string()
}
