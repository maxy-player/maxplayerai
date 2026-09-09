#!/usr/bin/env python3
"""muse-acp-bridge — run Maxplayer seller jobs on a scheduled Muse worker.

`maxplayer seller` spawns ONE agent process per job and talks Agent Client Protocol
(ACP) to it over stdio. A Muse account cannot be that process: its model turns are
driven by a scheduler, not by a pipe. This bridge is the adapter. It speaks the slice
of ACP the seller's driver uses, publishes each turn as a job in a queue directory,
and blocks until a Muse worker run claims that job and reports it done.

    seller driver  --ACP/stdio-->  this bridge  --queue dir-->  Muse worker run

Two roles, one file:

  serve                       ACP bridge on stdin/stdout (what --agent-argv runs)
  claim [--queue DIR]         worker side: atomically take the oldest ready job
  done --job DIR --status ok|error [--summary TEXT]
  reap [--queue DIR]          release claims abandoned by a killed/restarted worker
  selfcheck                   offline invariant check, no seller and no relay needed

What this bridge REFUSES to do, and why each refusal is load-bearing:

* No `os.getcwd()` fallback for the job workdir. The seller spawns the agent child
  with the SELLER's cwd, never the job workdir, so a fallback silently writes
  deliverables into the wrong tree and the delivery ships empty. Missing or
  unusable `cwd` in `session/new` fails the session instead.
* The pre-advertise self-probe is NOT answered inline. Writing probe.txt from this
  process would prove only that this process can write a file — the seat would
  advertise while the worker path was dead. The probe is queued like any other job,
  so a seat advertises only after the real worker has produced a real artifact.
* A `done` file is honoured only when it names the turn that is waiting. A done left
  behind by an earlier turn, or by a late worker run after this turn gave up, is
  never reused as this turn's answer.
* Every state transition is atomic: jobs appear by directory rename, claims are
  exclusive `mkdir`, `done` lands by `os.replace`. A worker run can be killed at any
  point without leaving a half-published job or a half-written result.

Stop reasons on the wire are what the seller's ACP driver actually accepts
(`stop_reason_from_params`): `completed`/`end_turn`, `cancelled`/`canceled`,
`failed`. Anything it does not recognise — including a missing reason — is read as
`failed`, so this bridge always states one explicitly.

No account identity, seat name, wallet value or absolute home path is baked in.
Queue location: $MAXPLAYER_MUSE_QUEUE, else $XDG_STATE_HOME/maxplayer-muse/queue,
else ~/.local/state/maxplayer-muse/queue.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import sys
import tempfile
import threading
import time
import uuid

# --- Budgets -----------------------------------------------------------------
# Defaults suit a worker on a two-minute schedule. Each is overridable by
# environment variable for a different scheduler cadence and for the offline test
# suite, which cannot wait out a nine-minute turn to prove what happens after one.
def _budget(name: str, default: float, floor: float) -> float:
    raw = os.environ.get(name)
    if not raw:
        return default
    try:
        value = float(raw)
    except ValueError:
        return default
    return value if value >= floor else default


# The seller driver drops a turn after its idle timeout of silence on the wire, so
# the keep-alive cadence must stay well under it; everything else is bounded by it.
KEEPALIVE_SECS = _budget("MAXPLAYER_MUSE_KEEPALIVE_SECS", 30.0, 0.1)
# One ACP turn. Longer than a worker run budget plus a scheduler interval, so a job
# that lands just after a run started still gets a full run before the turn expires.
TURN_BUDGET_SECS = _budget("MAXPLAYER_MUSE_TURN_BUDGET_SECS", 540.0, 0.5)
# Advisory: what a Muse worker run gets before its own scheduler stops it. Recorded
# in the job so the worker can refuse work it cannot finish rather than half-do it.
WORKER_RUN_BUDGET_SECS = _budget("MAXPLAYER_MUSE_WORKER_BUDGET_SECS", 480.0, 0.5)
# A claim older than this with no `done` is treated as abandoned by `reap`. Must
# exceed WORKER_RUN_BUDGET_SECS plus scheduler slop, which is minutes, not seconds.
CLAIM_TTL_SECS = _budget("MAXPLAYER_MUSE_CLAIM_TTL_SECS", 900.0, 0.5)
POLL_SECS = _budget("MAXPLAYER_MUSE_POLL_SECS", 2.0, 0.02)

PROBE_SENTINEL_RE = re.compile(r"maxplayer-probe-[A-Za-z0-9._-]+")


# --- Queue layout ------------------------------------------------------------
def queue_root(explicit: str | None = None) -> str:
    if explicit:
        return os.path.abspath(os.path.expanduser(explicit))
    from_env = os.environ.get("MAXPLAYER_MUSE_QUEUE")
    if from_env:
        return os.path.abspath(os.path.expanduser(from_env))
    state = os.environ.get("XDG_STATE_HOME") or os.path.join(
        os.path.expanduser("~"), ".local", "state"
    )
    return os.path.join(state, "maxplayer-muse", "queue")


def jobs_dir(root: str) -> str:
    return os.path.join(root, "jobs")


def read_json(path: str):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError):
        return None


def write_json_atomic(path: str, payload) -> None:
    """Write JSON so a reader sees either the old file or the whole new one."""
    directory = os.path.dirname(path) or "."
    handle, tmp = tempfile.mkstemp(dir=directory, prefix=".tmp-", suffix=".json")
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as out:
            json.dump(payload, out)
            out.flush()
            os.fsync(out.fileno())
        os.replace(tmp, path)
    except BaseException:
        _quiet_unlink(tmp)
        raise


def _quiet_unlink(path: str) -> None:
    try:
        os.unlink(path)
    except OSError:
        pass


def publish_job(root: str, *, turn_id: str, session_id: str, workdir: str,
                task: str, kind: str, now: float) -> str:
    """Stage a job out of sight, then make it visible with one atomic rename.

    A worker that lists the jobs directory sees only complete jobs: task text,
    metadata and all. There is no window in which a job exists but its task does
    not, so a worker never claims a job it cannot read.
    """
    staging = os.path.join(root, ".staging")
    os.makedirs(staging, exist_ok=True)
    os.makedirs(jobs_dir(root), exist_ok=True)
    build = tempfile.mkdtemp(dir=staging, prefix="build-")
    with open(os.path.join(build, "task.md"), "w", encoding="utf-8") as out:
        out.write(task)
    write_json_atomic(
        os.path.join(build, "meta.json"),
        {
            "turn_id": turn_id,
            "session_id": session_id,
            "workdir": workdir,
            "kind": kind,
            "created_at": now,
            "deadline_at": now + TURN_BUDGET_SECS,
            "worker_run_budget_secs": WORKER_RUN_BUDGET_SECS,
        },
    )
    final = os.path.join(jobs_dir(root), turn_id)
    os.rename(build, final)
    return final


def claim_job(root: str, *, now: float | None = None) -> dict | None:
    """Take the oldest ready job, exclusively. Returns the claim or None.

    Ready means: published, not cancelled, not already done, not past its deadline,
    and not already claimed. `os.mkdir` is the exclusive primitive — two concurrent
    worker runs racing for the same job cannot both succeed, so a job is executed at
    most once even though the scheduler documents no single-flight guarantee.
    """
    now = time.time() if now is None else now
    directory = jobs_dir(root)
    try:
        entries = sorted(os.listdir(directory))
    except FileNotFoundError:
        return None

    candidates = []
    for name in entries:
        job_dir = os.path.join(directory, name)
        meta = read_json(os.path.join(job_dir, "meta.json"))
        if not isinstance(meta, dict):
            continue
        if os.path.exists(os.path.join(job_dir, "done")):
            continue
        if os.path.exists(os.path.join(job_dir, "cancel")):
            continue
        if float(meta.get("deadline_at", 0)) <= now:
            continue
        if os.path.isdir(os.path.join(job_dir, "claim")):
            continue
        candidates.append((float(meta.get("created_at", 0)), job_dir, meta))

    candidates.sort(key=lambda item: (item[0], item[1]))
    for _, job_dir, meta in candidates:
        claim_dir = os.path.join(job_dir, "claim")
        try:
            os.mkdir(claim_dir)
        except FileExistsError:
            continue  # lost the race; try the next job
        except OSError:
            continue
        claim = {
            "claim_token": uuid.uuid4().hex,
            "claimed_at": time.time(),
            "pid": os.getpid(),
        }
        write_json_atomic(os.path.join(claim_dir, "claim.json"), claim)
        return {
            "job_dir": job_dir,
            "task_file": os.path.join(job_dir, "task.md"),
            "workdir": meta.get("workdir"),
            "turn_id": meta.get("turn_id"),
            "kind": meta.get("kind"),
            "deadline_at": meta.get("deadline_at"),
            "worker_run_budget_secs": meta.get("worker_run_budget_secs"),
            "claim_token": claim["claim_token"],
        }
    return None


def write_done(job_dir: str, *, status: str, summary: str) -> dict:
    """Record this job's outcome atomically, stamped with the turn it belongs to."""
    if status not in ("ok", "error"):
        raise ValueError("status must be ok or error")
    meta = read_json(os.path.join(job_dir, "meta.json"))
    if not isinstance(meta, dict):
        raise FileNotFoundError(f"no readable meta.json in {job_dir}")
    payload = {
        "turn_id": meta.get("turn_id"),
        "status": status,
        "summary": summary,
        "finished_at": time.time(),
    }
    write_json_atomic(os.path.join(job_dir, "done"), payload)
    return payload


def reap_stale_claims(root: str, *, now: float | None = None,
                      ttl: float = CLAIM_TTL_SECS) -> list[str]:
    """Release claims from worker runs that died mid-job.

    Only claims older than the TTL, on jobs that are still live and undone, are
    released. Nothing else is touched: pending jobs, finished results and the
    queue's configuration all survive. This is never a process kill.
    """
    now = time.time() if now is None else now
    released = []
    try:
        entries = sorted(os.listdir(jobs_dir(root)))
    except FileNotFoundError:
        return released
    for name in entries:
        job_dir = os.path.join(jobs_dir(root), name)
        claim_dir = os.path.join(job_dir, "claim")
        if not os.path.isdir(claim_dir):
            continue
        if os.path.exists(os.path.join(job_dir, "done")):
            continue
        meta = read_json(os.path.join(job_dir, "meta.json")) or {}
        if float(meta.get("deadline_at", 0)) <= now:
            continue  # expired jobs are not worth re-running
        claim = read_json(os.path.join(claim_dir, "claim.json")) or {}
        if now - float(claim.get("claimed_at", 0)) < ttl:
            continue
        shutil.rmtree(claim_dir, ignore_errors=True)
        if not os.path.exists(claim_dir):
            released.append(job_dir)
    return released


# --- ACP bridge --------------------------------------------------------------
class Bridge:
    def __init__(self, root: str, out=sys.stdout):
        self.root = root
        self.out = out
        self.out_lock = threading.Lock()
        self.sessions: dict[str, str] = {}      # session id -> workdir
        self.sessions_lock = threading.Lock()
        self.cancelled: set[str] = set()        # session ids cancelled by the client
        self.live_turns: dict[str, str] = {}    # session id -> job dir of the open turn

    # -- wire helpers
    def send(self, obj) -> None:
        line = json.dumps(obj, separators=(",", ":"))
        with self.out_lock:
            self.out.write(line + "\n")
            self.out.flush()

    def respond(self, msg_id, result) -> None:
        if msg_id is not None:
            self.send({"jsonrpc": "2.0", "id": msg_id, "result": result})

    def fail(self, msg_id, code: int, message: str) -> None:
        if msg_id is not None:
            self.send({"jsonrpc": "2.0", "id": msg_id,
                       "error": {"code": code, "message": message}})

    def update(self, session_id: str, text: str) -> None:
        self.send({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": text},
                },
            },
        })

    # -- methods
    def on_initialize(self, msg_id) -> None:
        self.respond(msg_id, {
            "protocolVersion": 2,
            "agentCapabilities": {
                "loadSession": False,
                "mcpCapabilities": {},
                "promptCapabilities": {
                    "image": False, "audio": False, "embeddedContext": False,
                },
            },
            "agentInfo": {"name": "muse-acp-bridge", "version": "1.0.0"},
        })

    def on_session_new(self, msg_id, params) -> None:
        cwd = (params or {}).get("cwd")
        # No fallback. An absent, relative, missing or unwritable workdir is a
        # broken session, and saying so here is the difference between a failed
        # job and a delivery that quietly contains nothing.
        if not isinstance(cwd, str) or not cwd:
            self.fail(msg_id, -32602,
                      "session/new requires an absolute cwd: this bridge never "
                      "guesses the job workdir")
            return
        if not os.path.isabs(cwd) or not os.path.isdir(cwd):
            self.fail(msg_id, -32602, f"session/new cwd is not an existing absolute directory: {cwd}")
            return
        if not os.access(cwd, os.W_OK):
            self.fail(msg_id, -32602, f"session/new cwd is not writable: {cwd}")
            return
        session_id = uuid.uuid4().hex
        with self.sessions_lock:
            self.sessions[session_id] = cwd
        self.respond(msg_id, {"sessionId": session_id})

    def on_session_prompt(self, msg_id, params) -> None:
        params = params or {}
        session_id = params.get("sessionId") or params.get("session_id") or ""
        with self.sessions_lock:
            workdir = self.sessions.get(session_id)
        # Per-turn identity check: a prompt for a session this process never opened
        # is refused rather than run against some other session's workdir.
        if workdir is None:
            self.fail(msg_id, -32602, f"unknown sessionId: {session_id!r}")
            return
        if session_id in self.cancelled:
            self.respond(msg_id, {"reason": "cancelled"})
            return

        text = prompt_text(params)
        kind = "probe" if is_probe_prompt(text) else "task"
        turn_id = uuid.uuid4().hex
        try:
            job_dir = publish_job(self.root, turn_id=turn_id, session_id=session_id,
                                  workdir=workdir, task=text, kind=kind, now=time.time())
        except OSError as error:
            # Cannot make the queue guarantee, so do not pretend the turn ran.
            self.update(session_id, f"queue unavailable: {error}")
            self.respond(msg_id, {"reason": "failed"})
            return
        with self.sessions_lock:
            self.live_turns[session_id] = job_dir

        self.update(session_id, "Task queued for the Muse worker.")
        status, summary = self.wait_for_done(job_dir, session_id, turn_id)
        with self.sessions_lock:
            self.live_turns.pop(session_id, None)

        if status == "ok":
            self.update(session_id, "Done. Deliverables are in the session workdir.\n" + summary[:2000])
            self.respond(msg_id, {"reason": "completed"})
        elif status == "cancelled":
            self.update(session_id, "Cancelled.")
            self.respond(msg_id, {"reason": "cancelled"})
        else:
            self.update(session_id, "Failed: " + summary[:2000])
            self.respond(msg_id, {"reason": "failed"})

    def wait_for_done(self, job_dir: str, session_id: str, turn_id: str) -> tuple[str, str]:
        done_path = os.path.join(job_dir, "done")
        deadline = time.time() + TURN_BUDGET_SECS
        last_beat = time.time()
        while time.time() < deadline:
            if session_id in self.cancelled:
                return "cancelled", "cancelled by the client"
            payload = read_json(done_path)
            if isinstance(payload, dict):
                # A done that names a different turn is somebody else's answer.
                if payload.get("turn_id") != turn_id:
                    return "failed", (
                        "stale done: the worker reported turn "
                        f"{payload.get('turn_id')!r}, this turn is {turn_id!r}"
                    )
                summary = str(payload.get("summary") or "")
                return ("ok" if payload.get("status") == "ok" else "failed"), summary
            now = time.time()
            if now - last_beat >= KEEPALIVE_SECS:
                last_beat = now
                self.update(session_id, "Muse worker still running.")
            time.sleep(POLL_SECS)
        # The turn is over for the driver. Tell the worker so a late run does not
        # keep spending on an answer nobody is waiting for.
        self.mark_expired(job_dir)
        return "failed", (
            f"turn budget of {int(TURN_BUDGET_SECS)}s expired with no worker result; "
            "check that the Muse worker schedule is enabled and its runs are firing"
        )

    def mark_expired(self, job_dir: str) -> None:
        try:
            with open(os.path.join(job_dir, "cancel"), "w", encoding="utf-8") as out:
                out.write("turn-expired\n")
        except OSError:
            pass

    def on_session_cancel(self, msg_id, params) -> None:
        params = params or {}
        session_id = params.get("sessionId") or params.get("session_id") or ""
        self.cancelled.add(session_id)
        with self.sessions_lock:
            job_dir = self.live_turns.get(session_id)
        if job_dir:
            # Propagate: the worker checks this marker and stops rather than
            # delivering into a workdir the buyer will never be shown.
            try:
                with open(os.path.join(job_dir, "cancel"), "w", encoding="utf-8") as out:
                    out.write("client-cancelled\n")
            except OSError:
                pass
        self.respond(msg_id, {})

    def handle(self, line: str) -> None:
        try:
            msg = json.loads(line)
        except ValueError:
            return
        if not isinstance(msg, dict):
            return
        method = msg.get("method")
        msg_id = msg.get("id")
        params = msg.get("params") or {}
        if method == "initialize":
            self.on_initialize(msg_id)
        elif method == "session/new":
            self.on_session_new(msg_id, params)
        elif method == "session/prompt":
            # Off-thread so session/cancel stays receivable during a long turn.
            threading.Thread(target=self.on_session_prompt,
                             args=(msg_id, params), daemon=True).start()
        elif method == "session/cancel":
            self.on_session_cancel(msg_id, params)
        elif msg_id is not None:
            self.fail(msg_id, -32601, f"unknown method: {method}")

    def serve(self, stream=sys.stdin) -> None:
        os.makedirs(jobs_dir(self.root), exist_ok=True)
        for line in stream:
            line = line.strip()
            if line:
                self.handle(line)


def prompt_text(params) -> str:
    chunks = []
    for block in (params.get("prompt") or []):
        if isinstance(block, dict):
            if block.get("type") == "text" or "text" in block:
                chunks.append(str(block.get("text", "")))
    return "\n".join(chunks)


def is_probe_prompt(text: str) -> bool:
    """The seller's pre-advertise self-probe, recognised but NOT shortcut.

    Recognising it only labels the job, so the worker can see it is a readiness
    probe and answer it first. The artifact is still written by the worker, in the
    workdir, which is the whole point of the gate.
    """
    return bool(PROBE_SENTINEL_RE.search(text)) and "probe.txt" in text


# --- selfcheck ---------------------------------------------------------------
def selfcheck() -> int:
    """Prove the queue invariants on a throwaway directory. No network, no seller."""
    root = tempfile.mkdtemp(prefix="muse-acp-selfcheck-")
    try:
        work = os.path.join(root, "work")
        os.makedirs(work)
        job = publish_job(root, turn_id="t1", session_id="s1", workdir=work,
                          task="do the thing", kind="task", now=time.time())
        assert os.path.isfile(os.path.join(job, "task.md")), "task published"
        first = claim_job(root)
        assert first and first["turn_id"] == "t1", "job is claimable once"
        assert claim_job(root) is None, "a claimed job is not claimable again"
        write_done(job, status="ok", summary="delivered")
        payload = read_json(os.path.join(job, "done"))
        assert payload["turn_id"] == "t1" and payload["status"] == "ok", "done is stamped"
        print("selfcheck ok")
        return 0
    except AssertionError as error:
        print(f"selfcheck FAILED: {error}", file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(root, ignore_errors=True)


# --- entry point -------------------------------------------------------------
def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--queue", default=None, help="queue directory (overrides the environment)")
    sub = parser.add_subparsers(dest="command")
    sub.add_parser("serve", help="ACP bridge on stdio (default)")
    sub.add_parser("claim", help="claim the oldest ready job; prints JSON or nothing")
    done = sub.add_parser("done", help="record a job outcome atomically")
    done.add_argument("--job", required=True)
    done.add_argument("--status", required=True, choices=["ok", "error"])
    done.add_argument("--summary", default="")
    sub.add_parser("reap", help="release claims abandoned by a dead worker run")
    sub.add_parser("selfcheck", help="offline invariant check")

    args = parser.parse_args(argv)
    root = queue_root(args.queue)
    command = args.command or "serve"

    if command == "serve":
        Bridge(root).serve()
        return 0
    if command == "claim":
        claim = claim_job(root)
        if claim is None:
            return 0  # nothing ready: silence is the whole answer
        print(json.dumps(claim))
        return 0
    if command == "done":
        write_done(args.job, status=args.status, summary=args.summary)
        return 0
    if command == "reap":
        for job_dir in reap_stale_claims(root):
            print(job_dir)
        return 0
    if command == "selfcheck":
        return selfcheck()
    parser.error(f"unknown command: {command}")
    return 2


if __name__ == "__main__":
    sys.exit(main())
