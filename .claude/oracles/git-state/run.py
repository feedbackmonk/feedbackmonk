"""git-state (C2, RB-21/22): branch, dirty counts, head, and ahead/behind an upstream.

A handful of git invocations, all cheap: `rev-parse --is-inside-work-tree` for repo detection,
`symbolic-ref --short HEAD` for the branch name, `rev-parse HEAD` for the head sha, one
`status -z` for dirty counts, and one conditional `rev-list --left-right --count` only when an
upstream is configured. No `git log` (RB-04 keeps this oracle cheap enough for every batch).

Repo detection is deliberately NOT `rev-parse --abbrev-ref HEAD`: in a freshly initialized repo
with zero commits, HEAD is an unborn symbolic ref and `rev-parse --abbrev-ref HEAD` fails exactly
the same way it does outside a repository at all (`fatal: ambiguous argument 'HEAD'`) -- measured
live against a real just-`git init`'d sandbox, which reported `is_git_repo: false` for a real
repository. `--is-inside-work-tree` and `symbolic-ref` both succeed on an unborn HEAD.
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, git, oracle  # noqa: E402


def run(ctx: oracle.Context) -> dict:
    root = ctx.project_root

    try:
        repo_probe = git.run(["rev-parse", "--is-inside-work-tree"], cwd=root, timeout_s=10)
    except Error as exc:
        return oracle.result("unknown", "git is not available", reason=str(exc))

    if repo_probe.rc != 0 or repo_probe.out.strip() != "true":
        return oracle.result(
            "pass", "not a git repository",
            data={"is_git_repo": False, "branch": None, "head": None, "dirty": False,
                  "modified": 0, "staged": 0, "untracked": 0, "deleted": 0,
                  "has_upstream": False, "ahead": None, "behind": None},
        )

    branch_probe = git.run(["symbolic-ref", "--short", "HEAD"], cwd=root, timeout_s=10)
    branch = branch_probe.out.strip() if branch_probe.rc == 0 else None  # None => detached

    head_probe = git.run(["rev-parse", "HEAD"], cwd=root, timeout_s=10)
    head = head_probe.out.strip() if head_probe.rc == 0 and head_probe.out.strip() else None

    rows = git.status(cwd=root)
    # Y='D' is a working-tree deletion, not a content "modification" -- excluded here and
    # counted only in `deleted`, or the same file would be reported in both buckets.
    modified = sum(1 for xy, _ in rows if xy[1] not in (" ", "?", "D"))
    staged = sum(1 for xy, _ in rows if xy[0] not in (" ", "?"))
    untracked = sum(1 for xy, _ in rows if xy == "??")
    deleted = sum(1 for xy, _ in rows if "D" in xy)

    has_upstream = False
    ahead = behind = None
    try:
        ab = git.run(["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
                     cwd=root, timeout_s=10)
    except Error:
        ab = None
    if ab is not None and ab.rc == 0:
        parts = ab.out.split()
        if len(parts) == 2 and all(p.lstrip("-").isdigit() for p in parts):
            has_upstream = True
            behind, ahead = int(parts[0]), int(parts[1])

    dirty = bool(rows)
    summary = f"branch={branch or '(detached)'} {'dirty' if dirty else 'clean'}"
    if has_upstream:
        summary += f" ahead={ahead} behind={behind}"
    return oracle.result("pass", summary[:200], data={
        "is_git_repo": True, "branch": branch, "head": head, "dirty": dirty,
        "modified": modified, "staged": staged, "untracked": untracked, "deleted": deleted,
        "has_upstream": has_upstream, "ahead": ahead, "behind": behind,
    })


if __name__ == "__main__":
    sys.exit(oracle.main(run))
