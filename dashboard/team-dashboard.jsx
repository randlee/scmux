import { useEffect, useMemo, useState } from "react";

const PROJECT_COLORS = {
  "radiant-p3": "#3b82f6",
  atm: "#10b981",
  beads: "#f59e0b",
  provenance: "#8b5cf6",
  "synaptic-canvas": "#ec4899",
  "claude-history": "#06b6d4",
  "ui-platform": "#f97316",
  "dolt-registry": "#84cc16",
};

const STATUS_DOT = {
  active: { color: "#10b981", pulse: true },
  idle: { color: "#f59e0b", pulse: false },
  stuck: { color: "#ef4444", pulse: true },
  offline: { color: "#64748b", pulse: false },
  unknown: { color: "#334155", pulse: false },
  stopped: { color: "#1e2535", pulse: false },
  running: { color: "#10b981", pulse: true },
  starting: { color: "#60a5fa", pulse: true },
  done: { color: "#a78bfa", pulse: false },
};

const DEFAULT_BASE_URL = "http://localhost:7878";
const DEFAULT_POLL_MS = 15_000;
const FLOTILLA_V1_ENABLED = false;

function daemonBaseUrl() {
  if (typeof window === "undefined") {
    return DEFAULT_BASE_URL;
  }
  const { origin, protocol } = window.location;
  if (!origin || origin === "null" || protocol === "file:") {
    return DEFAULT_BASE_URL;
  }
  return origin;
}

function Dot({ status, size = 7 }) {
  const s = STATUS_DOT[status] || STATUS_DOT.unknown;
  return (
    <span
      style={{
        position: "relative",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: size,
        height: size,
        flexShrink: 0,
      }}
    >
      {s.pulse && (
        <span
          style={{
            position: "absolute",
            inset: 0,
            borderRadius: "50%",
            backgroundColor: s.color,
            opacity: 0.35,
            animation: "ping 1.5s cubic-bezier(0,0,0.2,1) infinite",
          }}
        />
      )}
      <span
        style={{
          width: size,
          height: size,
          borderRadius: "50%",
          backgroundColor: s.color,
          display: "block",
        }}
      />
    </span>
  );
}

function relativeTime(iso) {
  if (!iso) {
    return "unknown";
  }
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return "unknown";
  }
  const elapsedSec = Math.max(0, Math.floor((Date.now() - then) / 1000));
  if (elapsedSec < 60) {
    return `${elapsedSec}s ago`;
  }
  if (elapsedSec < 3600) {
    return `${Math.floor(elapsedSec / 60)}m ago`;
  }
  if (elapsedSec < 86400) {
    return `${Math.floor(elapsedSec / 3600)}h ago`;
  }
  return `${Math.floor(elapsedSec / 86400)}d ago`;
}

function hostLabel(host) {
  if (!host) {
    return "unknown-host";
  }
  return host.name || host.address || `host-${host.id}`;
}

function hostBadge(host) {
  if (!host) {
    return "unknown";
  }
  if (host.reachable) {
    return host.is_local ? "local" : "reachable";
  }
  return `last seen ${relativeTime(host.last_seen)}`;
}

function parseCiPayload(raw) {
  if (!raw) {
    return null;
  }
  if (typeof raw === "object") {
    return raw;
  }
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function normalizePanes(session) {
  const panes = Array.isArray(session.panes) ? session.panes : [];
  return panes.map((pane, index) => {
    const status = String(pane.status || "unknown").toLowerCase();
    return {
      name: pane.name || `pane-${index}`,
      status,
      lastActivity: pane.last_activity || pane.lastActivity || "unknown",
      currentCommand: pane.current_command || pane.currentCommand || "",
    };
  });
}

function normalizeCi(session) {
  const source = Array.isArray(session.session_ci)
    ? session.session_ci
    : Array.isArray(session.ci)
      ? session.ci
      : [];

  return source
    .map((entry) => {
      const payload = parseCiPayload(entry.data_json || entry.data || entry.payload);
      return {
        provider: entry.provider || "unknown",
        status: String(entry.status || "unknown").toLowerCase(),
        payload,
        toolMessage: entry.tool_message || entry.message || null,
      };
    })
    .filter((entry) => entry.provider !== "unknown");
}

function extractPrs(session, ciEntries) {
  if (Array.isArray(session.prs)) {
    return session.prs.map((pr) => ({
      num: pr.num ?? pr.number ?? pr.id ?? "?",
      title: pr.title || "Untitled PR",
      url: pr.url || pr.web_url || null,
    }));
  }

  const github = ciEntries.find((entry) => entry.provider === "github");
  if (github?.payload && Array.isArray(github.payload.prs)) {
    return github.payload.prs.map((pr) => ({
      num: pr.num ?? pr.number ?? pr.id ?? "?",
      title: pr.title || "Untitled PR",
      url: pr.url || pr.web_url || null,
    }));
  }

  return [];
}

function extractRuns(ciEntries) {
  const rows = [];

  ciEntries.forEach((entry) => {
    if (!entry.payload || !Array.isArray(entry.payload.runs)) {
      return;
    }

    entry.payload.runs.forEach((run, index) => {
      rows.push({
        provider: entry.provider,
        title:
          run.displayTitle ||
          run.name ||
          run.pipeline?.name ||
          run.definition?.name ||
          `run-${index + 1}`,
        status: String(
          run.status || run.state || run.result || run.conclusion || "unknown",
        ).toLowerCase(),
        conclusion: run.conclusion || run.result || null,
        branch: run.headBranch || run.sourceBranch || run.branch || null,
        createdAt: run.createdAt || run.creationDate || run.queueTime || run.finishTime || null,
        url: run.url || run.webUrl || run._links?.web?.href || null,
      });
    });
  });

  return rows;
}

function normalizeAtm(session) {
  if (!session || typeof session.atm !== "object" || session.atm === null) {
    return null;
  }
  const state = String(session.atm.state || "unknown").toLowerCase();
  return {
    state: ["active", "idle", "stuck", "offline", "unknown"].includes(state)
      ? state
      : "unknown",
    lastTransition: session.atm.last_transition || session.atm.lastTransition || null,
  };
}

function normalizeSessions(sessionRows, hostRows) {
  const hostMap = new Map((Array.isArray(hostRows) ? hostRows : []).map((host) => [host.id, host]));

  return (Array.isArray(sessionRows) ? sessionRows : []).map((row) => {
    const ciEntries = normalizeCi(row);
    const prs = extractPrs(row, ciEntries);
    const ciRuns = extractRuns(ciEntries);
    const status = String(row.status || "stopped").toLowerCase();

    return {
      ...row,
      status,
      project: row.project || "unassigned",
      panes: normalizePanes(row),
      atm: normalizeAtm(row),
      ciEntries,
      ciRuns,
      prs,
      openPrCount: prs.length,
      host: hostMap.get(row.host_id) || null,
    };
  });
}

function normalizeDiscovery(rows) {
  return (Array.isArray(rows) ? rows : []).map((row) => ({
    name: row.name || "unknown",
    panes: normalizePanes(row),
  }));
}

function ciRunTone(run) {
  const status = String(run?.status || "unknown").toLowerCase();
  const conclusion = String(run?.conclusion || "").toLowerCase();
  const value = `${status} ${conclusion}`;

  if (value.includes("in_progress") || value.includes("queued") || value.includes("running")) {
    return { color: "#f59e0b", text: "running" };
  }
  if (value.includes("success") || value.includes("pass") || value.includes("succeeded") || value.includes("completed")) {
    return { color: "#10b981", text: "pass" };
  }
  if (value.includes("fail") || value.includes("error") || value.includes("cancel")) {
    return { color: "#ef4444", text: "fail" };
  }
  return { color: "#64748b", text: "unknown" };
}

function buildJumpCommand(session, host) {
  if (!host || host.is_local) {
    return `tmux attach -t ${session.name}`;
  }
  const sshUser = host.ssh_user || "<ssh_user>";
  return `ssh ${sshUser}@${host.address} tmux attach -t ${session.name}`;
}

function sessionStyle(session) {
  const baseOpacity = session.status === "stopped" ? 0.55 : 1;
  if (!session.host || session.host.reachable) {
    return { opacity: baseOpacity };
  }
  return {
    opacity: baseOpacity * 0.75,
    filter: "grayscale(1)",
  };
}

function SessionActionButtons({ session, busy, onStartStop, onEdit }) {
  const canStart = session.status === "stopped";
  const actionLabel = canStart ? "Start" : "Stop";

  return (
    <div style={{ display: "flex", gap: 6 }}>
      <button
        onClick={(event) => {
          event.stopPropagation();
          onStartStop(session);
        }}
        disabled={busy}
        style={{
          border: "1px solid #1e2535",
          borderRadius: 4,
          fontSize: 10,
          padding: "2px 8px",
          background: canStart ? "#102b1f" : "#2b1212",
          color: canStart ? "#34d399" : "#fca5a5",
          cursor: busy ? "default" : "pointer",
        }}
      >
        {busy ? "..." : actionLabel}
      </button>
      <button
        onClick={(event) => {
          event.stopPropagation();
          onEdit(session);
        }}
        style={{
          border: "1px solid #1e2535",
          borderRadius: 4,
          fontSize: 10,
          padding: "2px 8px",
          background: "#0f172a",
          color: "#93c5fd",
          cursor: "pointer",
        }}
      >
        Edit
      </button>
    </div>
  );
}

function CiSummary({ session }) {
  if (!session.ciEntries.length) {
    return null;
  }

  const runs = session.ciRuns.slice(0, 4);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
      <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
        {session.ciEntries.map((entry, index) => (
          <span
            key={`${entry.provider}-${index}`}
            title={entry.toolMessage || undefined}
            style={{
              fontSize: 9,
              color: entry.provider === "github" ? "#60a5fa" : "#38bdf8",
              background: entry.provider === "github" ? "#172554" : "#082f49",
              borderRadius: 3,
              padding: "1px 6px",
              textTransform: "uppercase",
            }}
          >
            {entry.provider}
          </span>
        ))}
      </div>
      <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
        {runs.map((run, index) => {
          const tone = ciRunTone(run);
          return (
            <span
              key={`${run.provider}-${run.title}-${index}`}
              title={run.title}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                fontSize: 9,
                color: "#94a3b8",
                border: "1px solid #1e2535",
                borderRadius: 3,
                padding: "1px 5px",
              }}
            >
              <span
                style={{
                  display: "inline-block",
                  width: 6,
                  height: 6,
                  borderRadius: "50%",
                  background: tone.color,
                }}
              />
              {tone.text}
            </span>
          );
        })}
      </div>
    </div>
  );
}

function GridCard({ session, busy, onJump, onStartStop, onEdit }) {
  const pc = PROJECT_COLORS[session.project] || "#6b7280";
  const activePanes = session.panes.filter((pane) => pane.status === "active").length;

  return (
    <div
      onClick={() => onJump(session)}
      style={{
        background: "#0d1117",
        border: "1px solid #131820",
        borderRadius: 8,
        overflow: "hidden",
        cursor: "pointer",
        transition: "border-color 0.15s, transform 0.1s",
        ...sessionStyle(session),
      }}
      onMouseEnter={(event) => {
        event.currentTarget.style.borderColor = `${pc}55`;
        event.currentTarget.style.transform = "translateY(-1px)";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.borderColor = "#131820";
        event.currentTarget.style.transform = "translateY(0)";
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "8px 12px",
          borderBottom: "1px solid #0a0e14",
        }}
      >
        <div style={{ width: 3, height: 26, borderRadius: 2, background: pc, flexShrink: 0 }} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              fontSize: 12,
              fontWeight: 600,
              color: "#e2e8f0",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {session.name}
          </div>
          <div style={{ fontSize: 10, color: pc, opacity: 0.8 }}>
            {(session.project || "unassigned") + " | " + hostLabel(session.host)}
          </div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 3 }}>
          <Dot status={session.status} size={7} />
          <span style={{ fontSize: 9, color: "#334155" }}>
            {activePanes}/{session.panes.length}
          </span>
        </div>
      </div>

      <div style={{ padding: "6px 12px 10px", display: "flex", flexDirection: "column", gap: 8 }}>
        {session.panes.slice(0, 4).map((pane, index) => (
          <div
            key={`${pane.name}-${index}`}
            style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}
          >
            <Dot status={pane.status} size={5} />
            <span style={{ fontSize: 10, color: "#94a3b8", flex: 1 }}>{pane.name}</span>
            <span style={{ fontSize: 9, color: "#64748b", textTransform: "uppercase" }}>
              {pane.status}
            </span>
          </div>
        ))}

        <CiSummary session={session} />

        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 9, color: "#475569" }}>{hostBadge(session.host)}</span>
          <SessionActionButtons
            session={session}
            busy={busy}
            onStartStop={onStartStop}
            onEdit={onEdit}
          />
        </div>
      </div>
    </div>
  );
}

function ListView({ sessions, busyBySession, onJump, onStartStop, onEdit }) {
  return (
    <div style={{ padding: "0 24px 24px", overflowX: "auto" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12, minWidth: 960 }}>
        <thead>
          <tr style={{ borderBottom: "1px solid #131820" }}>
            {["", "Session", "Project", "Host", "Status", "Pane States", "Open PRs", "Actions"].map(
              (header) => (
                <th
                  key={header}
                  style={{
                    padding: "8px 12px",
                    textAlign: "left",
                    fontSize: 10,
                    color: "#1e2535",
                    letterSpacing: "0.1em",
                    fontWeight: 600,
                  }}
                >
                  {header}
                </th>
              ),
            )}
          </tr>
        </thead>
        <tbody>
          {sessions.map((session, index) => {
            const pc = PROJECT_COLORS[session.project] || "#6b7280";
            return (
              <tr
                key={`${session.name}-${index}`}
                onClick={() => onJump(session)}
                style={{
                  borderBottom: "1px solid #0a0e14",
                  cursor: "pointer",
                  transition: "background 0.1s",
                  ...sessionStyle(session),
                }}
                onMouseEnter={(event) => {
                  event.currentTarget.style.background = "#0f1117";
                }}
                onMouseLeave={(event) => {
                  event.currentTarget.style.background = "transparent";
                }}
              >
                <td style={{ padding: "7px 12px" }}>
                  <div style={{ width: 3, height: 18, borderRadius: 2, background: pc }} />
                </td>
                <td style={{ padding: "7px 12px", color: "#cbd5e1", fontWeight: 500 }}>{session.name}</td>
                <td style={{ padding: "7px 12px", color: pc, fontSize: 11 }}>{session.project || "unassigned"}</td>
                <td style={{ padding: "7px 12px", color: "#94a3b8", fontSize: 11 }}>{hostLabel(session.host)}</td>
                <td style={{ padding: "7px 12px" }}>
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                    <Dot status={session.status} size={6} />
                    <span style={{ color: "#64748b", fontSize: 11 }}>{session.status}</span>
                  </span>
                </td>
                <td style={{ padding: "7px 12px", color: "#94a3b8", fontSize: 10 }}>
                  <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                    {session.panes.slice(0, 4).map((pane, paneIndex) => (
                      <span
                        key={`${pane.name}-${paneIndex}`}
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 4,
                          border: "1px solid #1e2535",
                          borderRadius: 3,
                          padding: "1px 5px",
                        }}
                      >
                        <Dot status={pane.status} size={5} />
                        {pane.name}:{pane.status}
                      </span>
                    ))}
                  </div>
                </td>
                <td style={{ padding: "7px 12px", color: "#60a5fa" }}>{session.openPrCount || "-"}</td>
                <td style={{ padding: "7px 12px" }}>
                  <SessionActionButtons
                    session={session}
                    busy={Boolean(busyBySession[session.name])}
                    onStartStop={onStartStop}
                    onEdit={onEdit}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function GroupedView({ sessions, busyBySession, onJump, onStartStop, onEdit }) {
  const byProject = useMemo(() => {
    const grouped = new Map();
    sessions.forEach((session) => {
      const project = session.project || "unassigned";
      if (!grouped.has(project)) {
        grouped.set(project, []);
      }
      grouped.get(project).push(session);
    });
    return grouped;
  }, [sessions]);

  return (
    <div style={{ padding: "0 24px 24px", display: "flex", flexDirection: "column", gap: 28 }}>
      {Array.from(byProject.entries()).map(([project, projectSessions]) => {
        const pc = PROJECT_COLORS[project] || "#6b7280";
        const running = projectSessions.filter((session) => session.status === "running").length;
        const totalPrs = projectSessions.reduce((sum, session) => sum + session.openPrCount, 0);

        const byHost = new Map();
        projectSessions.forEach((session) => {
          const key = session.host?.id || `unknown-${session.host_id}`;
          if (!byHost.has(key)) {
            byHost.set(key, { host: session.host, sessions: [] });
          }
          byHost.get(key).sessions.push(session);
        });

        return (
          <div key={project}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                marginBottom: 12,
                paddingBottom: 8,
                borderBottom: `1px solid ${pc}25`,
              }}
            >
              <div style={{ width: 4, height: 16, borderRadius: 2, background: pc }} />
              <span style={{ fontSize: 12, fontWeight: 700, color: pc, letterSpacing: "0.06em" }}>{project}</span>
              <span style={{ fontSize: 10, color: "#334155" }}>
                {running}/{projectSessions.length} running
              </span>
              {totalPrs > 0 && (
                <span
                  style={{
                    fontSize: 10,
                    color: pc,
                    background: `${pc}18`,
                    borderRadius: 3,
                    padding: "1px 7px",
                  }}
                >
                  {totalPrs} PR{totalPrs !== 1 ? "s" : ""}
                </span>
              )}
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              {Array.from(byHost.values()).map(({ host, sessions: hostSessions }) => (
                <div key={host?.id || `unknown-${hostSessions[0]?.host_id || "na"}`}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                    <span style={{ fontSize: 10, color: "#64748b", letterSpacing: "0.08em" }}>
                      HOST {hostLabel(host).toUpperCase()}
                    </span>
                    <span style={{ fontSize: 10, color: host?.reachable ? "#10b981" : "#94a3b8" }}>
                      {host?.reachable ? "reachable" : `last seen ${relativeTime(host?.last_seen)}`}
                    </span>
                  </div>
                  <div
                    style={{
                      display: "grid",
                      gridTemplateColumns: "repeat(auto-fill, minmax(250px, 1fr))",
                      gap: 10,
                    }}
                  >
                    {hostSessions.map((session, index) => (
                      <GridCard
                        key={`${session.name}-${index}`}
                        session={session}
                        busy={Boolean(busyBySession[session.name])}
                        onJump={onJump}
                        onStartStop={onStartStop}
                        onEdit={onEdit}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function DiscoveryView({ rows }) {
  return (
    <div style={{ padding: "16px 24px 24px" }}>
      <div style={{ fontSize: 11, color: "#64748b", marginBottom: 12 }}>
        Raw tmux discovery (informational only; no definition writes)
      </div>
      <div style={{ overflowX: "auto" }}>
        <table style={{ width: "100%", borderCollapse: "collapse", minWidth: 800 }}>
          <thead>
            <tr style={{ borderBottom: "1px solid #131820" }}>
              {["Session", "Pane", "State", "Command", "Last Activity"].map((header) => (
                <th
                  key={header}
                  style={{
                    textAlign: "left",
                    fontSize: 10,
                    color: "#334155",
                    letterSpacing: "0.1em",
                    padding: "7px 10px",
                  }}
                >
                  {header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td colSpan={5} style={{ padding: "16px 10px", color: "#64748b", fontSize: 11 }}>
                  No discovered tmux sessions.
                </td>
              </tr>
            )}
            {rows.map((row, rowIndex) =>
              row.panes.length ? (
                row.panes.map((pane, paneIndex) => (
                  <tr key={`${row.name}-${pane.name}-${paneIndex}`} style={{ borderBottom: "1px solid #0a0e14" }}>
                    <td style={{ padding: "7px 10px", color: "#cbd5e1", fontSize: 11 }}>
                      {paneIndex === 0 ? row.name : ""}
                    </td>
                    <td style={{ padding: "7px 10px", color: "#94a3b8", fontSize: 11 }}>{pane.name}</td>
                    <td style={{ padding: "7px 10px", color: "#94a3b8", fontSize: 11 }}>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                        <Dot status={pane.status} size={6} />
                        {pane.status}
                      </span>
                    </td>
                    <td style={{ padding: "7px 10px", color: "#475569", fontSize: 11 }}>{pane.currentCommand || "-"}</td>
                    <td style={{ padding: "7px 10px", color: "#475569", fontSize: 11 }}>{pane.lastActivity}</td>
                  </tr>
                ))
              ) : (
                <tr key={`${row.name}-${rowIndex}`} style={{ borderBottom: "1px solid #0a0e14" }}>
                  <td style={{ padding: "7px 10px", color: "#cbd5e1", fontSize: 11 }}>{row.name}</td>
                  <td style={{ padding: "7px 10px", color: "#64748b", fontSize: 11 }} colSpan={4}>
                    no panes reported
                  </td>
                </tr>
              ),
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function makeCrewUlid() {
  return `01J${Date.now().toString(36).toUpperCase()}${Math.random().toString(36).slice(2, 12).toUpperCase()}`;
}

function OrganizationEditorModal({ baseUrl, open, onClose, onSaved }) {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [errorMessage, setErrorMessage] = useState(null);
  const [successMessage, setSuccessMessage] = useState(null);
  const [state, setState] = useState({
    armadas: [],
    fleets: [],
    flotillas: [],
    crews: [],
    crew_refs: [],
  });

  const [armadaName, setArmadaName] = useState("");
  const [fleetName, setFleetName] = useState("");
  const [fleetColor, setFleetColor] = useState("#3b82f6");
  const [newCrewName, setNewCrewName] = useState("");
  const [newCrewUlid, setNewCrewUlid] = useState(makeCrewUlid());
  const [captainModel, setCaptainModel] = useState("claude-opus");
  const [mateModel, setMateModel] = useState("codex-high");
  const [newRootPath, setNewRootPath] = useState("/tmp/scmux");
  const [selectedArmadaId, setSelectedArmadaId] = useState(null);
  const [selectedFleetId, setSelectedFleetId] = useState(null);
  const [selectedFlotillaId, setSelectedFlotillaId] = useState(null);

  const loadState = async () => {
    setLoading(true);
    setErrorMessage(null);
    try {
      const response = await fetch(`${baseUrl}/editor/state`);
      const body = await response.json();
      if (!response.ok) {
        throw new Error(body.message || `HTTP ${response.status}`);
      }
      setState({
        armadas: Array.isArray(body.armadas) ? body.armadas : [],
        fleets: Array.isArray(body.fleets) ? body.fleets : [],
        flotillas: Array.isArray(body.flotillas) ? body.flotillas : [],
        crews: Array.isArray(body.crews) ? body.crews : [],
        crew_refs: Array.isArray(body.crew_refs) ? body.crew_refs : [],
      });
    } catch (error) {
      setErrorMessage(`Failed to load editor state: ${String(error)}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    loadState();
  }, [open]);

  useEffect(() => {
    if (!selectedArmadaId && state.armadas.length) {
      setSelectedArmadaId(state.armadas[0].id);
    }
  }, [state.armadas, selectedArmadaId]);

  useEffect(() => {
    const fleetsForArmada = state.fleets.filter((fleet) => fleet.armada_id === selectedArmadaId);
    if (!fleetsForArmada.length) {
      setSelectedFleetId(null);
      return;
    }
    if (!fleetsForArmada.some((fleet) => fleet.id === selectedFleetId)) {
      setSelectedFleetId(fleetsForArmada[0].id);
    }
  }, [state.fleets, selectedArmadaId, selectedFleetId]);

  useEffect(() => {
    const flotillasForFleet = state.flotillas.filter((item) => item.fleet_id === selectedFleetId);
    if (!flotillasForFleet.length) {
      setSelectedFlotillaId(null);
      return;
    }
    if (!flotillasForFleet.some((item) => item.id === selectedFlotillaId)) {
      setSelectedFlotillaId(flotillasForFleet[0].id);
    }
  }, [state.flotillas, selectedFleetId, selectedFlotillaId]);

  if (!open) {
    return null;
  }

  const fleetsForSelectedArmada = state.fleets.filter((fleet) => fleet.armada_id === selectedArmadaId);
  const flotillasForSelectedFleet = state.flotillas.filter((item) => item.fleet_id === selectedFleetId);
  const hasPlacement = Number.isFinite(selectedArmadaId) && Number.isFinite(selectedFleetId);

  const submitJson = async (url, method, payload, successText) => {
    setSaving(true);
    setErrorMessage(null);
    setSuccessMessage(null);
    try {
      const response = await fetch(url, {
        method,
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
      });
      const body = await response.json();
      if (!response.ok) {
        throw new Error(body.message || body.code || `HTTP ${response.status}`);
      }
      setSuccessMessage(successText || body.message || "Saved");
      await loadState();
      if (onSaved) {
        onSaved(successText || body.message || "Saved");
      }
      return true;
    } catch (error) {
      setErrorMessage(String(error));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const createArmada = async () => {
    if (!armadaName.trim()) {
      setErrorMessage("Armada name is required.");
      return;
    }
    const ok = await submitJson(
      `${baseUrl}/editor/armadas`,
      "POST",
      { name: armadaName.trim() },
      `Armada created: ${armadaName.trim()}`,
    );
    if (ok) {
      setArmadaName("");
    }
  };

  const createFleet = async () => {
    if (!fleetName.trim()) {
      setErrorMessage("Fleet name is required.");
      return;
    }
    if (!selectedArmadaId) {
      setErrorMessage("Create/select an Armada before creating a Fleet.");
      return;
    }
    const ok = await submitJson(
      `${baseUrl}/editor/fleets`,
      "POST",
      { armada_id: selectedArmadaId, name: fleetName.trim(), color: fleetColor },
      `Fleet created: ${fleetName.trim()}`,
    );
    if (ok) {
      setFleetName("");
    }
  };

  const createCrew = async () => {
    if (!newCrewName.trim()) {
      setErrorMessage("Crew name is required.");
      return;
    }
    if (!hasPlacement) {
      setErrorMessage("Crew placement is unresolved. Select Armada and Fleet.");
      return;
    }
    const payload = {
      crew_name: newCrewName.trim(),
      crew_ulid: newCrewUlid.trim() || makeCrewUlid(),
      members: [
        {
          member_id: "team-lead",
          role: "captain",
          ai_provider: "claude",
          model: captainModel.trim() || "claude-opus",
          startup_prompts: ["prompts/arch-startup.md", "prompts/pm-startup.md"],
        },
        {
          member_id: "arch-cmux",
          role: "mate",
          ai_provider: "codex",
          model: mateModel.trim() || "codex-high",
          startup_prompts: ["prompts/arch-cmux-startup.md"],
        },
      ],
      variants: [
        {
          host_id: 1,
          root_path: newRootPath.trim() || "/tmp/scmux",
        },
      ],
      placement: {
        armada_id: selectedArmadaId,
        fleet_id: selectedFleetId,
        flotilla_id: FLOTILLA_V1_ENABLED ? selectedFlotillaId : null,
      },
    };

    const ok = await submitJson(`${baseUrl}/editor/crews`, "POST", payload, `Crew created: ${newCrewName.trim()}`);
    if (ok) {
      setNewCrewName("");
      setNewCrewUlid(makeCrewUlid());
    }
  };

  const moveRef = async (refRow, nextArmadaId, nextFleetId) => {
    if (!nextArmadaId || !nextFleetId) {
      setErrorMessage("Move requires target armada and fleet.");
      return;
    }
    await submitJson(`${baseUrl}/editor/crew-refs/${refRow.id}/move`, "POST", {
      armada_id: Number(nextArmadaId),
      fleet_id: Number(nextFleetId),
      flotilla_id: null,
    }, `Moved crew ref ${refRow.id}`);
  };

  const cloneCrew = async (crewId, crewName) => {
    const copyName = `${crewName}-clone`;
    await submitJson(`${baseUrl}/editor/crews/${crewId}/clone`, "POST", {
      crew_name: copyName,
      crew_ulid: makeCrewUlid(),
      placement: {
        armada_id: selectedArmadaId,
        fleet_id: selectedFleetId,
        flotilla_id: FLOTILLA_V1_ENABLED ? selectedFlotillaId : null,
      },
    }, `Cloned crew to ${copyName}`);
  };

  const unlinkRef = async (refId) => {
    setSaving(true);
    setErrorMessage(null);
    setSuccessMessage(null);
    try {
      const response = await fetch(`${baseUrl}/editor/crew-refs/${refId}`, { method: "DELETE" });
      const body = await response.json();
      if (!response.ok) {
        throw new Error(body.message || `HTTP ${response.status}`);
      }
      setSuccessMessage(body.message || `Unlinked ref ${refId}`);
      await loadState();
      if (onSaved) {
        onSaved(body.message || `Unlinked ref ${refId}`);
      }
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.8)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 230,
      }}
      onClick={onClose}
    >
      <div
        style={{
          width: "min(1100px, 96vw)",
          maxHeight: "92vh",
          overflowY: "auto",
          background: "#0d1117",
          border: "1px solid #1e2535",
          borderRadius: 10,
          padding: 16,
          display: "grid",
          gap: 14,
        }}
        onClick={(event) => event.stopPropagation()}
      >
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <div>
            <div style={{ fontSize: 10, color: "#475569", letterSpacing: "0.1em" }}>ORGANIZATION EDITOR</div>
            <div style={{ fontSize: 13, color: "#cbd5e1" }}>
              Armada / Fleet{FLOTILLA_V1_ENABLED ? " / Flotilla" : ""} / Crew
            </div>
          </div>
          <button
            onClick={onClose}
            style={{
              border: "1px solid #1e2535",
              borderRadius: 5,
              background: "transparent",
              color: "#94a3b8",
              padding: "6px 10px",
              cursor: "pointer",
            }}
          >
            Close
          </button>
        </div>

        {!FLOTILLA_V1_ENABLED && (
          <div style={{ fontSize: 11, color: "#f59e0b" }}>
            Flotilla is currently behind feature flag. UI operates at Armada/Fleet scope.
          </div>
        )}

        {loading ? (
          <div style={{ fontSize: 12, color: "#64748b" }}>Loading editor state...</div>
        ) : (
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10 }}>
            <div style={{ border: "1px solid #1e2535", borderRadius: 8, padding: 10, display: "grid", gap: 8 }}>
              <div style={{ fontSize: 11, color: "#93c5fd" }}>Armada Editor</div>
              <input
                value={armadaName}
                onChange={(event) => setArmadaName(event.target.value)}
                placeholder="Armada name"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <button
                onClick={createArmada}
                disabled={saving}
                style={{ border: "none", background: "#2563eb", color: "#fff", borderRadius: 5, padding: "6px 8px", cursor: "pointer", fontSize: 11 }}
              >
                + Create Armada
              </button>
              <div style={{ maxHeight: 130, overflowY: "auto", borderTop: "1px solid #131820", paddingTop: 6 }}>
                {state.armadas.map((armada) => (
                  <div key={armada.id} style={{ fontSize: 11, color: "#94a3b8", padding: "2px 0" }}>
                    {armada.name} <span style={{ color: "#475569" }}>#{armada.id}</span>
                  </div>
                ))}
              </div>
            </div>

            <div style={{ border: "1px solid #1e2535", borderRadius: 8, padding: 10, display: "grid", gap: 8 }}>
              <div style={{ fontSize: 11, color: "#93c5fd" }}>Fleet Editor</div>
              <select
                value={selectedArmadaId || ""}
                onChange={(event) => setSelectedArmadaId(Number(event.target.value))}
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              >
                <option value="">Select armada</option>
                {state.armadas.map((armada) => (
                  <option key={armada.id} value={armada.id}>{armada.name}</option>
                ))}
              </select>
              <input
                value={fleetName}
                onChange={(event) => setFleetName(event.target.value)}
                placeholder="Fleet name"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <input
                value={fleetColor}
                onChange={(event) => setFleetColor(event.target.value)}
                placeholder="#3b82f6"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <button
                onClick={createFleet}
                disabled={saving || !selectedArmadaId}
                style={{ border: "none", background: "#0ea5e9", color: "#fff", borderRadius: 5, padding: "6px 8px", cursor: "pointer", fontSize: 11 }}
              >
                + Create Fleet
              </button>
              <div style={{ maxHeight: 130, overflowY: "auto", borderTop: "1px solid #131820", paddingTop: 6 }}>
                {fleetsForSelectedArmada.map((fleet) => (
                  <div key={fleet.id} style={{ fontSize: 11, color: "#94a3b8", padding: "2px 0" }}>
                    {fleet.name} <span style={{ color: fleet.color || "#64748b" }}>{fleet.color || ""}</span>
                  </div>
                ))}
              </div>
            </div>

            <div style={{ border: "1px solid #1e2535", borderRadius: 8, padding: 10, display: "grid", gap: 8 }}>
              <div style={{ fontSize: 11, color: "#93c5fd" }}>Crew Editor</div>
              <input
                value={newCrewName}
                onChange={(event) => setNewCrewName(event.target.value)}
                placeholder="Crew name"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <input
                value={newCrewUlid}
                onChange={(event) => setNewCrewUlid(event.target.value)}
                placeholder="Crew ULID"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <input
                value={newRootPath}
                onChange={(event) => setNewRootPath(event.target.value)}
                placeholder="root path"
                style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
              />
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
                <input
                  value={captainModel}
                  onChange={(event) => setCaptainModel(event.target.value)}
                  placeholder="Captain model"
                  style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
                />
                <input
                  value={mateModel}
                  onChange={(event) => setMateModel(event.target.value)}
                  placeholder="Mate model"
                  style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
                />
              </div>
              {FLOTILLA_V1_ENABLED && (
                <select
                  value={selectedFlotillaId || ""}
                  onChange={(event) => setSelectedFlotillaId(Number(event.target.value))}
                  style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "7px 9px" }}
                >
                  <option value="">No flotilla</option>
                  {flotillasForSelectedFleet.map((item) => (
                    <option key={item.id} value={item.id}>{item.name}</option>
                  ))}
                </select>
              )}
              {!hasPlacement && (
                <div style={{ fontSize: 11, color: "#f59e0b" }}>
                  Placement unresolved: create/select Armada and Fleet first.
                </div>
              )}
              <button
                onClick={createCrew}
                disabled={saving || !hasPlacement}
                style={{ border: "none", background: "#16a34a", color: "#fff", borderRadius: 5, padding: "6px 8px", cursor: "pointer", fontSize: 11 }}
              >
                + Create Crew
              </button>
            </div>
          </div>
        )}

        {errorMessage && <div style={{ color: "#f87171", fontSize: 11 }}>{errorMessage}</div>}
        {successMessage && <div style={{ color: "#34d399", fontSize: 11 }}>{successMessage}</div>}

        <div style={{ borderTop: "1px solid #131820", paddingTop: 10 }}>
          <div style={{ fontSize: 11, color: "#93c5fd", marginBottom: 8 }}>Crew Placement Controls</div>
          <div style={{ display: "grid", gap: 8 }}>
            {state.crew_refs.map((refRow) => {
              const crew = state.crews.find((item) => item.id === refRow.crew_id);
              const armada = state.armadas.find((item) => item.id === refRow.armada_id);
              const fleet = state.fleets.find((item) => item.id === refRow.fleet_id);
              return (
                <div key={refRow.id} style={{ border: "1px solid #1e2535", borderRadius: 6, padding: 8, display: "grid", gap: 6 }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
                    <div style={{ fontSize: 11, color: "#cbd5e1" }}>
                      {crew?.crew_name || `crew-${refRow.crew_id}`} <span style={{ color: "#64748b" }}>ref#{refRow.id}</span>
                    </div>
                    <div style={{ fontSize: 10, color: "#64748b" }}>
                      {armada?.name || "unknown"} / {fleet?.name || "unknown"}
                    </div>
                  </div>
                  <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                    <select id={`move-armada-${refRow.id}`} defaultValue={refRow.armada_id} style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "5px 8px", fontSize: 11 }}>
                      {state.armadas.map((item) => (
                        <option key={item.id} value={item.id}>{item.name}</option>
                      ))}
                    </select>
                    <select id={`move-fleet-${refRow.id}`} defaultValue={refRow.fleet_id} style={{ background: "#060810", border: "1px solid #1e2535", borderRadius: 5, color: "#cbd5e1", padding: "5px 8px", fontSize: 11 }}>
                      {state.fleets.map((item) => (
                        <option key={item.id} value={item.id}>{item.name}</option>
                      ))}
                    </select>
                    <button
                      onClick={() => {
                        const armadaValue = Number(document.getElementById(`move-armada-${refRow.id}`)?.value);
                        const fleetValue = Number(document.getElementById(`move-fleet-${refRow.id}`)?.value);
                        moveRef(refRow, armadaValue, fleetValue);
                      }}
                      style={{ border: "1px solid #1e2535", background: "#1e3a8a", color: "#bfdbfe", borderRadius: 5, padding: "5px 8px", cursor: "pointer", fontSize: 11 }}
                    >
                      Move
                    </button>
                    <button
                      onClick={() => cloneCrew(refRow.crew_id, crew?.crew_name || `crew-${refRow.crew_id}`)}
                      style={{ border: "1px solid #1e2535", background: "#312e81", color: "#c7d2fe", borderRadius: 5, padding: "5px 8px", cursor: "pointer", fontSize: 11 }}
                    >
                      Clone Crew
                    </button>
                    <button
                      onClick={() => unlinkRef(refRow.id)}
                      style={{ border: "1px solid #1e2535", background: "#3f1d1d", color: "#fca5a5", borderRadius: 5, padding: "5px 8px", cursor: "pointer", fontSize: 11 }}
                    >
                      Unlink
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

function JumpModal({ baseUrl, defaultTerminal, session, onClose }) {
  const [submitting, setSubmitting] = useState(false);
  const [feedback, setFeedback] = useState(null);

  useEffect(() => {
    if (!session) {
      return undefined;
    }
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [session, onClose]);

  useEffect(() => {
    setFeedback(null);
    setSubmitting(false);
  }, [session]);

  if (!session) {
    return null;
  }

  const pc = PROJECT_COLORS[session.project] || "#3b82f6";
  const cmd = buildJumpCommand(session, session.host);

  const handleJump = async () => {
    setSubmitting(true);
    try {
      const response = await fetch(`${baseUrl}/sessions/${encodeURIComponent(session.name)}/jump`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          terminal: defaultTerminal,
          host_id: session.host_id,
        }),
      });
      const body = await response.json();
      if (!response.ok) {
        setFeedback({ ok: false, message: body.message || `HTTP ${response.status}` });
      } else {
        setFeedback({ ok: body.ok, message: body.message || "No message" });
      }
    } catch (error) {
      setFeedback({ ok: false, message: String(error) });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.8)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 200,
        backdropFilter: "blur(6px)",
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: "#0d1117",
          border: `1px solid ${pc}50`,
          borderRadius: 12,
          padding: 24,
          minWidth: 360,
          maxWidth: 680,
          width: "92vw",
          fontFamily: "inherit",
        }}
        onClick={(event) => event.stopPropagation()}
      >
        <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.12em", marginBottom: 6 }}>
          JUMP TO SESSION
        </div>
        <div style={{ fontSize: 20, color: "#f1f5f9", fontWeight: 700, marginBottom: 3 }}>{session.name}</div>
        <div style={{ fontSize: 11, color: pc, marginBottom: 6 }}>
          {session.project || "unassigned"} on {hostLabel(session.host)}
        </div>

        <div
          style={{
            background: "#060810",
            borderRadius: 6,
            padding: "10px 14px",
            marginBottom: 18,
            fontSize: 11,
            color: "#94a3b8",
            overflowX: "auto",
            whiteSpace: "nowrap",
          }}
        >
          <span style={{ color: "#334155" }}>$ </span>
          {cmd}
        </div>

        <div style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.1em", marginBottom: 8 }}>PANES</div>
          {session.panes.length === 0 && <div style={{ fontSize: 11, color: "#475569" }}>No panes reported.</div>}
          {session.panes.map((pane, index) => (
            <div
              key={`${pane.name}-${index}`}
              style={{
                display: "grid",
                gridTemplateColumns: "auto 1fr auto",
                alignItems: "center",
                gap: 8,
                padding: "4px 0",
                borderBottom: index < session.panes.length - 1 ? "1px solid #0f172a" : "none",
              }}
            >
              <Dot status={pane.status} size={6} />
              <span style={{ fontSize: 12, color: "#94a3b8", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {pane.name} ({pane.currentCommand || "-"})
              </span>
              <span style={{ fontSize: 10, color: "#64748b", textTransform: "uppercase" }}>{pane.status}</span>
            </div>
          ))}
        </div>

        {feedback && (
          <div style={{ fontSize: 11, marginBottom: 14, color: feedback.ok ? "#34d399" : "#f87171" }}>
            {feedback.message}
          </div>
        )}

        <div style={{ display: "flex", gap: 8 }}>
          <button
            onClick={handleJump}
            disabled={submitting}
            style={{
              flex: 1,
              padding: "10px 0",
              background: pc,
              border: "none",
              borderRadius: 6,
              color: "#fff",
              fontSize: 12,
              fontWeight: 700,
              cursor: submitting ? "default" : "pointer",
              fontFamily: "inherit",
              letterSpacing: "0.05em",
              opacity: submitting ? 0.7 : 1,
            }}
          >
            {submitting ? "Launching..." : "Open in iTerm2 ->"}
          </button>
          <button
            style={{
              padding: "10px 16px",
              background: "transparent",
              border: "1px solid #1e2535",
              borderRadius: 6,
              color: "#475569",
              fontSize: 12,
              cursor: "pointer",
              fontFamily: "inherit",
            }}
            onClick={onClose}
          >
            esc
          </button>
        </div>
      </div>
    </div>
  );
}

function defaultConfigFor(name) {
  return {
    session_name: name || "new-session",
    panes: [
      {
        name: "agent",
        command: "sleep 1",
        atm_agent: "agent",
        atm_team: "scmux-dev",
      },
    ],
  };
}

function ProjectEditorModal({ baseUrl, defaultHostId, target, onClose, onSaved }) {
  const isEdit = target?.mode === "edit";
  const [loading, setLoading] = useState(Boolean(isEdit));
  const [saving, setSaving] = useState(false);
  const [errorMessage, setErrorMessage] = useState(null);
  const [name, setName] = useState(target?.session?.name || "");
  const [project, setProject] = useState(target?.session?.project || "");
  const [autoStart, setAutoStart] = useState(Boolean(target?.session?.auto_start));
  const [cronSchedule, setCronSchedule] = useState(target?.session?.cron_schedule || "");
  const [githubRepo, setGithubRepo] = useState(target?.session?.github_repo || "");
  const [azureProject, setAzureProject] = useState(target?.session?.azure_project || "");
  const [configText, setConfigText] = useState(
    JSON.stringify(defaultConfigFor(target?.session?.name || "new-session"), null, 2),
  );

  useEffect(() => {
    let cancelled = false;

    async function loadEditDetail() {
      if (!isEdit || !target?.session?.name) {
        setLoading(false);
        return;
      }

      setLoading(true);
      try {
        const response = await fetch(`${baseUrl}/sessions/${encodeURIComponent(target.session.name)}`);
        const body = await response.json();
        if (!response.ok) {
          throw new Error(body.message || `HTTP ${response.status}`);
        }
        if (cancelled) {
          return;
        }
        setName(body.name || target.session.name);
        setProject(body.project || "");
        setAutoStart(Boolean(body.auto_start));
        setCronSchedule(body.cron_schedule || "");
        setGithubRepo(body.github_repo || "");
        setAzureProject(body.azure_project || "");
        setConfigText(JSON.stringify(body.config_json || defaultConfigFor(target.session.name), null, 2));
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(`Failed to load session detail: ${String(error)}`);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    loadEditDetail();
    return () => {
      cancelled = true;
    };
  }, [isEdit, target, baseUrl]);

  if (!target) {
    return null;
  }

  const submit = async () => {
    setSaving(true);
    setErrorMessage(null);

    let configJson;
    try {
      configJson = JSON.parse(configText);
    } catch {
      setSaving(false);
      setErrorMessage("config_json must be valid JSON");
      return;
    }

    try {
      if (isEdit) {
        const response = await fetch(`${baseUrl}/sessions/${encodeURIComponent(name)}`, {
          method: "PATCH",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            project: project.trim() === "" ? null : project.trim(),
            config_json: configJson,
            cron_schedule: cronSchedule.trim() === "" ? null : cronSchedule.trim(),
            auto_start: autoStart,
            github_repo: githubRepo.trim() === "" ? null : githubRepo.trim(),
            azure_project: azureProject.trim() === "" ? null : azureProject.trim(),
          }),
        });
        const body = await response.json();
        if (!response.ok) {
          throw new Error(body.message || `HTTP ${response.status}`);
        }
      } else {
        const response = await fetch(`${baseUrl}/sessions`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            name: name.trim(),
            project: project.trim() === "" ? null : project.trim(),
            host_id: defaultHostId,
            config_json: configJson,
            cron_schedule: cronSchedule.trim() === "" ? null : cronSchedule.trim(),
            auto_start: autoStart,
            github_repo: githubRepo.trim() === "" ? null : githubRepo.trim(),
            azure_project: azureProject.trim() === "" ? null : azureProject.trim(),
          }),
        });
        const body = await response.json();
        if (!response.ok) {
          throw new Error(body.message || `HTTP ${response.status}`);
        }
      }

      onSaved(isEdit ? `Updated ${name}` : `Created ${name}`);
    } catch (error) {
      setErrorMessage(String(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.75)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 220,
      }}
      onClick={onClose}
    >
      <div
        style={{
          width: "min(860px, 95vw)",
          maxHeight: "90vh",
          overflowY: "auto",
          background: "#0d1117",
          border: "1px solid #1e2535",
          borderRadius: 10,
          padding: 18,
        }}
        onClick={(event) => event.stopPropagation()}
      >
        <div style={{ fontSize: 12, color: "#94a3b8", marginBottom: 12 }}>
          {isEdit ? "Project Editor" : "New Project"}
        </div>

        {loading ? (
          <div style={{ color: "#64748b", fontSize: 12, padding: "12px 0" }}>Loading project definition...</div>
        ) : (
          <div style={{ display: "grid", gap: 10 }}>
            <label style={{ display: "grid", gap: 5 }}>
              <span style={{ fontSize: 10, color: "#475569" }}>Session Name</span>
              <input
                value={name}
                disabled={isEdit}
                onChange={(event) => {
                  const value = event.target.value;
                  setName(value);
                  if (!isEdit) {
                    try {
                      const parsed = JSON.parse(configText);
                      parsed.session_name = value;
                      setConfigText(JSON.stringify(parsed, null, 2));
                    } catch {
                      setConfigText(JSON.stringify(defaultConfigFor(value), null, 2));
                    }
                  }
                }}
                style={{
                  background: "#0a0e14",
                  border: "1px solid #1e2535",
                  borderRadius: 5,
                  color: "#cbd5e1",
                  padding: "7px 10px",
                }}
              />
            </label>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
              <label style={{ display: "grid", gap: 5 }}>
                <span style={{ fontSize: 10, color: "#475569" }}>Project</span>
                <input
                  value={project}
                  onChange={(event) => setProject(event.target.value)}
                  style={{
                    background: "#0a0e14",
                    border: "1px solid #1e2535",
                    borderRadius: 5,
                    color: "#cbd5e1",
                    padding: "7px 10px",
                  }}
                />
              </label>
              <label style={{ display: "grid", gap: 5 }}>
                <span style={{ fontSize: 10, color: "#475569" }}>Cron Schedule</span>
                <input
                  value={cronSchedule}
                  onChange={(event) => setCronSchedule(event.target.value)}
                  placeholder="optional"
                  style={{
                    background: "#0a0e14",
                    border: "1px solid #1e2535",
                    borderRadius: 5,
                    color: "#cbd5e1",
                    padding: "7px 10px",
                  }}
                />
              </label>
            </div>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
              <label style={{ display: "grid", gap: 5 }}>
                <span style={{ fontSize: 10, color: "#475569" }}>GitHub Repo</span>
                <input
                  value={githubRepo}
                  onChange={(event) => setGithubRepo(event.target.value)}
                  placeholder="owner/repo"
                  style={{
                    background: "#0a0e14",
                    border: "1px solid #1e2535",
                    borderRadius: 5,
                    color: "#cbd5e1",
                    padding: "7px 10px",
                  }}
                />
              </label>
              <label style={{ display: "grid", gap: 5 }}>
                <span style={{ fontSize: 10, color: "#475569" }}>Azure Project</span>
                <input
                  value={azureProject}
                  onChange={(event) => setAzureProject(event.target.value)}
                  style={{
                    background: "#0a0e14",
                    border: "1px solid #1e2535",
                    borderRadius: 5,
                    color: "#cbd5e1",
                    padding: "7px 10px",
                  }}
                />
              </label>
            </div>

            <label style={{ display: "inline-flex", alignItems: "center", gap: 8, fontSize: 11, color: "#94a3b8" }}>
              <input type="checkbox" checked={autoStart} onChange={(event) => setAutoStart(event.target.checked)} />
              auto_start
            </label>

            <label style={{ display: "grid", gap: 5 }}>
              <span style={{ fontSize: 10, color: "#475569" }}>config_json</span>
              <textarea
                value={configText}
                onChange={(event) => setConfigText(event.target.value)}
                rows={12}
                style={{
                  background: "#060810",
                  border: "1px solid #1e2535",
                  borderRadius: 5,
                  color: "#cbd5e1",
                  padding: "8px 10px",
                  fontFamily: "inherit",
                  fontSize: 11,
                }}
              />
            </label>

            {errorMessage && <div style={{ color: "#f87171", fontSize: 11 }}>{errorMessage}</div>}

            <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
              <button
                onClick={onClose}
                style={{
                  border: "1px solid #1e2535",
                  background: "transparent",
                  color: "#94a3b8",
                  borderRadius: 5,
                  padding: "7px 10px",
                  cursor: "pointer",
                }}
              >
                Cancel
              </button>
              <button
                onClick={submit}
                disabled={saving || !name.trim()}
                style={{
                  border: "none",
                  background: "#2563eb",
                  color: "#fff",
                  borderRadius: 5,
                  padding: "7px 10px",
                  cursor: saving ? "default" : "pointer",
                  opacity: saving ? 0.7 : 1,
                }}
              >
                {saving ? "Saving..." : isEdit ? "Save Project" : "Create Project"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default function Dashboard() {
  const baseUrl = useMemo(() => daemonBaseUrl(), []);
  const [view, setView] = useState("grouped");
  const [statusFilter, setStatusFilter] = useState("all");
  const [projectFilter, setProjectFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [jumpTarget, setJumpTarget] = useState(null);
  const [editorTarget, setEditorTarget] = useState(null);
  const [orgEditorOpen, setOrgEditorOpen] = useState(false);
  const [hosts, setHosts] = useState([]);
  const [sessions, setSessions] = useState([]);
  const [discoveryRows, setDiscoveryRows] = useState([]);
  const [defaultTerminal, setDefaultTerminal] = useState("iterm2");
  const [pollIntervalMs, setPollIntervalMs] = useState(DEFAULT_POLL_MS);
  const [errorMessage, setErrorMessage] = useState(null);
  const [loading, setLoading] = useState(true);
  const [lastUpdated, setLastUpdated] = useState(null);
  const [busyBySession, setBusyBySession] = useState({});
  const [actionMessage, setActionMessage] = useState(null);

  useEffect(() => {
    let cancelled = false;

    async function loadConfig() {
      try {
        const response = await fetch(`${baseUrl}/dashboard-config.json`);
        if (!response.ok) {
          throw new Error(`dashboard-config HTTP ${response.status}`);
        }
        const config = await response.json();
        if (cancelled) {
          return;
        }
        if (Array.isArray(config.hosts)) {
          setHosts(config.hosts);
        }
        if (typeof config.default_terminal === "string" && config.default_terminal.length > 0) {
          setDefaultTerminal(config.default_terminal);
        }
        if (Number.isFinite(config.poll_interval_ms) && config.poll_interval_ms > 0) {
          setPollIntervalMs(config.poll_interval_ms);
        }
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(`Failed to load dashboard config: ${String(error)}`);
        }
      }
    }

    loadConfig();
    return () => {
      cancelled = true;
    };
  }, [baseUrl]);

  const refresh = async () => {
    const [sessionsResponse, hostsResponse, discoveryResponse] = await Promise.all([
      fetch(`${baseUrl}/sessions`),
      fetch(`${baseUrl}/hosts`),
      fetch(`${baseUrl}/discovery`),
    ]);

    if (!sessionsResponse.ok) {
      throw new Error(`/sessions HTTP ${sessionsResponse.status}`);
    }
    if (!hostsResponse.ok) {
      throw new Error(`/hosts HTTP ${hostsResponse.status}`);
    }
    if (!discoveryResponse.ok) {
      throw new Error(`/discovery HTTP ${discoveryResponse.status}`);
    }

    const [sessionsBody, hostsBody, discoveryBody] = await Promise.all([
      sessionsResponse.json(),
      hostsResponse.json(),
      discoveryResponse.json(),
    ]);

    const hostRows = Array.isArray(hostsBody) ? hostsBody : [];
    const normalizedSessions = normalizeSessions(sessionsBody, hostRows);

    setHosts(hostRows);
    setSessions(normalizedSessions);
    setDiscoveryRows(normalizeDiscovery(discoveryBody));
    setLastUpdated(new Date());
    setErrorMessage(null);
    setLoading(false);
  };

  useEffect(() => {
    let cancelled = false;

    async function run() {
      try {
        await refresh();
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(`Refresh failed: ${String(error)}`);
          setLoading(false);
        }
      }
    }

    run();
    const timer = window.setInterval(run, pollIntervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [baseUrl, pollIntervalMs]);

  const runStartStop = async (session) => {
    const action = session.status === "stopped" ? "start" : "stop";
    setBusyBySession((prev) => ({ ...prev, [session.name]: true }));
    setActionMessage(null);

    try {
      const response = await fetch(`${baseUrl}/sessions/${encodeURIComponent(session.name)}/${action}`, {
        method: "POST",
      });
      const body = await response.json();
      if (!response.ok) {
        throw new Error(body.message || `HTTP ${response.status}`);
      }
      setActionMessage(body.message || `${action} ${session.name}`);
      await refresh();
    } catch (error) {
      setErrorMessage(`${action} failed for ${session.name}: ${String(error)}`);
    } finally {
      setBusyBySession((prev) => {
        const next = { ...prev };
        delete next[session.name];
        return next;
      });
    }
  };

  const projects = useMemo(
    () => [...new Set(sessions.map((session) => session.project).filter(Boolean))],
    [sessions],
  );

  const filtered = useMemo(() => {
    const searchText = search.trim().toLowerCase();
    return sessions.filter((session) => {
      if (statusFilter !== "all" && session.status !== statusFilter) {
        return false;
      }
      if (projectFilter !== "all" && session.project !== projectFilter) {
        return false;
      }
      if (searchText && !session.name.toLowerCase().includes(searchText)) {
        return false;
      }
      return true;
    });
  }, [sessions, statusFilter, projectFilter, search]);

  const runningCount = sessions.filter((session) => session.status === "running").length;
  const idleCount = sessions.filter((session) => session.status === "idle").length;
  const stoppedCount = sessions.filter((session) => session.status === "stopped").length;
  const activeAgents = sessions
    .flatMap((session) => session.panes)
    .filter((pane) => pane.status === "active").length;
  const openPrs = sessions.reduce((sum, session) => sum + session.openPrCount, 0);
  const defaultHostId = hosts.find((host) => host.is_local)?.id || sessions[0]?.host_id || 1;

  return (
    <div
      style={{
        background: "#060810",
        minHeight: "100vh",
        color: "#e2e8f0",
        fontFamily: "'Berkeley Mono', 'Fira Code', 'JetBrains Mono', monospace",
      }}
    >
      <style>{`
        @keyframes ping { 75%, 100% { transform: scale(2.2); opacity: 0; } }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        input::placeholder, textarea::placeholder { color: #1a2030; }
        button { font-family: inherit; }
      `}</style>

      <div
        style={{
          borderBottom: "1px solid #0a0e14",
          padding: "12px 24px",
          display: "flex",
          alignItems: "center",
          gap: 20,
          position: "sticky",
          top: 0,
          background: "#060810",
          zIndex: 10,
          flexWrap: "wrap",
        }}
      >
        <div style={{ fontSize: 11, fontWeight: 800, color: "#475569", letterSpacing: "0.16em" }}>
          TEAM CONTROL
        </div>
        <div style={{ width: 1, height: 14, background: "#131820" }} />
        <div style={{ display: "flex", gap: 14, fontSize: 11, flexWrap: "wrap" }}>
          <span>
            <span style={{ color: "#10b981" }}>{runningCount}</span>
            <span style={{ color: "#1e2535" }}> run</span>
          </span>
          <span>
            <span style={{ color: "#f59e0b" }}>{idleCount}</span>
            <span style={{ color: "#1e2535" }}> idle</span>
          </span>
          <span>
            <span style={{ color: "#94a3b8" }}>{stoppedCount}</span>
            <span style={{ color: "#1e2535" }}> off</span>
          </span>
          <span style={{ color: "#131820" }}>.</span>
          <span>
            <span style={{ color: "#10b981" }}>{activeAgents}</span>
            <span style={{ color: "#1e2535" }}> agents</span>
          </span>
          <span style={{ color: "#131820" }}>.</span>
          <span>
            <span style={{ color: "#3b82f6" }}>{openPrs}</span>
            <span style={{ color: "#1e2535" }}> PRs</span>
          </span>
        </div>

        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <button
            onClick={() => setOrgEditorOpen(true)}
            style={{
              border: "1px solid #1e2535",
              borderRadius: 5,
              background: "#111827",
              color: "#93c5fd",
              padding: "5px 10px",
              fontSize: 10,
              cursor: "pointer",
              letterSpacing: "0.05em",
            }}
          >
            Org Editor
          </button>
          <button
            onClick={() => setEditorTarget({ mode: "create" })}
            style={{
              border: "1px solid #1e2535",
              borderRadius: 5,
              background: "#102b1f",
              color: "#34d399",
              padding: "5px 10px",
              fontSize: 10,
              cursor: "pointer",
              letterSpacing: "0.05em",
            }}
          >
            + New Project
          </button>
          <span style={{ fontSize: 10, color: "#475569" }}>poll {pollIntervalMs}ms</span>
          {lastUpdated && (
            <span style={{ fontSize: 10, color: "#475569" }}>updated {relativeTime(lastUpdated.toISOString())}</span>
          )}
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="search sessions..."
            style={{
              background: "#0a0e14",
              border: "1px solid #131820",
              borderRadius: 5,
              padding: "5px 10px",
              fontSize: 11,
              color: "#94a3b8",
              outline: "none",
              width: 180,
            }}
          />
          <div
            style={{
              display: "flex",
              background: "#0a0e14",
              borderRadius: 5,
              border: "1px solid #131820",
              overflow: "hidden",
            }}
          >
            {[
              ["grouped", "Project"],
              ["grid", "Grid"],
              ["list", "List"],
              ["discovery", "Discovery"],
            ].map(([value, label]) => (
              <button
                key={value}
                onClick={() => setView(value)}
                style={{
                  padding: "5px 12px",
                  background: view === value ? "#1e2535" : "transparent",
                  border: "none",
                  color: view === value ? "#cbd5e1" : "#334155",
                  cursor: "pointer",
                  fontSize: 10,
                  letterSpacing: "0.04em",
                  transition: "background 0.1s",
                }}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {view !== "discovery" && (
        <div
          style={{
            padding: "8px 24px",
            borderBottom: "1px solid #0a0e14",
            display: "flex",
            gap: 5,
            flexWrap: "wrap",
            alignItems: "center",
          }}
        >
          {["all", "running", "idle", "stopped"].map((filterValue) => (
            <button
              key={filterValue}
              onClick={() => setStatusFilter(filterValue)}
              style={{
                padding: "3px 8px",
                borderRadius: 4,
                fontSize: 10,
                cursor: "pointer",
                background: statusFilter === filterValue ? "#131820" : "transparent",
                border: statusFilter === filterValue ? "1px solid #1e2535" : "1px solid transparent",
                color: statusFilter === filterValue ? "#94a3b8" : "#1e2535",
                letterSpacing: "0.04em",
              }}
            >
              {filterValue}
            </button>
          ))}
          <div style={{ width: 1, height: 12, background: "#131820", margin: "0 4px" }} />
          <button
            onClick={() => setProjectFilter("all")}
            style={{
              padding: "3px 8px",
              borderRadius: 4,
              fontSize: 10,
              cursor: "pointer",
              background: projectFilter === "all" ? "#131820" : "transparent",
              border: projectFilter === "all" ? "1px solid #1e2535" : "1px solid transparent",
              color: projectFilter === "all" ? "#94a3b8" : "#1e2535",
            }}
          >
            all projects
          </button>
          {projects.map((project) => {
            const pc = PROJECT_COLORS[project] || "#6b7280";
            return (
              <button
                key={project}
                onClick={() => setProjectFilter(project)}
                style={{
                  padding: "3px 8px",
                  borderRadius: 4,
                  fontSize: 10,
                  cursor: "pointer",
                  background: projectFilter === project ? `${pc}15` : "transparent",
                  border: projectFilter === project ? `1px solid ${pc}40` : "1px solid transparent",
                  color: projectFilter === project ? pc : "#1e2535",
                }}
              >
                {project}
              </button>
            );
          })}
        </div>
      )}

      {errorMessage && <div style={{ padding: "10px 24px", color: "#f87171", fontSize: 12 }}>{errorMessage}</div>}
      {actionMessage && <div style={{ padding: "10px 24px", color: "#34d399", fontSize: 12 }}>{actionMessage}</div>}

      {loading ? (
        <div style={{ padding: "30px 24px", color: "#64748b", fontSize: 12 }}>Loading dashboard data...</div>
      ) : (
        <div style={{ paddingTop: 20 }}>
          {view === "grid" && (
            <div
              style={{
                padding: "0 24px 24px",
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(250px, 1fr))",
                gap: 10,
              }}
            >
              {filtered.map((session, index) => (
                <GridCard
                  key={`${session.name}-${index}`}
                  session={session}
                  busy={Boolean(busyBySession[session.name])}
                  onJump={setJumpTarget}
                  onStartStop={runStartStop}
                  onEdit={(item) => setEditorTarget({ mode: "edit", session: item })}
                />
              ))}
            </div>
          )}
          {view === "list" && (
            <ListView
              sessions={filtered}
              busyBySession={busyBySession}
              onJump={setJumpTarget}
              onStartStop={runStartStop}
              onEdit={(item) => setEditorTarget({ mode: "edit", session: item })}
            />
          )}
          {view === "grouped" && (
            <GroupedView
              sessions={filtered}
              busyBySession={busyBySession}
              onJump={setJumpTarget}
              onStartStop={runStartStop}
              onEdit={(item) => setEditorTarget({ mode: "edit", session: item })}
            />
          )}
          {view === "discovery" && <DiscoveryView rows={discoveryRows} />}
        </div>
      )}

      <JumpModal
        baseUrl={baseUrl}
        defaultTerminal={defaultTerminal}
        session={jumpTarget}
        onClose={() => setJumpTarget(null)}
      />

      <ProjectEditorModal
        baseUrl={baseUrl}
        defaultHostId={defaultHostId}
        target={editorTarget}
        onClose={() => setEditorTarget(null)}
        onSaved={async (message) => {
          setEditorTarget(null);
          setActionMessage(message);
          await refresh();
        }}
      />

      <OrganizationEditorModal
        baseUrl={baseUrl}
        open={orgEditorOpen}
        onClose={() => setOrgEditorOpen(false)}
        onSaved={(message) => {
          setActionMessage(message);
        }}
      />

      {hosts.length === 0 && !loading && (
        <div style={{ padding: "0 24px 24px", color: "#475569", fontSize: 11 }}>
          No hosts reported by daemon.
        </div>
      )}
    </div>
  );
}
