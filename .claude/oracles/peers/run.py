"""`peers` — which sessions are live in this project, with role and native idle/busy state (RB-22).

Reads the anchor's C1 registry and grades every record with the C1 § 1.4 ladder **through
`uldf.session`**. It does not import `registry.py` (C7 § 7.4 forbids a cross-directory import) and
it does not carry a second copy of the ladder — the whole reason the ladder was lifted into the
shared lib is that three directories need it and none may import another.

**Liveness is never cached** (RB-22). There is no `ttl` field in the manifest and none here: every
batch re-grades every record, because a cached liveness verdict is a claim that was true once and
is asserted afterwards, which is the defect class this oracle replaces.

`unknown` is a first-class answer, not a failure. In a batch there is no `--native-live` (the
model's `ListAgents` result), so step 0 is unreachable and a session with no native file grades
`unknown`. That is the honest reading, and since `unknown` authorises nothing destructive, an
under-report is safe where an over-report would not be.
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, jsonio, oracle, paths, session

SCHEMA = "sessions/2"


def _records(anchor: pathlib.Path) -> dict:
    """The C1 store, schema-checked. Raises on the pre-rebuild shape (C1 § 1.1)."""
    store = jsonio.read(paths.collab_dir(anchor) / "active-sessions.json", None)
    if store is None:
        return {}
    if not isinstance(store, dict) or store.get("schema") != SCHEMA:
        raise Error(f"not a {SCHEMA} registry (schema={store.get('schema') if isinstance(store, dict) else type(store).__name__})")
    sessions = store.get("sessions")
    if not isinstance(sessions, dict):
        raise Error("registry `sessions` is not an object")
    return sessions


def run(ctx) -> dict:
    """C2 § 2.4. `pass` when it answered; `unknown` when the registry could not be read."""
    native_live = None
    raw = ctx.args.get("native-live") if getattr(ctx, "args", None) else None
    if raw:
        native_live = [x.strip() for x in str(raw).split(",") if x.strip()]

    try:
        records = _records(pathlib.Path(ctx.anchor_root))
    except Error as exc:
        return oracle.result("unknown", "the session registry could not be read",
                             reason=str(exc))

    lines: list[str] = []
    counts = {"alive": 0, "dead": 0, "unknown": 0}
    data_rows = []
    for session_id, record in sorted(records.items()):
        live = session.liveness(record, native_live)
        counts[live] = counts.get(live, 0) + 1
        # The one sanctioned read of `status`, and only for a record already graded alive.
        status = session.native_status(record, live)
        who = record.get("agentId") or session_id
        role = record.get("role") or "?"
        lines.append(f"{role} {who} {live} {status or '-'}")
        data_rows.append({"id": session_id, "role": role, "agentId": record.get("agentId"),
                          "siblingGroup": record.get("siblingGroup"), "liveness": live,
                          "status": status})

    if not records:
        summary = "no sessions registered in this project"
    else:
        parts = [f"{counts[k]} {k}" for k in ("alive", "dead", "unknown") if counts.get(k)]
        summary = f"{len(records)} session(s): " + ", ".join(parts)
        if lines:
            summary += " — " + "; ".join(lines[:3])
    return oracle.result("pass", summary[:200],
                         data={"sessions": data_rows, "counts": counts,
                               "native_live_supplied": native_live is not None})


if __name__ == "__main__":
    raise SystemExit(oracle.main(run))
