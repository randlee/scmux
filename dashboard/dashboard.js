const { useEffect, useMemo, useState } = React;

const PROJECT_COLORS = {
  "radiant-p3": "#3b82f6",
  atm: "#10b981",
  beads: "#f59e0b",
  provenance: "#8b5cf6",
  "synaptic-canvas": "#ec4899",
  "claude-history": "#06b6d4",
  "ui-platform": "#f97316",
  "dolt-registry": "#84cc16"
};
const STATUS_DOT = {
  active: {
    color: "#10b981",
    pulse: true
  },
  idle: {
    color: "#f59e0b",
    pulse: false
  },
  stuck: {
    color: "#ef4444",
    pulse: true
  },
  offline: {
    color: "#64748b",
    pulse: false
  },
  unknown: {
    color: "#334155",
    pulse: false
  },
  stopped: {
    color: "#1e2535",
    pulse: false
  },
  running: {
    color: "#10b981",
    pulse: true
  },
  starting: {
    color: "#60a5fa",
    pulse: true
  },
  done: {
    color: "#a78bfa",
    pulse: false
  }
};
const DEFAULT_BASE_URL = "http://localhost:7878";
const DEFAULT_POLL_MS = 15_000;
function daemonBaseUrl() {
  if (typeof window === "undefined") {
    return DEFAULT_BASE_URL;
  }
  const {
    origin,
    protocol
  } = window.location;
  if (!origin || origin === "null" || protocol === "file:") {
    return DEFAULT_BASE_URL;
  }
  return origin;
}
function Dot({
  status,
  size = 7
}) {
  const s = STATUS_DOT[status] || STATUS_DOT.unknown;
  return /*#__PURE__*/React.createElement("span", {
    style: {
      position: "relative",
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      width: size,
      height: size,
      flexShrink: 0
    }
  }, s.pulse && /*#__PURE__*/React.createElement("span", {
    style: {
      position: "absolute",
      inset: 0,
      borderRadius: "50%",
      backgroundColor: s.color,
      opacity: 0.35,
      animation: "ping 1.5s cubic-bezier(0,0,0.2,1) infinite"
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      width: size,
      height: size,
      borderRadius: "50%",
      backgroundColor: s.color,
      display: "block"
    }
  }));
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
      currentCommand: pane.current_command || pane.currentCommand || ""
    };
  });
}
function normalizeCi(session) {
  const source = Array.isArray(session.session_ci) ? session.session_ci : Array.isArray(session.ci) ? session.ci : [];
  return source.map(entry => {
    const payload = parseCiPayload(entry.data_json || entry.data || entry.payload);
    return {
      provider: entry.provider || "unknown",
      status: String(entry.status || "unknown").toLowerCase(),
      payload,
      toolMessage: entry.tool_message || entry.message || null
    };
  }).filter(entry => entry.provider !== "unknown");
}
function extractPrs(session, ciEntries) {
  if (Array.isArray(session.prs)) {
    return session.prs.map(pr => ({
      num: pr.num ?? pr.number ?? pr.id ?? "?",
      title: pr.title || "Untitled PR",
      url: pr.url || pr.web_url || null
    }));
  }
  const github = ciEntries.find(entry => entry.provider === "github");
  if (github?.payload && Array.isArray(github.payload.prs)) {
    return github.payload.prs.map(pr => ({
      num: pr.num ?? pr.number ?? pr.id ?? "?",
      title: pr.title || "Untitled PR",
      url: pr.url || pr.web_url || null
    }));
  }
  return [];
}
function extractRuns(ciEntries) {
  const rows = [];
  ciEntries.forEach(entry => {
    if (!entry.payload || !Array.isArray(entry.payload.runs)) {
      return;
    }
    entry.payload.runs.forEach((run, index) => {
      rows.push({
        provider: entry.provider,
        title: run.displayTitle || run.name || run.pipeline?.name || run.definition?.name || `run-${index + 1}`,
        status: String(run.status || run.state || run.result || run.conclusion || "unknown").toLowerCase(),
        conclusion: run.conclusion || run.result || null,
        branch: run.headBranch || run.sourceBranch || run.branch || null,
        createdAt: run.createdAt || run.creationDate || run.queueTime || run.finishTime || null,
        url: run.url || run.webUrl || run._links?.web?.href || null
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
    state: ["active", "idle", "stuck", "offline", "unknown"].includes(state) ? state : "unknown",
    lastTransition: session.atm.last_transition || session.atm.lastTransition || null
  };
}
function normalizeSessions(sessionRows, hostRows) {
  const hostMap = new Map((Array.isArray(hostRows) ? hostRows : []).map(host => [host.id, host]));
  return (Array.isArray(sessionRows) ? sessionRows : []).map(row => {
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
      host: hostMap.get(row.host_id) || null
    };
  });
}
function normalizeDiscovery(rows) {
  return (Array.isArray(rows) ? rows : []).map(row => ({
    name: row.name || "unknown",
    panes: normalizePanes(row)
  }));
}
function ciRunTone(run) {
  const status = String(run?.status || "unknown").toLowerCase();
  const conclusion = String(run?.conclusion || "").toLowerCase();
  const value = `${status} ${conclusion}`;
  if (value.includes("in_progress") || value.includes("queued") || value.includes("running")) {
    return {
      color: "#f59e0b",
      text: "running"
    };
  }
  if (value.includes("success") || value.includes("pass") || value.includes("succeeded") || value.includes("completed")) {
    return {
      color: "#10b981",
      text: "pass"
    };
  }
  if (value.includes("fail") || value.includes("error") || value.includes("cancel")) {
    return {
      color: "#ef4444",
      text: "fail"
    };
  }
  return {
    color: "#64748b",
    text: "unknown"
  };
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
    return {
      opacity: baseOpacity
    };
  }
  return {
    opacity: baseOpacity * 0.75,
    filter: "grayscale(1)"
  };
}
function SessionActionButtons({
  session,
  busy,
  onStartStop,
  onEdit
}) {
  const canStart = session.status === "stopped";
  const actionLabel = canStart ? "Start" : "Stop";
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 6
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: event => {
      event.stopPropagation();
      onStartStop(session);
    },
    disabled: busy,
    style: {
      border: "1px solid #1e2535",
      borderRadius: 4,
      fontSize: 10,
      padding: "2px 8px",
      background: canStart ? "#102b1f" : "#2b1212",
      color: canStart ? "#34d399" : "#fca5a5",
      cursor: busy ? "default" : "pointer"
    }
  }, busy ? "..." : actionLabel), /*#__PURE__*/React.createElement("button", {
    onClick: event => {
      event.stopPropagation();
      onEdit(session);
    },
    style: {
      border: "1px solid #1e2535",
      borderRadius: 4,
      fontSize: 10,
      padding: "2px 8px",
      background: "#0f172a",
      color: "#93c5fd",
      cursor: "pointer"
    }
  }, "Edit"));
}
function CiSummary({
  session
}) {
  if (!session.ciEntries.length) {
    return null;
  }
  const runs = session.ciRuns.slice(0, 4);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 5,
      flexWrap: "wrap"
    }
  }, session.ciEntries.map((entry, index) => /*#__PURE__*/React.createElement("span", {
    key: `${entry.provider}-${index}`,
    title: entry.toolMessage || undefined,
    style: {
      fontSize: 9,
      color: entry.provider === "github" ? "#60a5fa" : "#38bdf8",
      background: entry.provider === "github" ? "#172554" : "#082f49",
      borderRadius: 3,
      padding: "1px 6px",
      textTransform: "uppercase"
    }
  }, entry.provider))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 5,
      flexWrap: "wrap"
    }
  }, runs.map((run, index) => {
    const tone = ciRunTone(run);
    return /*#__PURE__*/React.createElement("span", {
      key: `${run.provider}-${run.title}-${index}`,
      title: run.title,
      style: {
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        fontSize: 9,
        color: "#94a3b8",
        border: "1px solid #1e2535",
        borderRadius: 3,
        padding: "1px 5px"
      }
    }, /*#__PURE__*/React.createElement("span", {
      style: {
        display: "inline-block",
        width: 6,
        height: 6,
        borderRadius: "50%",
        background: tone.color
      }
    }), tone.text);
  })));
}
function GridCard({
  session,
  busy,
  onJump,
  onStartStop,
  onEdit
}) {
  const pc = PROJECT_COLORS[session.project] || "#6b7280";
  const activePanes = session.panes.filter(pane => pane.status === "active").length;
  return /*#__PURE__*/React.createElement("div", {
    onClick: () => onJump(session),
    style: {
      background: "#0d1117",
      border: "1px solid #131820",
      borderRadius: 8,
      overflow: "hidden",
      cursor: "pointer",
      transition: "border-color 0.15s, transform 0.1s",
      ...sessionStyle(session)
    },
    onMouseEnter: event => {
      event.currentTarget.style.borderColor = `${pc}55`;
      event.currentTarget.style.transform = "translateY(-1px)";
    },
    onMouseLeave: event => {
      event.currentTarget.style.borderColor = "#131820";
      event.currentTarget.style.transform = "translateY(0)";
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8,
      padding: "8px 12px",
      borderBottom: "1px solid #0a0e14"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 3,
      height: 26,
      borderRadius: 2,
      background: pc,
      flexShrink: 0
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      fontWeight: 600,
      color: "#e2e8f0",
      whiteSpace: "nowrap",
      overflow: "hidden",
      textOverflow: "ellipsis"
    }
  }, session.name), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 10,
      color: pc,
      opacity: 0.8
    }
  }, (session.project || "unassigned") + " | " + hostLabel(session.host))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      alignItems: "flex-end",
      gap: 3
    }
  }, /*#__PURE__*/React.createElement(Dot, {
    status: session.status,
    size: 7
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 9,
      color: "#334155"
    }
  }, activePanes, "/", session.panes.length))), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "6px 12px 10px",
      display: "flex",
      flexDirection: "column",
      gap: 8
    }
  }, session.panes.slice(0, 4).map((pane, index) => /*#__PURE__*/React.createElement("div", {
    key: `${pane.name}-${index}`,
    style: {
      display: "flex",
      alignItems: "center",
      gap: 6,
      padding: "2px 0"
    }
  }, /*#__PURE__*/React.createElement(Dot, {
    status: pane.status,
    size: 5
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#94a3b8",
      flex: 1
    }
  }, pane.name), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 9,
      color: "#64748b",
      textTransform: "uppercase"
    }
  }, pane.status))), /*#__PURE__*/React.createElement(CiSummary, {
    session: session
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "space-between",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 9,
      color: "#475569"
    }
  }, hostBadge(session.host)), /*#__PURE__*/React.createElement(SessionActionButtons, {
    session: session,
    busy: busy,
    onStartStop: onStartStop,
    onEdit: onEdit
  }))));
}
function ListView({
  sessions,
  busyBySession,
  onJump,
  onStartStop,
  onEdit
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 24px 24px",
      overflowX: "auto"
    }
  }, /*#__PURE__*/React.createElement("table", {
    style: {
      width: "100%",
      borderCollapse: "collapse",
      fontSize: 12,
      minWidth: 960
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", {
    style: {
      borderBottom: "1px solid #131820"
    }
  }, ["", "Session", "Project", "Host", "Status", "Pane States", "Open PRs", "Actions"].map(header => /*#__PURE__*/React.createElement("th", {
    key: header,
    style: {
      padding: "8px 12px",
      textAlign: "left",
      fontSize: 10,
      color: "#1e2535",
      letterSpacing: "0.1em",
      fontWeight: 600
    }
  }, header)))), /*#__PURE__*/React.createElement("tbody", null, sessions.map((session, index) => {
    const pc = PROJECT_COLORS[session.project] || "#6b7280";
    return /*#__PURE__*/React.createElement("tr", {
      key: `${session.name}-${index}`,
      onClick: () => onJump(session),
      style: {
        borderBottom: "1px solid #0a0e14",
        cursor: "pointer",
        transition: "background 0.1s",
        ...sessionStyle(session)
      },
      onMouseEnter: event => {
        event.currentTarget.style.background = "#0f1117";
      },
      onMouseLeave: event => {
        event.currentTarget.style.background = "transparent";
      }
    }, /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px"
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        width: 3,
        height: 18,
        borderRadius: 2,
        background: pc
      }
    })), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px",
        color: "#cbd5e1",
        fontWeight: 500
      }
    }, session.name), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px",
        color: pc,
        fontSize: 11
      }
    }, session.project || "unassigned"), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px",
        color: "#94a3b8",
        fontSize: 11
      }
    }, hostLabel(session.host)), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px"
      }
    }, /*#__PURE__*/React.createElement("span", {
      style: {
        display: "inline-flex",
        alignItems: "center",
        gap: 6
      }
    }, /*#__PURE__*/React.createElement(Dot, {
      status: session.status,
      size: 6
    }), /*#__PURE__*/React.createElement("span", {
      style: {
        color: "#64748b",
        fontSize: 11
      }
    }, session.status))), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px",
        color: "#94a3b8",
        fontSize: 10
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        gap: 4,
        flexWrap: "wrap"
      }
    }, session.panes.slice(0, 4).map((pane, paneIndex) => /*#__PURE__*/React.createElement("span", {
      key: `${pane.name}-${paneIndex}`,
      style: {
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        border: "1px solid #1e2535",
        borderRadius: 3,
        padding: "1px 5px"
      }
    }, /*#__PURE__*/React.createElement(Dot, {
      status: pane.status,
      size: 5
    }), pane.name, ":", pane.status)))), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px",
        color: "#60a5fa"
      }
    }, session.openPrCount || "-"), /*#__PURE__*/React.createElement("td", {
      style: {
        padding: "7px 12px"
      }
    }, /*#__PURE__*/React.createElement(SessionActionButtons, {
      session: session,
      busy: Boolean(busyBySession[session.name]),
      onStartStop: onStartStop,
      onEdit: onEdit
    })));
  }))));
}
function GroupedView({
  sessions,
  busyBySession,
  onJump,
  onStartStop,
  onEdit
}) {
  const byProject = useMemo(() => {
    const grouped = new Map();
    sessions.forEach(session => {
      const project = session.project || "unassigned";
      if (!grouped.has(project)) {
        grouped.set(project, []);
      }
      grouped.get(project).push(session);
    });
    return grouped;
  }, [sessions]);
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 24px 24px",
      display: "flex",
      flexDirection: "column",
      gap: 28
    }
  }, Array.from(byProject.entries()).map(([project, projectSessions]) => {
    const pc = PROJECT_COLORS[project] || "#6b7280";
    const running = projectSessions.filter(session => session.status === "running").length;
    const totalPrs = projectSessions.reduce((sum, session) => sum + session.openPrCount, 0);
    const byHost = new Map();
    projectSessions.forEach(session => {
      const key = session.host?.id || `unknown-${session.host_id}`;
      if (!byHost.has(key)) {
        byHost.set(key, {
          host: session.host,
          sessions: []
        });
      }
      byHost.get(key).sessions.push(session);
    });
    return /*#__PURE__*/React.createElement("div", {
      key: project
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        alignItems: "center",
        gap: 10,
        marginBottom: 12,
        paddingBottom: 8,
        borderBottom: `1px solid ${pc}25`
      }
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        width: 4,
        height: 16,
        borderRadius: 2,
        background: pc
      }
    }), /*#__PURE__*/React.createElement("span", {
      style: {
        fontSize: 12,
        fontWeight: 700,
        color: pc,
        letterSpacing: "0.06em"
      }
    }, project), /*#__PURE__*/React.createElement("span", {
      style: {
        fontSize: 10,
        color: "#334155"
      }
    }, running, "/", projectSessions.length, " running"), totalPrs > 0 && /*#__PURE__*/React.createElement("span", {
      style: {
        fontSize: 10,
        color: pc,
        background: `${pc}18`,
        borderRadius: 3,
        padding: "1px 7px"
      }
    }, totalPrs, " PR", totalPrs !== 1 ? "s" : "")), /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        flexDirection: "column",
        gap: 14
      }
    }, Array.from(byHost.values()).map(({
      host,
      sessions: hostSessions
    }) => /*#__PURE__*/React.createElement("div", {
      key: host?.id || `unknown-${hostSessions[0]?.host_id || "na"}`
    }, /*#__PURE__*/React.createElement("div", {
      style: {
        display: "flex",
        alignItems: "center",
        gap: 8,
        marginBottom: 8
      }
    }, /*#__PURE__*/React.createElement("span", {
      style: {
        fontSize: 10,
        color: "#64748b",
        letterSpacing: "0.08em"
      }
    }, "HOST ", hostLabel(host).toUpperCase()), /*#__PURE__*/React.createElement("span", {
      style: {
        fontSize: 10,
        color: host?.reachable ? "#10b981" : "#94a3b8"
      }
    }, host?.reachable ? "reachable" : `last seen ${relativeTime(host?.last_seen)}`)), /*#__PURE__*/React.createElement("div", {
      style: {
        display: "grid",
        gridTemplateColumns: "repeat(auto-fill, minmax(250px, 1fr))",
        gap: 10
      }
    }, hostSessions.map((session, index) => /*#__PURE__*/React.createElement(GridCard, {
      key: `${session.name}-${index}`,
      session: session,
      busy: Boolean(busyBySession[session.name]),
      onJump: onJump,
      onStartStop: onStartStop,
      onEdit: onEdit
    })))))));
  }));
}
function DiscoveryView({
  rows
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "16px 24px 24px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      color: "#64748b",
      marginBottom: 12
    }
  }, "Raw tmux discovery (informational only; no definition writes)"), /*#__PURE__*/React.createElement("div", {
    style: {
      overflowX: "auto"
    }
  }, /*#__PURE__*/React.createElement("table", {
    style: {
      width: "100%",
      borderCollapse: "collapse",
      minWidth: 800
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", {
    style: {
      borderBottom: "1px solid #131820"
    }
  }, ["Session", "Pane", "State", "Command", "Last Activity"].map(header => /*#__PURE__*/React.createElement("th", {
    key: header,
    style: {
      textAlign: "left",
      fontSize: 10,
      color: "#334155",
      letterSpacing: "0.1em",
      padding: "7px 10px"
    }
  }, header)))), /*#__PURE__*/React.createElement("tbody", null, rows.length === 0 && /*#__PURE__*/React.createElement("tr", null, /*#__PURE__*/React.createElement("td", {
    colSpan: 5,
    style: {
      padding: "16px 10px",
      color: "#64748b",
      fontSize: 11
    }
  }, "No discovered tmux sessions.")), rows.map((row, rowIndex) => row.panes.length ? row.panes.map((pane, paneIndex) => /*#__PURE__*/React.createElement("tr", {
    key: `${row.name}-${pane.name}-${paneIndex}`,
    style: {
      borderBottom: "1px solid #0a0e14"
    }
  }, /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#cbd5e1",
      fontSize: 11
    }
  }, paneIndex === 0 ? row.name : ""), /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#94a3b8",
      fontSize: 11
    }
  }, pane.name), /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#94a3b8",
      fontSize: 11
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6
    }
  }, /*#__PURE__*/React.createElement(Dot, {
    status: pane.status,
    size: 6
  }), pane.status)), /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#475569",
      fontSize: 11
    }
  }, pane.currentCommand || "-"), /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#475569",
      fontSize: 11
    }
  }, pane.lastActivity))) : /*#__PURE__*/React.createElement("tr", {
    key: `${row.name}-${rowIndex}`,
    style: {
      borderBottom: "1px solid #0a0e14"
    }
  }, /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#cbd5e1",
      fontSize: 11
    }
  }, row.name), /*#__PURE__*/React.createElement("td", {
    style: {
      padding: "7px 10px",
      color: "#64748b",
      fontSize: 11
    },
    colSpan: 4
  }, "no panes reported")))))));
}
function JumpModal({
  baseUrl,
  defaultTerminal,
  session,
  onClose
}) {
  const [submitting, setSubmitting] = useState(false);
  const [feedback, setFeedback] = useState(null);
  useEffect(() => {
    if (!session) {
      return undefined;
    }
    const onKeyDown = event => {
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
        headers: {
          "content-type": "application/json"
        },
        body: JSON.stringify({
          terminal: defaultTerminal,
          host_id: session.host_id
        })
      });
      const body = await response.json();
      if (!response.ok) {
        setFeedback({
          ok: false,
          message: body.message || `HTTP ${response.status}`
        });
      } else {
        setFeedback({
          ok: body.ok,
          message: body.message || "No message"
        });
      }
    } catch (error) {
      setFeedback({
        ok: false,
        message: String(error)
      });
    } finally {
      setSubmitting(false);
    }
  };
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: "fixed",
      inset: 0,
      background: "rgba(0,0,0,0.8)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      zIndex: 200,
      backdropFilter: "blur(6px)"
    },
    onClick: onClose
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: "#0d1117",
      border: `1px solid ${pc}50`,
      borderRadius: 12,
      padding: 24,
      minWidth: 360,
      maxWidth: 680,
      width: "92vw",
      fontFamily: "inherit"
    },
    onClick: event => event.stopPropagation()
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 10,
      color: "#334155",
      letterSpacing: "0.12em",
      marginBottom: 6
    }
  }, "JUMP TO SESSION"), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 20,
      color: "#f1f5f9",
      fontWeight: 700,
      marginBottom: 3
    }
  }, session.name), /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      color: pc,
      marginBottom: 6
    }
  }, session.project || "unassigned", " on ", hostLabel(session.host)), /*#__PURE__*/React.createElement("div", {
    style: {
      background: "#060810",
      borderRadius: 6,
      padding: "10px 14px",
      marginBottom: 18,
      fontSize: 11,
      color: "#94a3b8",
      overflowX: "auto",
      whiteSpace: "nowrap"
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#334155"
    }
  }, "$ "), cmd), /*#__PURE__*/React.createElement("div", {
    style: {
      marginBottom: 16
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 10,
      color: "#334155",
      letterSpacing: "0.1em",
      marginBottom: 8
    }
  }, "PANES"), session.panes.length === 0 && /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      color: "#475569"
    }
  }, "No panes reported."), session.panes.map((pane, index) => /*#__PURE__*/React.createElement("div", {
    key: `${pane.name}-${index}`,
    style: {
      display: "grid",
      gridTemplateColumns: "auto 1fr auto",
      alignItems: "center",
      gap: 8,
      padding: "4px 0",
      borderBottom: index < session.panes.length - 1 ? "1px solid #0f172a" : "none"
    }
  }, /*#__PURE__*/React.createElement(Dot, {
    status: pane.status,
    size: 6
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 12,
      color: "#94a3b8",
      whiteSpace: "nowrap",
      overflow: "hidden",
      textOverflow: "ellipsis"
    }
  }, pane.name, " (", pane.currentCommand || "-", ")"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#64748b",
      textTransform: "uppercase"
    }
  }, pane.status)))), feedback && /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      marginBottom: 14,
      color: feedback.ok ? "#34d399" : "#f87171"
    }
  }, feedback.message), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: handleJump,
    disabled: submitting,
    style: {
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
      opacity: submitting ? 0.7 : 1
    }
  }, submitting ? "Launching..." : "Open in iTerm2 ->"), /*#__PURE__*/React.createElement("button", {
    style: {
      padding: "10px 16px",
      background: "transparent",
      border: "1px solid #1e2535",
      borderRadius: 6,
      color: "#475569",
      fontSize: 12,
      cursor: "pointer",
      fontFamily: "inherit"
    },
    onClick: onClose
  }, "esc"))));
}
function defaultConfigFor(name) {
  return {
    session_name: name || "new-session",
    panes: [{
      name: "agent",
      command: "sleep 1",
      atm_agent: "agent",
      atm_team: "scmux-dev"
    }]
  };
}
function ProjectEditorModal({
  baseUrl,
  defaultHostId,
  target,
  onClose,
  onSaved
}) {
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
  const [configText, setConfigText] = useState(JSON.stringify(defaultConfigFor(target?.session?.name || "new-session"), null, 2));
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
          headers: {
            "content-type": "application/json"
          },
          body: JSON.stringify({
            project: project.trim() === "" ? null : project.trim(),
            config_json: configJson,
            cron_schedule: cronSchedule.trim() === "" ? null : cronSchedule.trim(),
            auto_start: autoStart,
            github_repo: githubRepo.trim() === "" ? null : githubRepo.trim(),
            azure_project: azureProject.trim() === "" ? null : azureProject.trim()
          })
        });
        const body = await response.json();
        if (!response.ok) {
          throw new Error(body.message || `HTTP ${response.status}`);
        }
      } else {
        const response = await fetch(`${baseUrl}/sessions`, {
          method: "POST",
          headers: {
            "content-type": "application/json"
          },
          body: JSON.stringify({
            name: name.trim(),
            project: project.trim() === "" ? null : project.trim(),
            host_id: defaultHostId,
            config_json: configJson,
            cron_schedule: cronSchedule.trim() === "" ? null : cronSchedule.trim(),
            auto_start: autoStart,
            github_repo: githubRepo.trim() === "" ? null : githubRepo.trim(),
            azure_project: azureProject.trim() === "" ? null : azureProject.trim()
          })
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
  return /*#__PURE__*/React.createElement("div", {
    style: {
      position: "fixed",
      inset: 0,
      background: "rgba(0,0,0,0.75)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      zIndex: 220
    },
    onClick: onClose
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: "min(860px, 95vw)",
      maxHeight: "90vh",
      overflowY: "auto",
      background: "#0d1117",
      border: "1px solid #1e2535",
      borderRadius: 10,
      padding: 18
    },
    onClick: event => event.stopPropagation()
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 12,
      color: "#94a3b8",
      marginBottom: 12
    }
  }, isEdit ? "Project Editor" : "New Project"), loading ? /*#__PURE__*/React.createElement("div", {
    style: {
      color: "#64748b",
      fontSize: 12,
      padding: "12px 0"
    }
  }, "Loading project definition...") : /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "Session Name"), /*#__PURE__*/React.createElement("input", {
    value: name,
    disabled: isEdit,
    onChange: event => {
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
    },
    style: {
      background: "#0a0e14",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "7px 10px"
    }
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "Project"), /*#__PURE__*/React.createElement("input", {
    value: project,
    onChange: event => setProject(event.target.value),
    style: {
      background: "#0a0e14",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "7px 10px"
    }
  })), /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "Cron Schedule"), /*#__PURE__*/React.createElement("input", {
    value: cronSchedule,
    onChange: event => setCronSchedule(event.target.value),
    placeholder: "optional",
    style: {
      background: "#0a0e14",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "7px 10px"
    }
  }))), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 10
    }
  }, /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "GitHub Repo"), /*#__PURE__*/React.createElement("input", {
    value: githubRepo,
    onChange: event => setGithubRepo(event.target.value),
    placeholder: "owner/repo",
    style: {
      background: "#0a0e14",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "7px 10px"
    }
  })), /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "Azure Project"), /*#__PURE__*/React.createElement("input", {
    value: azureProject,
    onChange: event => setAzureProject(event.target.value),
    style: {
      background: "#0a0e14",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "7px 10px"
    }
  }))), /*#__PURE__*/React.createElement("label", {
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: 8,
      fontSize: 11,
      color: "#94a3b8"
    }
  }, /*#__PURE__*/React.createElement("input", {
    type: "checkbox",
    checked: autoStart,
    onChange: event => setAutoStart(event.target.checked)
  }), "auto_start"), /*#__PURE__*/React.createElement("label", {
    style: {
      display: "grid",
      gap: 5
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "config_json"), /*#__PURE__*/React.createElement("textarea", {
    value: configText,
    onChange: event => setConfigText(event.target.value),
    rows: 12,
    style: {
      background: "#060810",
      border: "1px solid #1e2535",
      borderRadius: 5,
      color: "#cbd5e1",
      padding: "8px 10px",
      fontFamily: "inherit",
      fontSize: 11
    }
  })), errorMessage && /*#__PURE__*/React.createElement("div", {
    style: {
      color: "#f87171",
      fontSize: 11
    }
  }, errorMessage), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "flex-end",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: onClose,
    style: {
      border: "1px solid #1e2535",
      background: "transparent",
      color: "#94a3b8",
      borderRadius: 5,
      padding: "7px 10px",
      cursor: "pointer"
    }
  }, "Cancel"), /*#__PURE__*/React.createElement("button", {
    onClick: submit,
    disabled: saving || !name.trim(),
    style: {
      border: "none",
      background: "#2563eb",
      color: "#fff",
      borderRadius: 5,
      padding: "7px 10px",
      cursor: saving ? "default" : "pointer",
      opacity: saving ? 0.7 : 1
    }
  }, saving ? "Saving..." : isEdit ? "Save Project" : "Create Project")))));
}
function Dashboard() {
  const baseUrl = useMemo(() => daemonBaseUrl(), []);
  const [view, setView] = useState("grouped");
  const [statusFilter, setStatusFilter] = useState("all");
  const [projectFilter, setProjectFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [jumpTarget, setJumpTarget] = useState(null);
  const [editorTarget, setEditorTarget] = useState(null);
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
    const [sessionsResponse, hostsResponse, discoveryResponse] = await Promise.all([fetch(`${baseUrl}/sessions`), fetch(`${baseUrl}/hosts`), fetch(`${baseUrl}/discovery`)]);
    if (!sessionsResponse.ok) {
      throw new Error(`/sessions HTTP ${sessionsResponse.status}`);
    }
    if (!hostsResponse.ok) {
      throw new Error(`/hosts HTTP ${hostsResponse.status}`);
    }
    if (!discoveryResponse.ok) {
      throw new Error(`/discovery HTTP ${discoveryResponse.status}`);
    }
    const [sessionsBody, hostsBody, discoveryBody] = await Promise.all([sessionsResponse.json(), hostsResponse.json(), discoveryResponse.json()]);
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
  const runStartStop = async session => {
    const action = session.status === "stopped" ? "start" : "stop";
    setBusyBySession(prev => ({
      ...prev,
      [session.name]: true
    }));
    setActionMessage(null);
    try {
      const response = await fetch(`${baseUrl}/sessions/${encodeURIComponent(session.name)}/${action}`, {
        method: "POST"
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
      setBusyBySession(prev => {
        const next = {
          ...prev
        };
        delete next[session.name];
        return next;
      });
    }
  };
  const projects = useMemo(() => [...new Set(sessions.map(session => session.project).filter(Boolean))], [sessions]);
  const filtered = useMemo(() => {
    const searchText = search.trim().toLowerCase();
    return sessions.filter(session => {
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
  const runningCount = sessions.filter(session => session.status === "running").length;
  const idleCount = sessions.filter(session => session.status === "idle").length;
  const stoppedCount = sessions.filter(session => session.status === "stopped").length;
  const activeAgents = sessions.flatMap(session => session.panes).filter(pane => pane.status === "active").length;
  const openPrs = sessions.reduce((sum, session) => sum + session.openPrCount, 0);
  const defaultHostId = hosts.find(host => host.is_local)?.id || sessions[0]?.host_id || 1;
  return /*#__PURE__*/React.createElement("div", {
    style: {
      background: "#060810",
      minHeight: "100vh",
      color: "#e2e8f0",
      fontFamily: "'Berkeley Mono', 'Fira Code', 'JetBrains Mono', monospace"
    }
  }, /*#__PURE__*/React.createElement("style", null, `
        @keyframes ping { 75%, 100% { transform: scale(2.2); opacity: 0; } }
        * { box-sizing: border-box; margin: 0; padding: 0; }
        input::placeholder, textarea::placeholder { color: #1a2030; }
        button { font-family: inherit; }
      `), /*#__PURE__*/React.createElement("div", {
    style: {
      borderBottom: "1px solid #0a0e14",
      padding: "12px 24px",
      display: "flex",
      alignItems: "center",
      gap: 20,
      position: "sticky",
      top: 0,
      background: "#060810",
      zIndex: 10,
      flexWrap: "wrap"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      fontSize: 11,
      fontWeight: 800,
      color: "#475569",
      letterSpacing: "0.16em"
    }
  }, "TEAM CONTROL"), /*#__PURE__*/React.createElement("div", {
    style: {
      width: 1,
      height: 14,
      background: "#131820"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 14,
      fontSize: 11,
      flexWrap: "wrap"
    }
  }, /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#10b981"
    }
  }, runningCount), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#1e2535"
    }
  }, " run")), /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#f59e0b"
    }
  }, idleCount), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#1e2535"
    }
  }, " idle")), /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#94a3b8"
    }
  }, stoppedCount), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#1e2535"
    }
  }, " off")), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#131820"
    }
  }, "."), /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#10b981"
    }
  }, activeAgents), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#1e2535"
    }
  }, " agents")), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#131820"
    }
  }, "."), /*#__PURE__*/React.createElement("span", null, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#3b82f6"
    }
  }, openPrs), /*#__PURE__*/React.createElement("span", {
    style: {
      color: "#1e2535"
    }
  }, " PRs"))), /*#__PURE__*/React.createElement("div", {
    style: {
      marginLeft: "auto",
      display: "flex",
      alignItems: "center",
      gap: 8,
      flexWrap: "wrap"
    }
  }, /*#__PURE__*/React.createElement("button", {
    onClick: () => setEditorTarget({
      mode: "create"
    }),
    style: {
      border: "1px solid #1e2535",
      borderRadius: 5,
      background: "#102b1f",
      color: "#34d399",
      padding: "5px 10px",
      fontSize: 10,
      cursor: "pointer",
      letterSpacing: "0.05em"
    }
  }, "+ New Project"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "poll ", pollIntervalMs, "ms"), lastUpdated && /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 10,
      color: "#475569"
    }
  }, "updated ", relativeTime(lastUpdated.toISOString())), /*#__PURE__*/React.createElement("input", {
    value: search,
    onChange: event => setSearch(event.target.value),
    placeholder: "search sessions...",
    style: {
      background: "#0a0e14",
      border: "1px solid #131820",
      borderRadius: 5,
      padding: "5px 10px",
      fontSize: 11,
      color: "#94a3b8",
      outline: "none",
      width: 180
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      background: "#0a0e14",
      borderRadius: 5,
      border: "1px solid #131820",
      overflow: "hidden"
    }
  }, [["grouped", "Project"], ["grid", "Grid"], ["list", "List"], ["discovery", "Discovery"]].map(([value, label]) => /*#__PURE__*/React.createElement("button", {
    key: value,
    onClick: () => setView(value),
    style: {
      padding: "5px 12px",
      background: view === value ? "#1e2535" : "transparent",
      border: "none",
      color: view === value ? "#cbd5e1" : "#334155",
      cursor: "pointer",
      fontSize: 10,
      letterSpacing: "0.04em",
      transition: "background 0.1s"
    }
  }, label))))), view !== "discovery" && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "8px 24px",
      borderBottom: "1px solid #0a0e14",
      display: "flex",
      gap: 5,
      flexWrap: "wrap",
      alignItems: "center"
    }
  }, ["all", "running", "idle", "stopped"].map(filterValue => /*#__PURE__*/React.createElement("button", {
    key: filterValue,
    onClick: () => setStatusFilter(filterValue),
    style: {
      padding: "3px 8px",
      borderRadius: 4,
      fontSize: 10,
      cursor: "pointer",
      background: statusFilter === filterValue ? "#131820" : "transparent",
      border: statusFilter === filterValue ? "1px solid #1e2535" : "1px solid transparent",
      color: statusFilter === filterValue ? "#94a3b8" : "#1e2535",
      letterSpacing: "0.04em"
    }
  }, filterValue)), /*#__PURE__*/React.createElement("div", {
    style: {
      width: 1,
      height: 12,
      background: "#131820",
      margin: "0 4px"
    }
  }), /*#__PURE__*/React.createElement("button", {
    onClick: () => setProjectFilter("all"),
    style: {
      padding: "3px 8px",
      borderRadius: 4,
      fontSize: 10,
      cursor: "pointer",
      background: projectFilter === "all" ? "#131820" : "transparent",
      border: projectFilter === "all" ? "1px solid #1e2535" : "1px solid transparent",
      color: projectFilter === "all" ? "#94a3b8" : "#1e2535"
    }
  }, "all projects"), projects.map(project => {
    const pc = PROJECT_COLORS[project] || "#6b7280";
    return /*#__PURE__*/React.createElement("button", {
      key: project,
      onClick: () => setProjectFilter(project),
      style: {
        padding: "3px 8px",
        borderRadius: 4,
        fontSize: 10,
        cursor: "pointer",
        background: projectFilter === project ? `${pc}15` : "transparent",
        border: projectFilter === project ? `1px solid ${pc}40` : "1px solid transparent",
        color: projectFilter === project ? pc : "#1e2535"
      }
    }, project);
  })), errorMessage && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 24px",
      color: "#f87171",
      fontSize: 12
    }
  }, errorMessage), actionMessage && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "10px 24px",
      color: "#34d399",
      fontSize: 12
    }
  }, actionMessage), loading ? /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "30px 24px",
      color: "#64748b",
      fontSize: 12
    }
  }, "Loading dashboard data...") : /*#__PURE__*/React.createElement("div", {
    style: {
      paddingTop: 20
    }
  }, view === "grid" && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 24px 24px",
      display: "grid",
      gridTemplateColumns: "repeat(auto-fill, minmax(250px, 1fr))",
      gap: 10
    }
  }, filtered.map((session, index) => /*#__PURE__*/React.createElement(GridCard, {
    key: `${session.name}-${index}`,
    session: session,
    busy: Boolean(busyBySession[session.name]),
    onJump: setJumpTarget,
    onStartStop: runStartStop,
    onEdit: item => setEditorTarget({
      mode: "edit",
      session: item
    })
  }))), view === "list" && /*#__PURE__*/React.createElement(ListView, {
    sessions: filtered,
    busyBySession: busyBySession,
    onJump: setJumpTarget,
    onStartStop: runStartStop,
    onEdit: item => setEditorTarget({
      mode: "edit",
      session: item
    })
  }), view === "grouped" && /*#__PURE__*/React.createElement(GroupedView, {
    sessions: filtered,
    busyBySession: busyBySession,
    onJump: setJumpTarget,
    onStartStop: runStartStop,
    onEdit: item => setEditorTarget({
      mode: "edit",
      session: item
    })
  }), view === "discovery" && /*#__PURE__*/React.createElement(DiscoveryView, {
    rows: discoveryRows
  })), /*#__PURE__*/React.createElement(JumpModal, {
    baseUrl: baseUrl,
    defaultTerminal: defaultTerminal,
    session: jumpTarget,
    onClose: () => setJumpTarget(null)
  }), /*#__PURE__*/React.createElement(ProjectEditorModal, {
    baseUrl: baseUrl,
    defaultHostId: defaultHostId,
    target: editorTarget,
    onClose: () => setEditorTarget(null),
    onSaved: async message => {
      setEditorTarget(null);
      setActionMessage(message);
      await refresh();
    }
  }), hosts.length === 0 && !loading && /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "0 24px 24px",
      color: "#475569",
      fontSize: 11
    }
  }, "No hosts reported by daemon."));
}

ReactDOM.createRoot(document.getElementById("root")).render(React.createElement(Dashboard));
