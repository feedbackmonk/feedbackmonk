"""`peers` self-test — and **M1's regression harness** (gate G1).

RBD-M1 measured two things about the harness's own surfaces: the native listing drops a killed
session within ≤ 14 s, and (RBD-N1) `~/.claude/sessions/<pid>.json` does **not** — it outlives the
process by 156–193 s carrying a `status` frozen at `idle`. The whole liveness design rests on those
two facts. This file is where that dependency is re-checked, so that a change to the ladder which
quietly re-introduces the false-liveness reading fails here rather than in a live PODS session six
weeks later.

Every cell builds a sandbox anchor plus a **fake** native sessions directory (`HOME` and
`USERPROFILE` overridden) and a **real** child process, so the propositions are exercised against a
process the OS actually knows about while nothing touches the machine's own state.

The six M1 propositions, one cell each:

  1. alive        — a live pid whose `procStart` matches → `alive`
  2. dead         — the same pid after the process is killed → `dead`
  3. pid reuse    — a live pid whose `procStart` does NOT match → `dead`, not `alive`
  4. no stamp     — a live pid with no comparable start time → `unknown`, never `dead`
  5. absence      — an id absent from `--native-live` falls through, never to `dead`
  6. status guard — `status` is not read for a non-alive record, however fresh the file looks
"""

from __future__ import annotations

import json
import os
import pathlib
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import cells, jsonio, oracle, proc
from uldf.cells import T, cell

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import run as peers  # noqa: E402


def _sleeper() -> subprocess.Popen:
    return subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _wait_dead(pid: int, timeout_s: float = 10.0) -> str:
    end = time.monotonic() + timeout_s
    while time.monotonic() < end:
        if proc.pid_alive(pid) == "dead":
            return "dead"
        time.sleep(0.2)
    return proc.pid_alive(pid)


def _sandbox(t: T):
    """`(anchor, native sessions dir)`; HOME redirected so nothing reads the machine's own."""
    anchor = t.tmpdir()
    home = t.tmpdir()
    os.environ["HOME"] = str(home)
    os.environ["USERPROFILE"] = str(home)
    return anchor, home / ".claude" / "sessions"


def _record(anchor: pathlib.Path, session_id: str, **over) -> None:
    store = jsonio.read(anchor / ".claude" / "collaboration" / "active-sessions.json",
                        {"schema": "sessions/2", "sessions": {}})
    record = {"id": session_id, "role": "worker", "siblingGroup": "collab-x",
              "agentId": session_id.upper(), "worktree": None, "assignment": None,
              "arcDriver": False, "pid": None, "procStart": None,
              "startedAt": "2026-09-06T18:00:00Z"}
    record.update(over)
    store["sessions"][session_id] = record
    jsonio.write_atomic(anchor / ".claude" / "collaboration" / "active-sessions.json", store)


def _native(directory: pathlib.Path, pid: int, session_id: str, proc_start, status="idle") -> None:
    directory.mkdir(parents=True, exist_ok=True)
    record = {"pid": pid, "sessionId": session_id, "cwd": str(directory), "status": status,
              "statusUpdatedAt": "2026-09-06T18:00:00Z", "name": f"peer-{session_id}"}
    if proc_start is not None:
        record["procStart"] = proc_start
    (directory / f"{pid}.json").write_text(json.dumps(record), encoding="utf-8")


def _ctx(anchor: pathlib.Path, **args):
    return oracle.Context(project_root=anchor, anchor_root=anchor, home=anchor,
                          session_id=None, budget_ms=1000, args=args)


def _row(result: dict, agent_id: str) -> dict:
    for row in result.get("data", {}).get("sessions", []):
        if row["agentId"] == agent_id:
            return row
    return {}


# --- M1 propositions 1 and 2: alive, then dead ------------------------------------------------


@cell(red="no peers oracle; and while pid_start_time returned None a live session read unknown")
def cell_m1_live_session_reads_alive(t: T) -> None:
    anchor, sessions = _sandbox(t)
    child = _sleeper()
    try:
        _record(anchor, "sess-a", pid=child.pid)
        _native(sessions, child.pid, "sess-a", proc.pid_start_time(child.pid), status="busy")
        result = peers.run(_ctx(anchor))
        t.eq(result["verdict"], "pass", f"the oracle did not answer: {result}")
        t.eq(_row(result, "SESS-A")["liveness"], "alive", "a live corroborated session is not alive")
        t.eq(_row(result, "SESS-A")["status"], "busy", "status was withheld from a live session")
    finally:
        child.kill()
        child.wait(timeout=10)


@cell(red="a killed session read alive from the native file, which outlives its process by 156-193 s")
def cell_m1_killed_session_reads_dead(t: T) -> None:
    anchor, sessions = _sandbox(t)
    child = _sleeper()
    start = proc.pid_start_time(child.pid)
    child.kill()
    child.wait(timeout=10)
    t.eq(_wait_dead(child.pid), "dead", "the killed child never probed dead")

    # The file is STILL THERE, exactly as RBD-N1 measured, and still says `idle`.
    _record(anchor, "sess-a", pid=child.pid)
    _native(sessions, child.pid, "sess-a", start, status="idle")
    result = peers.run(_ctx(anchor))
    t.eq(_row(result, "SESS-A")["liveness"], "dead",
         "a dead session read as alive from a file that outlives the process — the false-liveness "
         "class this oracle exists to close")
    t.ok(_row(result, "SESS-A")["status"] is None,
         "the stale `idle` was read for a dead session (M1 proposition 6)")


# --- M1 proposition 3: pid reuse ---------------------------------------------------------------


@cell(red="a reused pid reads as the original session; nothing compared procStart, the discriminator")
def cell_m1_pid_reuse_reads_dead(t: T) -> None:
    anchor, sessions = _sandbox(t)
    child = _sleeper()
    try:
        real = proc.pid_start_time(child.pid)
        t.ok(real is not None, "no start time for a live child; this cell would be vacuous")
        # The pid is ALIVE, but it is not the process the record was written about.
        _record(anchor, "sess-a", pid=child.pid)
        _native(sessions, child.pid, "sess-a", str(int(real or 0) - 60_000_000_000))
        result = peers.run(_ctx(anchor))
        t.eq(_row(result, "SESS-A")["liveness"], "dead",
             "a live pid whose start time disagrees is a DIFFERENT process")
    finally:
        child.kill()
        child.wait(timeout=10)


# --- M1 proposition 4: no start time is `unknown`, never `dead` -------------------------------


@cell(red="an unavailable start time answered dead, which authorises a sweep of a live session")
def cell_m1_no_start_time_reads_unknown(t: T) -> None:
    anchor, sessions = _sandbox(t)
    child = _sleeper()
    try:
        _record(anchor, "sess-a", pid=child.pid)
        _native(sessions, child.pid, "sess-a", None)      # a file with no procStart at all
        result = peers.run(_ctx(anchor))
        t.eq(_row(result, "SESS-A")["liveness"], "unknown",
             "a live pid with nothing to compare against must be unknown, never dead")
        t.ok(_row(result, "SESS-A")["status"] is None, "status was read for an `unknown` record")
    finally:
        child.kill()
        child.wait(timeout=10)


# --- M1 proposition 5: absence from the listing is not death ----------------------------------


@cell(red="absence from the ListAgents snapshot read as death; the script cannot prove it fresh")
def cell_m1_absence_from_native_live_is_not_death(t: T) -> None:
    anchor, sessions = _sandbox(t)
    child = _sleeper()
    try:
        _record(anchor, "sess-a", pid=child.pid)
        _native(sessions, child.pid, "sess-a", proc.pid_start_time(child.pid))
        result = peers.run(_ctx(anchor, **{"native-live": "someone-else,and-another"}))
        t.eq(_row(result, "SESS-A")["liveness"], "alive",
             "an id absent from --native-live must fall through to step 1, not to dead")
        t.eq(result["data"]["native_live_supplied"], True, "the oracle did not report that it "
                                                           "was given a listing")

        # And a batch supplies none at all — step 0 unreachable, everything else unchanged.
        batch = peers.run(_ctx(anchor))
        t.eq(batch["data"]["native_live_supplied"], False,
             "a batch run claimed a listing it was never given")
    finally:
        child.kill()
        child.wait(timeout=10)


# --- the oracle's own contract ------------------------------------------------------------------


@cell(red="the pre-rebuild registry read as an empty project: 0 sessions for a repo full of live ones")
def cell_the_old_registry_shape_is_unknown_not_empty(t: T) -> None:
    anchor, _ = _sandbox(t)
    jsonio.write_atomic(anchor / ".claude" / "collaboration" / "active-sessions.json",
                        {"sessions": [{"sessionId": "old-1", "status": "active"}]})
    result = peers.run(_ctx(anchor))
    t.eq(result["verdict"], "unknown",
         "the pre-rebuild shape was read as data instead of refused")
    t.ok(result.get("reason"), "an `unknown` verdict carries no reason (C2 § 2.4)")

    # An ABSENT registry is a legitimate empty answer, not an unknown.
    (anchor / ".claude" / "collaboration" / "active-sessions.json").unlink()
    empty = peers.run(_ctx(anchor))
    t.eq(empty["verdict"], "pass", "a project with no registry yet must answer, not fail")
    t.ok("no sessions" in empty["summary"], f"summary was {empty['summary']!r}")


@cell(red="RB-22 reserves 1,000 ms and nothing measured it; the pid probe forked at ~470 ms per record")
def cell_within_the_batch_reservation(t: T) -> None:
    import json as _json
    manifest = _json.loads((HERE / "oracle.json").read_text(encoding="utf-8"))
    t.ok(manifest["expected_runtime_ms"] <= 1000,
         f"expected_runtime_ms is {manifest['expected_runtime_ms']}, reservation is 1000 (RB-22)")
    t.ok(manifest["assertion"]["known_gaps"], "a manifest with no declared gaps (OVALID)")

    anchor, sessions = _sandbox(t)
    child = _sleeper()
    try:
        for i in range(8):
            _record(anchor, f"sess-{i}", pid=child.pid)
            _native(sessions, child.pid + i, f"sess-{i}", proc.pid_start_time(child.pid))
        started = time.monotonic()
        result = peers.run(_ctx(anchor))
        ms = int((time.monotonic() - started) * 1000)
        t.eq(result["verdict"], "pass", "the oracle did not answer over 8 records")
        t.ok(ms <= manifest["expected_runtime_ms"],
             f"8 records took {ms} ms against a declared {manifest['expected_runtime_ms']} ms")
        t.note(f"8 records graded in {ms} ms")
    finally:
        child.kill()
        child.wait(timeout=10)


@cell(red="the ladder copied into the oracle: a fix in uldf.session does not reach it (C7 7.4)")
def cell_no_second_copy_of_the_ladder(t: T) -> None:
    source = (HERE / "run.py").read_text(encoding="utf-8")
    t.ok("import registry" not in source,
         "peers imports registry.py — a cross-directory import C7 § 7.4 forbids")
    for banned in ("pid_alive(", "pid_start_time(", "native_sessions_dir("):
        t.ok(banned not in source, f"peers re-implements the ladder: found {banned!r}")
    t.ok("session.liveness" in source and "session.native_status" in source,
         "peers does not reach the ladder through uldf.session")


# --- env hygiene: no cell leaks HOME to the next one (C6 § 6.1: no shared fixtures) ------------
#
# Several cells here redirect `HOME`/`USERPROFILE` at a `t.tmpdir()` so the liveness ladder reads a
# FAKE `~/.claude/sessions` instead of this machine's own (GUIDE § 11 rule 8). `os.environ` is
# process-global and `t.tmpdir()` is deleted when the cell ends, so a cell that redirected and did
# not restore leaves every LATER cell — in this module and in every other lane's module, since the
# runner is one process — pointing at a directory that no longer exists.
#
# Measured, and this is why the block exists: in the first whole-suite run
# `cell_binding_note_1_a_fresh_worktree_spawn_starts_and_acts` failed with `~/.claude.json`
# "unreadable" and a launched `claude` exiting rc=1 (no credentials under a dead HOME), and
# `cell_pid_start_time_matches_the_harness_unit` SKIPPED claiming the machine has no
# `~/.claude/sessions`. Both had passed alone minutes earlier. An order-dependent suite that is
# green when you run one file is worse than a red one.
#
# Restoring inside each cell would work and would have to be remembered by every future author of
# a cell in this file. Wrapping here cannot be forgotten.
def _env_safe(fn):
    def wrapper(t):
        saved = {key: os.environ.get(key) for key in ("HOME", "USERPROFILE")}
        try:
            return fn(t)
        finally:
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
    for attribute in ("_uldf_cell", "_uldf_red", "_uldf_timeout_ms", "__name__", "__doc__"):
        if hasattr(fn, attribute):
            setattr(wrapper, attribute, getattr(fn, attribute))
    return wrapper


for _cell_name, _cell_fn in list(globals().items()):
    if _cell_name.startswith("cell_") and getattr(_cell_fn, "_uldf_cell", False):
        globals()[_cell_name] = _env_safe(_cell_fn)


if __name__ == "__main__":
    raise SystemExit(cells.main())
