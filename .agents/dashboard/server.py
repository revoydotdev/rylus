#!/usr/bin/env python3
"""Project Skinner status dashboard — dependency-free LAN webui (read-only).

Projects the live repo state (ROADMAP.md milestone/phase/stage/todo tree,
.agents/ledger.jsonl verified-done set, .agents/STATE.md control + audit
verdicts, and git history) as JSON at /api/state, and serves the UI at /.
No external deps; mirrors revoy-spec's stdlib-webui posture.
"""
import json
import os
import re
import subprocess
import time
import http.server
import socketserver
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent  # .agents/dashboard -> .agents -> repo
ROADMAP = REPO / "ROADMAP.md"
LEDGER = REPO / ".agents" / "ledger.jsonl"
STATE = REPO / ".agents" / "STATE.md"
INDEX = HERE / "index.html"
PORT = int(os.environ.get("SKINNER_DASH_PORT", "8770"))

MS_RE = re.compile(r"^# (M\d+) — (.+?)\s*$", re.M)
PHASE_RE = re.compile(r"^## (M\d+\.P\d+) — (.+?)\s*$")
STAGE_RE = re.compile(r"^### (M\d+\.P\d+\.S\d+) — (.+?)\s*$")
TODO_RE = re.compile(r"^- \*\*`(M\d+\.P\d+\.S\d+\.T\d+)`\*\* — (.+?)\s*$")
GATE_RE = re.compile(r"^- \*\*(M\d+G\d+)\*\* — (.+?)\s*$", re.M)
TODO_ID = re.compile(r"M\d+\.P\d+\.S\d+\.T\d+")


def _read(p):
    try:
        return p.read_text(encoding="utf-8")
    except Exception:
        return ""


def _clip(s, n=140):
    s = re.sub(r"\s+", " ", s).strip()
    # cut at the artifact/concern markers if present
    for sep in (" → *Artifact", " · *Concern", ". →"):
        if sep in s:
            s = s.split(sep)[0]
    return s[:n]


def done_set():
    out = set()
    for line in _read(LEDGER).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            e = json.loads(line)
        except Exception:
            continue
        if e.get("type") == "done" and e.get("todo"):
            out.add(e["todo"])
    return out


def control():
    txt = _read(STATE)
    cm = re.search(r"CURRENT_MILESTONE:\s*(M\d+)", txt)
    ph = re.search(r"MILESTONE_PHASE:\s*(\w+)", txt)
    return (cm.group(1) if cm else "M1"), (ph.group(1) if ph else "NORMAL")


def last_audit(txt):
    ms = re.search(r"##\s*(M\d+)\s+AUDIT[^\n]*VERDICT:\s*(PASS|FAIL)", txt)
    if ms:
        return {"milestone": ms.group(1), "verdict": ms.group(2)}
    m = re.search(r"VERDICT:\s*(PASS|FAIL)", txt)
    return {"milestone": None, "verdict": m.group(1)} if m else None


def gate_status(txt, milestone):
    # newest wins (STATE.md keeps newest at top); accept the common verdict words.
    out = {}
    pat = re.compile(rf"({milestone}G\d+)\b[^\n]{{0,80}}?\b(PASS|FAIL|NOT MET|PENDING|MET)\b")
    for m in pat.finditer(txt):
        out.setdefault(m.group(1), m.group(2))
    return out


def parse_active(section, done):
    phases = []
    cur_phase = cur_stage = None
    for line in section.splitlines():
        pm = PHASE_RE.match(line)
        sm = STAGE_RE.match(line)
        tm = TODO_RE.match(line)
        if pm:
            cur_phase = {"id": pm.group(1), "title": pm.group(2), "stages": []}
            phases.append(cur_phase)
            cur_stage = None
        elif sm:
            if cur_phase is None:
                cur_phase = {"id": sm.group(1).rsplit(".", 1)[0], "title": "", "stages": []}
                phases.append(cur_phase)
            cur_stage = {"id": sm.group(1), "title": sm.group(2), "todos": []}
            cur_phase["stages"].append(cur_stage)
        elif tm:
            if cur_stage is None:
                if cur_phase is None:
                    cur_phase = {"id": tm.group(1).rsplit(".", 2)[0], "title": "", "stages": []}
                    phases.append(cur_phase)
                cur_stage = {"id": tm.group(1).rsplit(".", 1)[0], "title": "", "todos": []}
                cur_phase["stages"].append(cur_stage)
            cur_stage["todos"].append(
                {"id": tm.group(1), "desc": _clip(tm.group(2)), "done": tm.group(1) in done}
            )
    # rollups
    for ph in phases:
        for st in ph["stages"]:
            d = sum(1 for t in st["todos"] if t["done"])
            st["done"], st["total"] = d, len(st["todos"])
            st["status"] = "done" if st["total"] and d == st["total"] else ("active" if d else "pending")
        d = sum(s["done"] for s in ph["stages"])
        t = sum(s["total"] for s in ph["stages"])
        ph["done"], ph["total"] = d, t
        ph["status"] = "done" if t and d == t else ("active" if d else "pending")
    return phases


def commits(n=18):
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO), "log", f"-{n}", "--pretty=%h%x1f%s%x1f%cr"],
            capture_output=True, text=True, timeout=6,
        ).stdout
    except Exception:
        return []
    res = []
    for line in out.splitlines():
        parts = line.split("\x1f")
        if len(parts) == 3:
            h, s, rel = parts
            m = re.match(r"(feat|fix|chore|docs|test|refactor|ci|perf)", s)
            res.append({"hash": h, "subject": s, "rel": rel, "type": m.group(1) if m else "other"})
    return res


def tick_status():
    lock = REPO / ".agents" / "locks" / "RUN.lock"
    if lock.exists():
        try:
            age = int(time.time() - lock.stat().st_mtime)
        except Exception:
            age = None
        return {"active": True, "age_s": age}
    return {"active": False, "age_s": None}


def worktrees():
    # Active `concern/<tag>` worktrees == what workers are building right now.
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO), "worktree", "list", "--porcelain"],
            capture_output=True, text=True, timeout=5,
        ).stdout
    except Exception:
        return []
    concerns = []
    for line in out.splitlines():
        if line.startswith("branch ") and "concern/" in line:
            concerns.append(line.split("concern/", 1)[1].strip())
    return concerns


def recent_done(n=8):
    evs = []
    for line in _read(LEDGER).splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            e = json.loads(line)
        except Exception:
            continue
        if e.get("type") == "done":
            evs.append(e)
    evs = [e for e in evs if e.get("source") != "backfill"]  # live completions only
    evs.sort(key=lambda e: e.get("ts", ""))
    out = []
    for e in evs[-n:][::-1]:
        out.append({
            "todo": e.get("todo"), "concern": e.get("concern", ""),
            "ts": e.get("ts"), "commit": e.get("commit", "")[:8],
            "cmd": (e.get("verified_by") or {}).get("cmd", ""),
        })
    return out


def log_tail(n=14):
    txt = _read(REPO / ".agents" / "tick.log")
    lines = [l for l in txt.splitlines() if l.strip()]
    return lines[-n:]


def build_state():
    road = _read(ROADMAP)
    st_txt = _read(STATE)
    done = done_set()
    cur, phase = control()
    heads = list(MS_RE.finditer(road))
    ids = [h.group(1) for h in heads]
    titles = {h.group(1): h.group(2) for h in heads}
    sections = {}
    for i, h in enumerate(heads):
        end = heads[i + 1].start() if i + 1 < len(heads) else len(road)
        sections[h.group(1)] = road[h.end():end]
    cur_idx = ids.index(cur) if cur in ids else 0

    milestones = []
    for i, mid in enumerate(ids):
        sec = sections[mid]
        todos = [t for t in TODO_ID.findall(sec)]
        # only count todo IDs of this milestone (avoid gate cross-refs to other Ms)
        todos = [t for t in set(todos) if t.startswith(mid + ".")]
        d = sum(1 for t in todos if t in done)
        status = "done" if i < cur_idx else ("active" if i == cur_idx else "pending")
        milestones.append({
            "id": mid, "title": titles[mid], "status": status,
            "todos_done": d, "todos_total": len(todos),
            "is_last": i == len(ids) - 1,
        })

    phase_detail = {"NORMAL": "building", "AUDIT": "auditing", "REMEDIATION": "remediation"}.get(phase, phase.lower())
    active = None
    if cur in sections:
        active = {
            "id": cur, "title": titles.get(cur, ""), "phase": phase, "phase_detail": phase_detail,
            "phases": parse_active(sections[cur], done),
            "gates": [
                {"id": g, "desc": _clip(d, 120), "status": gate_status(st_txt, cur).get(g, "unknown")}
                for g, d in GATE_RE.findall(sections[cur])
            ],
        }

    return {
        "current_milestone": cur,
        "phase": phase,
        "phase_detail": phase_detail,
        "last_audit": last_audit(st_txt),
        "milestones": milestones,
        "active": active,
        "commits": commits(),
        "live": {
            "tick": tick_status(),
            "working": worktrees(),
            "recent_done": recent_done(),
            "log_tail": log_tail(),
        },
        "totals": {
            "done": len(done),
            "milestones": len(ids),
            "current_index": cur_idx + 1,
        },
    }


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _send(self, code, body, ctype):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body if isinstance(body, bytes) else body.encode("utf-8"))

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        try:
            if path == "/api/state":
                self._send(200, json.dumps(build_state()), "application/json")
            elif path in ("/", "/index.html"):
                self._send(200, _read(INDEX) or "<h1>index.html missing</h1>", "text/html; charset=utf-8")
            elif path == "/healthz":
                self._send(200, "ok", "text/plain")
            else:
                self._send(404, "not found", "text/plain")
        except Exception as e:  # never crash the server on a bad read
            self._send(500, json.dumps({"error": str(e)}), "application/json")


class DashServer(socketserver.ThreadingTCPServer):
    # Set SO_REUSEADDR on the CLASS so it applies before bind() in __init__ —
    # otherwise a restart within TIME_WAIT of served requests fails with EADDRINUSE.
    allow_reuse_address = True
    daemon_threads = True


def main():
    with DashServer(("0.0.0.0", PORT), Handler) as srv:
        print(f"skinner-dashboard serving on 0.0.0.0:{PORT} (repo {REPO})", flush=True)
        srv.serve_forever()


if __name__ == "__main__":
    main()
