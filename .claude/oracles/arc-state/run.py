"""arc-state (C2, RB-21/22): the LTADS arc record (C5).

Absent file -> `pass` "no arc". An unrecognized schema major, or a shape C5 does not describe,
is `unknown` with a reason -- never a guess. `arc.status`, `driver_session`,
`consent.expires_at` and `progress` are rendered when a current arc exists (C5 Table E).
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, jsonio, oracle  # noqa: E402

_ARC_STATUSES = ("active", "paused", "concluded")


def run(ctx: oracle.Context) -> dict:
    path = ctx.project_root / "ltads" / "arc-state.json"
    if not path.is_file():
        return oracle.result("pass", "no arc-state.json (no arc)",
                              data={"exists": False, "status": None, "driver_session": None,
                                    "progress": None, "consent_expires_at": None,
                                    "arc_id": None})

    try:
        record = jsonio.read(path)
    except Error as exc:
        return oracle.result("unknown", "arc-state.json is unparsable", reason=str(exc))

    if not isinstance(record, dict):
        return oracle.result("unknown", "arc-state.json is not a JSON object",
                              reason=f"top-level value is {type(record).__name__}")

    schema = str(record.get("schema", ""))
    if not schema.startswith("arc-state/"):
        return oracle.result("unknown", f"unrecognized schema {schema!r}",
                              reason=f"expected 'arc-state/<n>', got {schema!r}")
    major = schema.split("/", 1)[1]
    if major != "2":
        return oracle.result("unknown", f"unsupported arc-state major version {major!r}",
                              reason="only arc-state/2 is understood")

    arc = record.get("arc")
    if arc is None:
        return oracle.result("pass", "no active arc",
                              data={"exists": True, "status": None, "driver_session": None,
                                    "progress": None, "consent_expires_at": None,
                                    "arc_id": None})

    if not isinstance(arc, dict) or arc.get("status") not in _ARC_STATUSES:
        return oracle.result("unknown", "the `arc` field does not match C5's shape",
                              reason=f"arc={arc!r}"[:200])

    consent = arc.get("consent")
    expires_at = consent.get("expires_at") if isinstance(consent, dict) else None
    data = {
        "exists": True,
        "status": arc.get("status"),
        "driver_session": arc.get("driver_session"),
        "progress": arc.get("progress"),
        "consent_expires_at": expires_at,
        "arc_id": arc.get("id"),
    }
    summary = f"arc {arc.get('id')} status={arc.get('status')}"
    return oracle.result("pass", summary[:200], data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
