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
  blocked: { color: "#ef4444", pulse: true },
  stopped: { color: "#1e2535", pulse: false },
  running: { color: "#10b981", pulse: true },
};

const DEFAULT_BASE_URL = "http://localhost:7878";
const DEFAULT_POLL_MS = 15_000;

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
  const s = STATUS_DOT[status] || STATUS_DOT.stopped;
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

function normalizePanes(session) {
  const panes = Array.isArray(session.panes) ? session.panes : [];
  return panes.map((pane, index) => ({
    name: pane.name || `pane-${index}`,
    status: pane.status || "idle",
    lastActivity: pane.last_activity || pane.lastActivity || "unknown",
    currentCommand: pane.current_command || pane.currentCommand || "",
  }));
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
        status: entry.status || "unknown",
        payload,
        toolMessage: entry.tool_message || entry.message || null,
      };
    })
    .filter((entry) => entry.provider !== "unknown");
}

function extractPrs(session, ciEntries) {
  if (Array.isArray(session.prs)) {
    return session.prs;
  }

  const github = ciEntries.find((entry) => entry.provider === "github");
  if (!github || !github.payload) {
    return [];
  }

  if (Array.isArray(github.payload.prs)) {
    return github.payload.prs;
  }

  return [];
}

function extractOpenPrCount(session, ciEntries) {
  const direct = Array.isArray(session.prs) ? session.prs.length : null;
  if (direct !== null) {
    return direct;
  }

  const github = ciEntries.find((entry) => entry.provider === "github");
  if (!github) {
    return 0;
  }
  if (github.payload && Array.isArray(github.payload.prs)) {
    return github.payload.prs.length;
  }
  const numericCandidates = [
    github.payload?.open_pr_count,
    github.payload?.open_prs,
    github.payload?.pr_count,
  ];
  const numeric = numericCandidates.find((value) => Number.isFinite(value));
  return Number.isFinite(numeric) ? Number(numeric) : 0;
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
        status: run.status || run.state || run.result || run.conclusion || "unknown",
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
  const normalizedState = ["active", "idle", "stuck", "offline", "unknown"].includes(state)
    ? state
    : "unknown";
  return {
    state: normalizedState,
    lastTransition: session.atm.last_transition || session.atm.lastTransition || null,
  };
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

function AtmBadge({ atm }) {
  if (!atm) {
    return null;
  }

  return (
    <span
      title={atm.lastTransition ? `last transition ${atm.lastTransition}` : undefined}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        padding: "2px 6px",
        borderRadius: 4,
        border: "1px solid #1e2535",
        fontSize: 9,
        color: "#94a3b8",
        textTransform: "uppercase",
        letterSpacing: "0.04em",
      }}
    >
      <Dot status={atm.state} size={6} />
      {atm.state}
    </span>
  );
}

function CiBadges({ session }) {
  const [showGithubPrs, setShowGithubPrs] = useState(false);

  if (!session.ciEntries.length) {
    return null;
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
        {session.ciEntries.map((entry, index) => {
          if (entry.status === "tool_unavailable") {
            const installHint =
              entry.provider === "github"
                ? "Install gh CLI: brew install gh"
                : entry.provider === "azure"
                  ? "Install az CLI: brew install azure-cli"
                  : "Install required CLI tool";
            return (
              <span
                key={`${entry.provider}-${index}`}
                title={entry.toolMessage || installHint}
                style={{
                  fontSize: 9,
                  color: "#94a3b8",
                  background: "#1e293b",
                  borderRadius: 3,
                  padding: "1px 6px",
                }}
              >
                {entry.provider}: unavailable
              </span>
            );
          }

          if (entry.provider === "github") {
            return (
              <button
                key={`${entry.provider}-${index}`}
                onClick={(event) => {
                  event.stopPropagation();
                  setShowGithubPrs((prev) => !prev);
                }}
                title={`GitHub Actions: ${session.ciRuns.filter((run) => run.provider === "github").length} runs`}
                style={{
                  fontSize: 9,
                  color: "#60a5fa",
                  background: "#172554",
                  borderRadius: 3,
                  padding: "1px 6px",
                  border: "none",
                  cursor: "pointer",
                  fontFamily: "inherit",
                }}
              >
                GH PRs: {session.openPrCount}
              </button>
            );
          }

          if (entry.provider === "azure") {
            return (
              <span
                key={`${entry.provider}-${index}`}
                title={`Azure Pipelines: ${session.ciRuns.filter((run) => run.provider === "azure").length} runs`}
                style={{
                  fontSize: 9,
                  color: "#38bdf8",
                  background: "#082f49",
                  borderRadius: 3,
                  padding: "1px 6px",
                }}
              >
                Azure: {entry.status}
              </span>
            );
          }

          return null;
        })}
      </div>

      {showGithubPrs && (
        <div
          onClick={(event) => event.stopPropagation()}
          style={{
            background: "#0a0e14",
            border: "1px solid #131820",
            borderRadius: 4,
            padding: "6px 8px",
            minWidth: 180,
          }}
        >
          {session.prs.length === 0 && (
            <div style={{ fontSize: 10, color: "#64748b" }}>No open PRs.</div>
          )}
          {session.prs.map((pr, index) => (
            <a
              key={`${pr.url || "pr"}-${index}`}
              href={pr.url || "#"}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => event.stopPropagation()}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                textDecoration: "none",
                padding: "3px 0",
              }}
            >
              <span
                style={{
                  fontSize: 9,
                  color: "#60a5fa",
                  background: "#172554",
                  borderRadius: 3,
                  padding: "1px 5px",
                  flexShrink: 0,
                }}
              >
                #{pr.num || "?"}
              </span>
              <span
                style={{
                  fontSize: 10,
                  color: "#94a3b8",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {pr.title || "Untitled PR"}
              </span>
            </a>
          ))}
        </div>
      )}
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
  const ciEntries = Array.isArray(session.ciEntries) ? session.ciEntries : [];
  const ciRuns = Array.isArray(session.ciRuns) ? session.ciRuns : [];

  const handleJump = async () => {
    setSubmitting(true);
    try {
      const response = await fetch(
        `${baseUrl}/sessions/${encodeURIComponent(session.name)}/jump`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            terminal: defaultTerminal,
            host_id: session.host_id,
          }),
        },
      );
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
        <div style={{ fontSize: 20, color: "#f1f5f9", fontWeight: 700, marginBottom: 3 }}>
          {session.name}
        </div>
        <div style={{ fontSize: 11, color: pc, marginBottom: 6 }}>
          {session.project || "unassigned"} on {hostLabel(session.host)}
        </div>
        <div style={{ fontSize: 10, color: "#64748b", marginBottom: 16 }}>
          {session.host?.reachable ? "host reachable" : `host unreachable (${hostBadge(session.host)})`}
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
          <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.1em", marginBottom: 8 }}>
            PANES
          </div>
          {session.panes.length === 0 && (
            <div style={{ fontSize: 11, color: "#475569" }}>No panes reported.</div>
          )}
          {session.panes.map((pane, index) => (
            <div
              key={`${pane.name}-${index}`}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "4px 0",
                borderBottom: index < session.panes.length - 1 ? "1px solid #0f172a" : "none",
              }}
            >
              <Dot status={pane.status} size={6} />
              <span style={{ fontSize: 12, color: "#94a3b8", flex: 1 }}>{pane.name}</span>
              <span style={{ fontSize: 10, color: "#475569" }}>{pane.lastActivity}</span>
            </div>
          ))}
        </div>

        {session.prs.length > 0 && (
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.1em", marginBottom: 8 }}>
              OPEN PRS
            </div>
            {session.prs.map((pr, index) => (
              <a
                key={`${pr.url || "pr"}-${index}`}
                href={pr.url || "#"}
                target="_blank"
                rel="noreferrer"
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "5px 0",
                  textDecoration: "none",
                  borderBottom: index < session.prs.length - 1 ? "1px solid #0f172a" : "none",
                }}
              >
                <span
                  style={{
                    fontSize: 10,
                    color: pc,
                    background: `${pc}18`,
                    borderRadius: 3,
                    padding: "2px 6px",
                    flexShrink: 0,
                  }}
                >
                  #{pr.num || "?"}
                </span>
                <span style={{ fontSize: 11, color: "#94a3b8", flex: 1 }}>
                  {pr.title || "Untitled PR"}
                </span>
                <span style={{ fontSize: 11, color: "#334155" }}>↗</span>
              </a>
            ))}
          </div>
        )}

        {ciEntries.length > 0 && (
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 10, color: "#334155", letterSpacing: "0.1em", marginBottom: 8 }}>
              CI RUN STATUS
            </div>
            {ciRuns.length === 0 && (
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {ciEntries.map((entry, index) => (
                  <div
                    key={`${entry.provider}-${index}`}
                    style={{ display: "flex", alignItems: "center", gap: 8, padding: "3px 0" }}
                  >
                    <span
                      style={{
                        fontSize: 9,
                        color: entry.provider === "github" ? "#60a5fa" : "#38bdf8",
                        background: entry.provider === "github" ? "#172554" : "#082f49",
                        borderRadius: 3,
                        padding: "1px 5px",
                        flexShrink: 0,
                        textTransform: "uppercase",
                      }}
                    >
                      {entry.provider}
                    </span>
                    <span style={{ fontSize: 10, color: "#64748b" }}>{entry.status}</span>
                  </div>
                ))}
              </div>
            )}
            {ciRuns.slice(0, 8).map((run, index) => (
              <div
                key={`${run.provider}-${run.title}-${index}`}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "5px 0",
                  borderBottom: index < Math.min(ciRuns.length, 8) - 1 ? "1px solid #0f172a" : "none",
                }}
              >
                <span
                  style={{
                    fontSize: 9,
                    color: run.provider === "github" ? "#60a5fa" : "#38bdf8",
                    background: run.provider === "github" ? "#172554" : "#082f49",
                    borderRadius: 3,
                    padding: "1px 5px",
                    flexShrink: 0,
                    textTransform: "uppercase",
                  }}
                >
                  {run.provider}
                </span>
                <span style={{ fontSize: 11, color: "#94a3b8", flex: 1 }}>
                  {run.title}
                </span>
                <span style={{ fontSize: 10, color: "#64748b" }}>
                  {run.conclusion || run.status}
                </span>
              </div>
            ))}
          </div>
        )}

        {feedback && (
          <div
            style={{
              fontSize: 11,
              marginBottom: 14,
              color: feedback.ok ? "#34d399" : "#f87171",
            }}
          >
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

function GridCard({ session, onJump }) {
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

      <div style={{ padding: "6px 12px 10px" }}>
        {session.panes.slice(0, 4).map((pane, index) => (
          <div key={`${pane.name}-${index}`} style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0" }}>
            <Dot status={pane.status} size={5} />
            <span style={{ fontSize: 10, color: "#94a3b8", flex: 1 }}>{pane.name}</span>
            <span style={{ fontSize: 9, color: "#334155" }}>{pane.lastActivity}</span>
          </div>
        ))}

        <div style={{ marginTop: 8, display: "flex", justifyContent: "space-between", gap: 8, alignItems: "center" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 4, alignItems: "flex-start" }}>
            <AtmBadge atm={session.atm} />
            <CiBadges session={session} />
          </div>
          <span style={{ fontSize: 9, color: "#475569" }}>{hostBadge(session.host)}</span>
        </div>
      </div>
    </div>
  );
}

function ListView({ sessions, onJump }) {
  return (
    <div style={{ padding: "0 24px 24px", overflowX: "auto" }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12, minWidth: 780 }}>
        <thead>
          <tr style={{ borderBottom: "1px solid #131820" }}>
            {["", "Session", "Project", "Host", "Status", "Activity", "Panes", "Active", "Open PRs", "Last Activity"].map(
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
            const activePanes = session.panes.filter((pane) => pane.status === "active").length;
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
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <Dot status={session.status} size={6} />
                    <span style={{ color: "#64748b", fontSize: 11 }}>{session.status}</span>
                  </div>
                </td>
                <td style={{ padding: "7px 12px", color: "#94a3b8", fontSize: 11 }}>
                  {session.atm ? (
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
                      <Dot status={session.atm.state} size={6} />
                      {session.atm.state}
                    </span>
                  ) : (
                    ""
                  )}
                </td>
                <td style={{ padding: "7px 12px", color: "#334155" }}>{session.panes.length}</td>
                <td style={{ padding: "7px 12px", color: activePanes > 0 ? "#10b981" : "#1e2535" }}>{activePanes}</td>
                <td style={{ padding: "7px 12px", color: "#60a5fa" }}>{session.openPrCount || "-"}</td>
                <td style={{ padding: "7px 12px", color: "#475569", fontSize: 11 }}>
                  {session.panes[0]?.lastActivity || relativeTime(session.polled_at)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function GroupedView({ sessions, onJump }) {
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
                      gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
                      gap: 8,
                    }}
                  >
                    {hostSessions.map((session, index) => (
                      <GridCard key={`${session.name}-${index}`} session={session} onJump={onJump} />
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

export default function Dashboard() {
  const baseUrl = useMemo(() => daemonBaseUrl(), []);
  const [view, setView] = useState("grouped");
  const [statusFilter, setStatusFilter] = useState("all");
  const [projectFilter, setProjectFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [jumpTarget, setJumpTarget] = useState(null);
  const [hosts, setHosts] = useState([]);
  const [sessions, setSessions] = useState([]);
  const [defaultTerminal, setDefaultTerminal] = useState("iterm2");
  const [pollIntervalMs, setPollIntervalMs] = useState(DEFAULT_POLL_MS);
  const [errorMessage, setErrorMessage] = useState(null);
  const [loading, setLoading] = useState(true);
  const [lastUpdated, setLastUpdated] = useState(null);

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

  useEffect(() => {
    let cancelled = false;

    async function refresh() {
      try {
        const [sessionsResponse, hostsResponse] = await Promise.all([
          fetch(`${baseUrl}/sessions`),
          fetch(`${baseUrl}/hosts`),
        ]);
        if (!sessionsResponse.ok) {
          throw new Error(`/sessions HTTP ${sessionsResponse.status}`);
        }
        if (!hostsResponse.ok) {
          throw new Error(`/hosts HTTP ${hostsResponse.status}`);
        }

        const [sessionsBody, hostsBody] = await Promise.all([
          sessionsResponse.json(),
          hostsResponse.json(),
        ]);
        if (cancelled) {
          return;
        }

        const hostRows = Array.isArray(hostsBody) ? hostsBody : [];
        const hostMap = new Map(hostRows.map((host) => [host.id, host]));

        const normalizedSessions = (Array.isArray(sessionsBody) ? sessionsBody : []).map((row) => {
          const ciEntries = normalizeCi(row);
          const prs = extractPrs(row, ciEntries);
          const ciRuns = extractRuns(ciEntries);
          return {
            ...row,
            status: row.status || "stopped",
            project: row.project || "unassigned",
            panes: normalizePanes(row),
            atm: normalizeAtm(row),
            ciEntries,
            ciRuns,
            prs,
            openPrCount: extractOpenPrCount(row, ciEntries),
            host: hostMap.get(row.host_id) || null,
          };
        });

        setHosts(hostRows);
        setSessions(normalizedSessions);
        setErrorMessage(null);
        setLastUpdated(new Date());
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(`Refresh failed: ${String(error)}`);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    refresh();
    const timer = window.setInterval(refresh, pollIntervalMs);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [baseUrl, pollIntervalMs]);

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
        input::placeholder { color: #1a2030; }
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
              ["grid", "Grid"],
              ["list", "List"],
              ["grouped", "Project"],
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
              border:
                statusFilter === filterValue
                  ? "1px solid #1e2535"
                  : "1px solid transparent",
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
            border:
              projectFilter === "all"
                ? "1px solid #1e2535"
                : "1px solid transparent",
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
                border:
                  projectFilter === project
                    ? `1px solid ${pc}40`
                    : "1px solid transparent",
                color: projectFilter === project ? pc : "#1e2535",
              }}
            >
              {project}
            </button>
          );
        })}
      </div>

      {errorMessage && (
        <div style={{ padding: "10px 24px", color: "#f87171", fontSize: 12 }}>{errorMessage}</div>
      )}

      {loading ? (
        <div style={{ padding: "30px 24px", color: "#64748b", fontSize: 12 }}>Loading dashboard data...</div>
      ) : (
        <div style={{ paddingTop: 20 }}>
          {view === "grid" && (
            <div
              style={{
                padding: "0 24px 24px",
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
                gap: 10,
              }}
            >
              {filtered.map((session, index) => (
                <GridCard key={`${session.name}-${index}`} session={session} onJump={setJumpTarget} />
              ))}
            </div>
          )}
          {view === "list" && <ListView sessions={filtered} onJump={setJumpTarget} />}
          {view === "grouped" && <GroupedView sessions={filtered} onJump={setJumpTarget} />}
        </div>
      )}

      <JumpModal
        baseUrl={baseUrl}
        defaultTerminal={defaultTerminal}
        session={jumpTarget}
        onClose={() => setJumpTarget(null)}
      />

      {hosts.length === 0 && !loading && (
        <div style={{ padding: "0 24px 24px", color: "#475569", fontSize: 11 }}>
          No hosts reported by daemon.
        </div>
      )}
    </div>
  );
}
