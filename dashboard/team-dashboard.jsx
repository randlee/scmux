import { useState } from "react";

const PROJECT_COLORS = {
  "radiant-p3": "#3b82f6",
  "atm": "#10b981",
  "beads": "#f59e0b",
  "provenance": "#8b5cf6",
  "synaptic-canvas": "#ec4899",
  "claude-history": "#06b6d4",
  "ui-platform": "#f97316",
  "dolt-registry": "#84cc16",
};

const STATUS_DOT = {
  active: { color: "#10b981", pulse: true },
  idle: { color: "#f59e0b", pulse: false },
  blocked: { color: "#ef4444", pulse: true },
  stopped: { color: "#1e2535", pulse: false },
  running: { color: "#10b981", pulse: true },
};

const TEAMS = [
  { name: "ui-template", project: "radiant-p3", sessionStatus: "running", prs: [{ num: 42, title: "Add template renderer", url: "#" }, { num: 43, title: "Fix layout props", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "2m ago" }, { name: "architect", status: "idle", lastActivity: "8m ago" }, { name: "implementer", status: "active", lastActivity: "1m ago" }] },
  { name: "p3-backend", project: "radiant-p3", sessionStatus: "running", prs: [{ num: 55, title: "Subagent session extraction", url: "#" }], panes: [{ name: "architect", status: "active", lastActivity: "30s ago" }, { name: "implementer", status: "active", lastActivity: "1m ago" }, { name: "tester", status: "idle", lastActivity: "12m ago" }] },
  { name: "p3-calibration", project: "radiant-p3", sessionStatus: "running", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "5m ago" }, { name: "implementer", status: "idle", lastActivity: "9m ago" }, { name: "tester", status: "active", lastActivity: "2m ago" }] },
  { name: "atm-daemon", project: "atm", sessionStatus: "running", prs: [{ num: 12, title: "Slack bridge plugin", url: "#" }], panes: [{ name: "rust-dev", status: "active", lastActivity: "4m ago" }, { name: "tui", status: "idle", lastActivity: "22m ago" }, { name: "tester", status: "idle", lastActivity: "18m ago" }] },
  { name: "atm-tui", project: "atm", sessionStatus: "idle", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "32m ago" }, { name: "implementer", status: "idle", lastActivity: "45m ago" }, { name: "tester", status: "stopped", lastActivity: "1h ago" }] },
  { name: "atm-slack-bridge", project: "atm", sessionStatus: "stopped", prs: [{ num: 8, title: "Socket mode per agent", url: "#" }], panes: [{ name: "architect", status: "stopped", lastActivity: "3h ago" }, { name: "implementer", status: "stopped", lastActivity: "3h ago" }] },
  { name: "dagu-bootstrap", project: "atm", sessionStatus: "running", prs: [], panes: [{ name: "team-lead", status: "active", lastActivity: "1m ago" }, { name: "architect", status: "active", lastActivity: "3m ago" }, { name: "implementer", status: "idle", lastActivity: "7m ago" }] },
  { name: "codex-mcp", project: "atm", sessionStatus: "idle", prs: [{ num: 3, title: "JSON-RPC wrapper", url: "#" }], panes: [{ name: "team-lead", status: "idle", lastActivity: "1h ago" }, { name: "implementer", status: "idle", lastActivity: "1h ago" }] },
  { name: "beads-editor", project: "beads", sessionStatus: "stopped", prs: [], panes: [{ name: "react-flow", status: "stopped", lastActivity: "1d ago" }, { name: "architect", status: "stopped", lastActivity: "1d ago" }] },
  { name: "beads-molecules", project: "beads", sessionStatus: "stopped", prs: [{ num: 7, title: "Jinja2 conditionals", url: "#" }], panes: [{ name: "team-lead", status: "stopped", lastActivity: "2d ago" }, { name: "implementer", status: "stopped", lastActivity: "2d ago" }] },
  { name: "kuzu-schema", project: "provenance", sessionStatus: "running", prs: [{ num: 21, title: "File provenance links", url: "#" }, { num: 22, title: "Agent ID indexing", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "3m ago" }, { name: "architect", status: "active", lastActivity: "5m ago" }, { name: "tester", status: "idle", lastActivity: "11m ago" }] },
  { name: "neo4j-migration", project: "provenance", sessionStatus: "idle", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "40m ago" }, { name: "implementer", status: "idle", lastActivity: "50m ago" }] },
  { name: "genealogy-graph", project: "provenance", sessionStatus: "stopped", prs: [], panes: [{ name: "team-lead", status: "stopped", lastActivity: "5d ago" }, { name: "researcher", status: "stopped", lastActivity: "5d ago" }] },
  { name: "sc-marketplace", project: "synaptic-canvas", sessionStatus: "running", prs: [{ num: 33, title: "Tier 2 packaging", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "1m ago" }, { name: "implementer", status: "active", lastActivity: "2m ago" }, { name: "tester", status: "active", lastActivity: "4m ago" }] },
  { name: "sc-hooks", project: "synaptic-canvas", sessionStatus: "idle", prs: [], panes: [{ name: "architect", status: "idle", lastActivity: "25m ago" }, { name: "implementer", status: "idle", lastActivity: "30m ago" }] },
  { name: "sc-plugin-devkit", project: "synaptic-canvas", sessionStatus: "stopped", prs: [{ num: 19, title: "Pytest fixture harness", url: "#" }], panes: [{ name: "team-lead", status: "stopped", lastActivity: "4h ago" }, { name: "tester", status: "stopped", lastActivity: "4h ago" }] },
  { name: "claude-history-cli", project: "claude-history", sessionStatus: "running", prs: [{ num: 53, title: "Subagent extraction", url: "#" }, { num: 54, title: "PreCompact hooks", url: "#" }, { num: 55, title: "PostCompact workflow", url: "#" }], panes: [{ name: "rust-dev", status: "active", lastActivity: "2m ago" }, { name: "tester", status: "idle", lastActivity: "14m ago" }, { name: "architect", status: "idle", lastActivity: "20m ago" }] },
  { name: "history-search", project: "claude-history", sessionStatus: "idle", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "55m ago" }, { name: "implementer", status: "idle", lastActivity: "1h ago" }] },
  { name: "ui-components", project: "ui-platform", sessionStatus: "running", prs: [{ num: 9, title: "Design system tokens", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "6m ago" }, { name: "implementer", status: "active", lastActivity: "8m ago" }, { name: "tester", status: "idle", lastActivity: "15m ago" }] },
  { name: "ui-design-system", project: "ui-platform", sessionStatus: "stopped", prs: [], panes: [{ name: "architect", status: "stopped", lastActivity: "6h ago" }, { name: "implementer", status: "stopped", lastActivity: "6h ago" }] },
  { name: "dolt-agent-reg", project: "dolt-registry", sessionStatus: "running", prs: [{ num: 4, title: "CLI management interface", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "3m ago" }, { name: "architect", status: "idle", lastActivity: "9m ago" }, { name: "implementer", status: "active", lastActivity: "5m ago" }] },
  { name: "dolt-cam-db", project: "dolt-registry", sessionStatus: "idle", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "35m ago" }, { name: "implementer", status: "idle", lastActivity: "40m ago" }] },
  { name: "dolt-req-db", project: "dolt-registry", sessionStatus: "stopped", prs: [{ num: 2, title: "ADR schema migration", url: "#" }], panes: [{ name: "architect", status: "stopped", lastActivity: "8h ago" }, { name: "implementer", status: "stopped", lastActivity: "8h ago" }] },
  { name: "adr-tracker", project: "dolt-registry", sessionStatus: "idle", prs: [], panes: [{ name: "team-lead", status: "idle", lastActivity: "2h ago" }, { name: "implementer", status: "idle", lastActivity: "2h ago" }] },
  { name: "roslyn-rdf", project: "radiant-p3", sessionStatus: "stopped", prs: [], panes: [{ name: "architect", status: "stopped", lastActivity: "3d ago" }, { name: "implementer", status: "stopped", lastActivity: "3d ago" }] },
  { name: "nurbs-sensor", project: "radiant-p3", sessionStatus: "idle", prs: [{ num: 61, title: "NURBS bias correction", url: "#" }], panes: [{ name: "team-lead", status: "idle", lastActivity: "1h ago" }, { name: "implementer", status: "idle", lastActivity: "1h ago" }] },
  { name: "bayer-cnn", project: "radiant-p3", sessionStatus: "stopped", prs: [], panes: [{ name: "architect", status: "stopped", lastActivity: "2d ago" }, { name: "implementer", status: "stopped", lastActivity: "2d ago" }] },
  { name: "rust-imgproc", project: "radiant-p3", sessionStatus: "running", prs: [{ num: 17, title: "UnaryMap shape impl", url: "#" }], panes: [{ name: "rust-dev", status: "active", lastActivity: "1m ago" }, { name: "architect", status: "active", lastActivity: "3m ago" }, { name: "tester", status: "idle", lastActivity: "10m ago" }] },
  { name: "mip-schema", project: "synaptic-canvas", sessionStatus: "stopped", prs: [], panes: [{ name: "architect", status: "stopped", lastActivity: "4d ago" }, { name: "implementer", status: "stopped", lastActivity: "4d ago" }] },
  { name: "p3-perf-bench", project: "radiant-p3", sessionStatus: "running", prs: [{ num: 71, title: "Benchmark harness", url: "#" }], panes: [{ name: "team-lead", status: "active", lastActivity: "4m ago" }, { name: "implementer", status: "active", lastActivity: "6m ago" }, { name: "tester", status: "active", lastActivity: "8m ago" }] },
];

function Dot({ status, size = 7 }) {
  const s = STATUS_DOT[status] || STATUS_DOT.stopped;
  return (
    <span style={{ position: "relative", display: "inline-flex", alignItems: "center", justifyContent: "center", width: size, height: size, flexShrink: 0 }}>
      {s.pulse && <span style={{ position: "absolute", inset: 0, borderRadius: "50%", backgroundColor: s.color, opacity: 0.35, animation: "ping 1.5s cubic-bezier(0,0,0.2,1) infinite" }} />}
      <span style={{ width: size, height: size, borderRadius: "50%", backgroundColor: s.color, display: "block" }} />
    </span>
  );
}

function JumpModal({ team, onClose }) {
  if (!team) return null;
  const pc = PROJECT_COLORS[team.project] || "#3b82f6";
  const cmd = `wezterm start -- tmux attach -t ${team.name}`;
  return (
    <div style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.8)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 200, backdropFilter: "blur(6px)" }} onClick={onClose}>
      <div style={{ background: "#0d1117", border: `1px solid ${pc}50`, borderRadius: 12, padding: 28, minWidth: 380, fontFamily: "inherit" }} onClick={e => e.stopPropagation()}>
        <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.12em", marginBottom: 6 }}>JUMP TO SESSION</div>
        <div style={{ fontSize: 20, color: "#f1f5f9", fontWeight: 700, marginBottom: 3 }}>{team.name}</div>
        <div style={{ fontSize: 11, color: pc, marginBottom: 20 }}>{team.project}</div>

        <div style={{ background: "#060810", borderRadius: 6, padding: "10px 14px", marginBottom: 20, fontSize: 11, color: "#475569" }}>
          <span style={{ color: "#334155" }}>$ </span>{cmd}
        </div>

        <div style={{ marginBottom: 18 }}>
          <div style={{ fontSize: 10, color: "#1e2535", letterSpacing: "0.1em", marginBottom: 8 }}>PANES</div>
          {team.panes.map((p, i) => (
            <div key={i} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 0", borderBottom: i < team.panes.length - 1 ? "1px solid #0d1117" : "none" }}>
              <Dot status={p.status} size={6} />
              <span style={{ fontSize: 12, color: "#94a3b8", flex: 1 }}>{p.name}</span>
              <span style={{ fontSize: 10, color: "#334155" }}>{p.lastActivity}</span>
            </div>
          ))}
        </div>

        {team.prs.length > 0 && (
          <div style={{ marginBottom: 20 }}>
            <div style={{ fontSize: 10, color: "#1e2535", letterSpacing: "0.1em", marginBottom: 8 }}>OPEN PRS</div>
            {team.prs.map((pr, i) => (
              <a key={i} href={pr.url} style={{ display: "flex", alignItems: "center", gap: 8, padding: "5px 0", textDecoration: "none", borderBottom: i < team.prs.length - 1 ? "1px solid #0d1117" : "none" }}>
                <span style={{ fontSize: 10, color: pc, background: `${pc}18`, borderRadius: 3, padding: "2px 6px", flexShrink: 0 }}>#{pr.num}</span>
                <span style={{ fontSize: 11, color: "#64748b", flex: 1 }}>{pr.title}</span>
                <span style={{ fontSize: 11, color: "#334155" }}>↗</span>
              </a>
            ))}
          </div>
        )}

        <div style={{ display: "flex", gap: 8 }}>
          <button style={{ flex: 1, padding: "10px 0", background: pc, border: "none", borderRadius: 6, color: "#fff", fontSize: 12, fontWeight: 700, cursor: "pointer", fontFamily: "inherit", letterSpacing: "0.05em" }}>
            Open in WezTerm →
          </button>
          <button style={{ padding: "10px 16px", background: "transparent", border: "1px solid #1e2535", borderRadius: 6, color: "#475569", fontSize: 12, cursor: "pointer", fontFamily: "inherit" }} onClick={onClose}>
            esc
          </button>
        </div>
      </div>
    </div>
  );
}

function GridCard({ team, onJump }) {
  const pc = PROJECT_COLORS[team.project] || "#6b7280";
  const activePanes = team.panes.filter(p => p.status === "active").length;
  const isStopped = team.sessionStatus === "stopped";
  return (
    <div onClick={() => onJump(team)} style={{ background: "#0d1117", border: "1px solid #131820", borderRadius: 8, overflow: "hidden", cursor: "pointer", opacity: isStopped ? 0.5 : 1, transition: "border-color 0.15s, transform 0.1s" }}
      onMouseEnter={e => { e.currentTarget.style.borderColor = pc + "55"; e.currentTarget.style.transform = "translateY(-1px)"; }}
      onMouseLeave={e => { e.currentTarget.style.borderColor = "#131820"; e.currentTarget.style.transform = "translateY(0)"; }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid #0a0e14" }}>
        <div style={{ width: 3, height: 26, borderRadius: 2, background: pc, flexShrink: 0 }} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: "#e2e8f0", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{team.name}</div>
          <div style={{ fontSize: 10, color: pc, opacity: 0.8 }}>{team.project}</div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 3, flexShrink: 0 }}>
          <Dot status={team.sessionStatus} size={7} />
          {!isStopped && <span style={{ fontSize: 9, color: "#334155" }}>{activePanes}/{team.panes.length}</span>}
        </div>
      </div>
      <div style={{ padding: "6px 12px 8px" }}>
        {team.panes.map((pane, i) => (
          <div key={i} style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
            <Dot status={pane.status} size={5} />
            <span style={{ fontSize: 10, color: pane.status === "active" ? "#94a3b8" : "#2d3748", flex: 1 }}>{pane.name}</span>
            <span style={{ fontSize: 9, color: "#1a2030" }}>{pane.lastActivity}</span>
          </div>
        ))}
        {team.prs.length > 0 && (
          <div style={{ marginTop: 6, paddingTop: 5, borderTop: "1px solid #0a0e14", display: "flex", gap: 3, flexWrap: "wrap" }}>
            {team.prs.map((pr, i) => (
              <span key={i} style={{ fontSize: 9, color: pc, background: `${pc}18`, borderRadius: 3, padding: "1px 5px" }}>#{pr.num}</span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ListView({ teams, onJump }) {
  return (
    <div style={{ padding: "0 24px 24px" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
        <thead>
          <tr style={{ borderBottom: "1px solid #131820" }}>
            {["", "Session", "Project", "Status", "Panes", "Active", "Open PRs", "Last Activity"].map((h, i) => (
              <th key={i} style={{ padding: "8px 12px", textAlign: "left", fontSize: 10, color: "#1e2535", letterSpacing: "0.1em", fontWeight: 600 }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {teams.map((team, i) => {
            const pc = PROJECT_COLORS[team.project] || "#6b7280";
            const activePanes = team.panes.filter(p => p.status === "active").length;
            return (
              <tr key={i} onClick={() => onJump(team)} style={{ borderBottom: "1px solid #0a0e14", cursor: "pointer", opacity: team.sessionStatus === "stopped" ? 0.5 : 1, transition: "background 0.1s" }}
                onMouseEnter={e => e.currentTarget.style.background = "#0f1117"}
                onMouseLeave={e => e.currentTarget.style.background = "transparent"}
              >
                <td style={{ padding: "7px 12px" }}><div style={{ width: 3, height: 18, borderRadius: 2, background: pc }} /></td>
                <td style={{ padding: "7px 12px", color: "#cbd5e1", fontWeight: 500 }}>{team.name}</td>
                <td style={{ padding: "7px 12px", color: pc, fontSize: 11 }}>{team.project}</td>
                <td style={{ padding: "7px 12px" }}><div style={{ display: "flex", alignItems: "center", gap: 6 }}><Dot status={team.sessionStatus} size={6} /><span style={{ color: "#334155", fontSize: 11 }}>{team.sessionStatus}</span></div></td>
                <td style={{ padding: "7px 12px", color: "#334155" }}>{team.panes.length}</td>
                <td style={{ padding: "7px 12px", color: activePanes > 0 ? "#10b981" : "#1e2535" }}>{activePanes}</td>
                <td style={{ padding: "7px 12px" }}>
                  <div style={{ display: "flex", gap: 3, flexWrap: "wrap" }}>
                    {team.prs.length === 0
                      ? <span style={{ color: "#1e2535" }}>—</span>
                      : team.prs.map((pr, j) => <span key={j} style={{ fontSize: 9, color: pc, background: `${pc}18`, borderRadius: 3, padding: "1px 5px" }}>#{pr.num}</span>)
                    }
                  </div>
                </td>
                <td style={{ padding: "7px 12px", color: "#1e2535", fontSize: 11 }}>{team.panes[0]?.lastActivity || "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function GroupedView({ teams, onJump }) {
  const byProject = {};
  teams.forEach(t => { if (!byProject[t.project]) byProject[t.project] = []; byProject[t.project].push(t); });
  return (
    <div style={{ padding: "0 24px 24px", display: "flex", flexDirection: "column", gap: 28 }}>
      {Object.entries(byProject).map(([project, projectTeams]) => {
        const pc = PROJECT_COLORS[project] || "#6b7280";
        const running = projectTeams.filter(t => t.sessionStatus === "running").length;
        const totalPRs = projectTeams.reduce((n, t) => n + t.prs.length, 0);
        return (
          <div key={project}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12, paddingBottom: 8, borderBottom: `1px solid ${pc}25` }}>
              <div style={{ width: 4, height: 16, borderRadius: 2, background: pc }} />
              <span style={{ fontSize: 12, fontWeight: 700, color: pc, letterSpacing: "0.06em" }}>{project}</span>
              <span style={{ fontSize: 10, color: "#1e2535" }}>{running}/{projectTeams.length} running</span>
              {totalPRs > 0 && <span style={{ fontSize: 10, color: pc, background: `${pc}18`, borderRadius: 3, padding: "1px 7px" }}>{totalPRs} PR{totalPRs !== 1 ? "s" : ""}</span>}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 8 }}>
              {projectTeams.map((team, i) => <GridCard key={i} team={team} onJump={onJump} />)}
            </div>
          </div>
        );
      })}
    </div>
  );
}

export default function Dashboard() {
  const [view, setView] = useState("grouped");
  const [statusFilter, setStatusFilter] = useState("all");
  const [projectFilter, setProjectFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [jumpTarget, setJumpTarget] = useState(null);

  const projects = [...new Set(TEAMS.map(t => t.project))];

  const filtered = TEAMS.filter(t => {
    if (statusFilter === "running" && t.sessionStatus !== "running") return false;
    if (statusFilter === "idle" && t.sessionStatus !== "idle") return false;
    if (statusFilter === "stopped" && t.sessionStatus !== "stopped") return false;
    if (projectFilter !== "all" && t.project !== projectFilter) return false;
    if (search && !t.name.includes(search.toLowerCase())) return false;
    return true;
  });

  const runningCount = TEAMS.filter(t => t.sessionStatus === "running").length;
  const idleCount = TEAMS.filter(t => t.sessionStatus === "idle").length;
  const stoppedCount = TEAMS.filter(t => t.sessionStatus === "stopped").length;
  const activeAgents = TEAMS.flatMap(t => t.panes).filter(p => p.status === "active").length;
  const openPRs = TEAMS.reduce((n, t) => n + t.prs.length, 0);

  return (
    <div style={{ background: "#060810", minHeight: "100vh", color: "#e2e8f0", fontFamily: "'Berkeley Mono', 'Fira Code', 'JetBrains Mono', monospace" }}>
      <style>{`
        @keyframes ping { 75%, 100% { transform: scale(2.2); opacity: 0; } }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        input::placeholder { color: #1a2030; }
      `}</style>

      {/* Top bar */}
      <div style={{ borderBottom: "1px solid #0a0e14", padding: "12px 24px", display: "flex", alignItems: "center", gap: 20, position: "sticky", top: 0, background: "#060810", zIndex: 10 }}>
        <div style={{ fontSize: 11, fontWeight: 800, color: "#475569", letterSpacing: "0.16em" }}>TEAM CONTROL</div>
        <div style={{ width: 1, height: 14, background: "#131820" }} />
        <div style={{ display: "flex", gap: 14, fontSize: 11 }}>
          <span><span style={{ color: "#10b981" }}>{runningCount}</span><span style={{ color: "#1e2535" }}> run</span></span>
          <span><span style={{ color: "#f59e0b" }}>{idleCount}</span><span style={{ color: "#1e2535" }}> idle</span></span>
          <span><span style={{ color: "#1e2535" }}>{stoppedCount}</span><span style={{ color: "#131820" }}> off</span></span>
          <span style={{ color: "#131820" }}>·</span>
          <span><span style={{ color: "#10b981" }}>{activeAgents}</span><span style={{ color: "#1e2535" }}> agents</span></span>
          <span style={{ color: "#131820" }}>·</span>
          <span><span style={{ color: "#3b82f6" }}>{openPRs}</span><span style={{ color: "#1e2535" }}> PRs</span></span>
        </div>
        <div style={{ flex: 1 }} />
        <input value={search} onChange={e => setSearch(e.target.value)} placeholder="search sessions…"
          style={{ background: "#0a0e14", border: "1px solid #131820", borderRadius: 5, padding: "5px 10px", fontSize: 11, color: "#94a3b8", outline: "none", width: 150 }} />
        <div style={{ display: "flex", background: "#0a0e14", borderRadius: 5, border: "1px solid #131820", overflow: "hidden" }}>
          {[["grid", "⊞ Grid"], ["list", "≡ List"], ["grouped", "❏ Project"]].map(([v, label]) => (
            <button key={v} onClick={() => setView(v)} style={{
              padding: "5px 12px", background: view === v ? "#1e2535" : "transparent",
              border: "none", color: view === v ? "#cbd5e1" : "#334155", cursor: "pointer",
              fontSize: 10, fontFamily: "inherit", letterSpacing: "0.04em", transition: "background 0.1s"
            }}>{label}</button>
          ))}
        </div>
      </div>

      {/* Filter bar */}
      <div style={{ padding: "8px 24px", borderBottom: "1px solid #0a0e14", display: "flex", gap: 5, flexWrap: "wrap", alignItems: "center" }}>
        {["all", "running", "idle", "stopped"].map(f => (
          <button key={f} onClick={() => setStatusFilter(f)} style={{
            padding: "3px 8px", borderRadius: 4, fontSize: 10, cursor: "pointer", fontFamily: "inherit",
            background: statusFilter === f ? "#131820" : "transparent",
            border: statusFilter === f ? "1px solid #1e2535" : "1px solid transparent",
            color: statusFilter === f ? "#94a3b8" : "#1e2535", letterSpacing: "0.04em"
          }}>{f}</button>
        ))}
        <div style={{ width: 1, height: 12, background: "#131820", margin: "0 4px" }} />
        <button onClick={() => setProjectFilter("all")} style={{
          padding: "3px 8px", borderRadius: 4, fontSize: 10, cursor: "pointer", fontFamily: "inherit",
          background: projectFilter === "all" ? "#131820" : "transparent",
          border: projectFilter === "all" ? "1px solid #1e2535" : "1px solid transparent",
          color: projectFilter === "all" ? "#94a3b8" : "#1e2535"
        }}>all projects</button>
        {projects.map(p => {
          const pc = PROJECT_COLORS[p] || "#6b7280";
          return (
            <button key={p} onClick={() => setProjectFilter(p)} style={{
              padding: "3px 8px", borderRadius: 4, fontSize: 10, cursor: "pointer", fontFamily: "inherit",
              background: projectFilter === p ? `${pc}15` : "transparent",
              border: projectFilter === p ? `1px solid ${pc}40` : "1px solid transparent",
              color: projectFilter === p ? pc : "#1e2535"
            }}>{p}</button>
          );
        })}
      </div>

      {/* Content */}
      <div style={{ paddingTop: 20 }}>
        {view === "grid" && (
          <div style={{ padding: "0 24px 24px", display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 10 }}>
            {filtered.map((team, i) => <GridCard key={i} team={team} onJump={setJumpTarget} />)}
          </div>
        )}
        {view === "list" && <ListView teams={filtered} onJump={setJumpTarget} />}
        {view === "grouped" && <GroupedView teams={filtered} onJump={setJumpTarget} />}
      </div>

      <JumpModal team={jumpTarget} onClose={() => setJumpTarget(null)} />
    </div>
  );
}
