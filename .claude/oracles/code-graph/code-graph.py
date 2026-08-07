#!/usr/bin/env python3
"""code-graph.py -- canonical implementation of the ULDF module-level
dependency-graph oracle (scrutiny Arc 3 proposal A1, verdict AMEND;
docs/planning/scrutiny-hierarchical-modularity-20260707.md sec 6 A1 / sec 5 M6).

WHAT IT IS
    A read-only query oracle over the repo's MODULE-LEVEL dependency graph. It
    does NOT derive the graph itself -- it reuses the SAME pluggable
    `dependencyDrift.edgeExtractor` seam the dependency-drift check already
    defines (M6): it shells `dependency-drift.py --emit-edges` (the additive
    mode) to obtain the derived module edges + per-module coverage, then answers
    four query verbs over the resulting module DAG:

        --deps <module>       modules <module> depends on (efferent / out-edges)
        --consumers <module>  modules that depend ON <module> (afferent / reverse)
        --impact <module>     transitive closure of consumers (everything that
                              could break if <module>'s interface changes) -- the
                              set Arc 3 A4 injects into worker briefs / finalize
        --cycles              all dependency cycles / SCCs of size >= 2

    --deps / --consumers accept --transitive to extend to the transitive closure;
    --impact is transitive by construction. Default (no verb) = a full-graph
    summary. Verbs are mutually exclusive.

WHAT IT IS NOT
    NOT a per-language parser fleet, NOT a new edge derivation. The grep-grade
    default observer + the pluggable per-project edge-extractor ARE the coverage
    story (M6). If you find yourself writing an import parser here, stop -- plug
    an extractor into .claude/config.json dependencyDrift.edgeExtractor instead.

COVERAGE HONESTY (M6 / NO-DATA-never-a-silent-absence-of-edges)
    Every answer carries a MANDATORY `coverage` field = the weakest (min) of the
    coverage of the modules involved in the answer:
        full       every involved module is authoritatively extractor-covered
        grep-only  at least one involved module is only grep-observed -> the
                   answer is a FLOOR (lossy), not a proven-complete set
        none       nothing observable for the involved module(s) -> an empty
                   result is inability-to-observe, NOT proven absence
    An impact set computed over grep-only edges SAYS SO (coverage grep-only +
    coverageNote). Unparsed files are NO-DATA, never a silent absence-of-edges.

OUTPUT
    A single JSON document on stdout, sorted keys + deterministic separators, so
    the run.sh and run.ps1 wrappers (which share this one canonical python)
    produce BYTE-IDENTICAL JSON. Schema frozen in README.md. Advisory: exit 0.

USAGE
    code-graph.py [--deps M | --consumers M | --impact M | --cycles]
                  [--transitive] [--root DIR] [--config FILE] [--compact]
"""

import json
import os
import subprocess
import sys

SCHEMA_VERSION = "1.0"
RESULT_CAP = 500          # no silent unbounded lists; truncated flag when hit

_HERE = os.path.dirname(os.path.abspath(__file__))
# The edge source lives beside the oracle tree: <root>/scripts/dependency-drift.py
# in the template repo (claude-template/oracles/code-graph -> claude-template/
# scripts) AND in a deployed project (.claude/oracles/code-graph -> .claude/
# scripts). One relative hop covers both layouts.
_DDPY = os.path.normpath(os.path.join(_HERE, "..", "..", "scripts", "dependency-drift.py"))

_LEVELS = {"none": 0, "grep-only": 1, "full": 2}
_NAMES = {0: "none", 1: "grep-only", 2: "full"}


def _emit(doc, compact):
    if compact:
        sys.stdout.write(json.dumps(doc, separators=(",", ":"), sort_keys=True) + "\n")
    else:
        sys.stdout.write(json.dumps(doc, indent=2, sort_keys=True) + "\n")


def _no_data(reason, query, compact):
    _emit({
        "status": "no-data",
        "schemaVersion": SCHEMA_VERSION,
        "query": query,
        "result": [],
        "coverage": "none",
        "coverageNote": "",
        "truncated": False,
        "reason": reason,
        "extractor": {"configured": False, "coveredModules": []},
        "briefing": "",
    }, compact)


def _norm_module(t):
    """Normalize a user-supplied module target to a graph module key."""
    t = (t or "").replace("\\", "/").strip()
    t = t.strip("/")
    if t in ("", "."):
        return "<root>"
    return t


def _combine_coverage(mods, cov):
    if not mods:
        return "none"
    lv = min(_LEVELS.get(cov.get(m, "none"), 0) for m in mods)
    return _NAMES[lv]


def _coverage_note(coverage):
    if coverage == "full":
        return ""
    if coverage == "grep-only":
        return ("at least one involved module is only grep-observed; this answer "
                "is a FLOOR (lossy grep-grade edges), not a proven-complete set")
    return ("no extractor-covered or grep-observable edges for the involved "
            "module(s); an empty result is inability-to-observe, not proven "
            "absence (NO-DATA floor)")


def _closure(start, adj):
    """Transitive closure of `start` over adjacency `adj` (excludes `start`)."""
    seen = set()
    stack = sorted(adj.get(start, set()))
    while stack:
        n = stack.pop()
        if n in seen:
            continue
        seen.add(n)
        for nxt in sorted(adj.get(n, set())):
            if nxt not in seen:
                stack.append(nxt)
    seen.discard(start)
    return seen


def _tarjan_sccs(nodes, adj):
    """Deterministic iterative Tarjan; returns SCCs with >= 2 members as sorted
    member lists. (Lateral edges never self-loop, so size-1 SCCs are not cycles.)"""
    index = {}
    low = {}
    on_stack = set()
    stack = []
    counter = [0]
    sccs = []

    def strongconnect(root):
        # Iterative DFS to avoid recursion limits on large graphs.
        work = [(root, iter(sorted(adj.get(root, set()))))]
        index[root] = low[root] = counter[0]; counter[0] += 1
        stack.append(root); on_stack.add(root)
        while work:
            v, it = work[-1]
            advanced = False
            for w in it:
                if w not in index:
                    index[w] = low[w] = counter[0]; counter[0] += 1
                    stack.append(w); on_stack.add(w)
                    work.append((w, iter(sorted(adj.get(w, set())))))
                    advanced = True
                    break
                elif w in on_stack:
                    low[v] = min(low[v], index[w])
            if advanced:
                continue
            work.pop()
            if low[v] == index[v]:
                comp = []
                while True:
                    w = stack.pop(); on_stack.discard(w)
                    comp.append(w)
                    if w == v:
                        break
                if len(comp) >= 2:
                    sccs.append(sorted(comp))
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[v])

    for n in sorted(nodes):
        if n not in index:
            strongconnect(n)
    # Deterministic order: larger cycles first, then lexicographic by first member.
    sccs.sort(key=lambda c: (-len(c), c[0]))
    return sccs


def _cap(lst):
    """Return (capped_list, truncated)."""
    if len(lst) > RESULT_CAP:
        return lst[:RESULT_CAP], True
    return lst, False


def load_graph(root, config_path):
    """Shell dependency-drift.py --emit-edges and parse the module graph.
    Returns (graphdict, err) where err is a reason string on failure."""
    if not os.path.isfile(_DDPY):
        return None, "edge source dependency-drift.py not found"
    cmd = [sys.executable, _DDPY, "--emit-edges", "--compact"]
    if root:
        cmd += ["--root", root]
    if config_path:
        cmd += ["--config", config_path]
    try:
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
    except Exception as e:  # noqa: BLE001
        return None, "edge source failed: %s" % str(e)[:120]
    out = p.stdout.decode("utf-8", "replace").replace("\r\n", "\n").replace("\r", "\n").strip()
    if not out:
        return None, "edge source produced no output"
    try:
        return json.loads(out), None
    except ValueError as e:
        return None, "edge source emitted invalid JSON: %s" % str(e)[:80]


def main(argv):
    verb = None
    target = None
    transitive = False
    root_arg = None
    config_path = None
    compact = False
    verbs_seen = 0

    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--deps":
            verb = "deps"; target = argv[i + 1]; verbs_seen += 1; i += 2; continue
        if a == "--consumers":
            verb = "consumers"; target = argv[i + 1]; verbs_seen += 1; i += 2; continue
        if a == "--impact":
            verb = "impact"; target = argv[i + 1]; verbs_seen += 1; i += 2; continue
        if a == "--cycles":
            verb = "cycles"; verbs_seen += 1; i += 1; continue
        if a == "--transitive":
            transitive = True; i += 1; continue
        if a == "--root":
            root_arg = argv[i + 1]; i += 2; continue
        if a.startswith("--root="):
            root_arg = a[len("--root="):]; i += 1; continue
        if a == "--config":
            config_path = argv[i + 1]; i += 2; continue
        if a.startswith("--config="):
            config_path = a[len("--config="):]; i += 1; continue
        if a == "--compact":
            compact = True; i += 1; continue
        if a in ("-h", "--help"):
            sys.stdout.write(__doc__)
            return 0
        i += 1

    query = {
        "verb": verb or "summary",
        "target": _norm_module(target) if target is not None else None,
        "transitive": transitive if verb in ("deps", "consumers") else (verb == "impact"),
    }

    if verbs_seen > 1:
        _no_data("verbs are mutually exclusive (--deps/--consumers/--impact/--cycles)", query, compact)
        return 0

    root = os.path.abspath(root_arg) if root_arg else None
    graph, err = load_graph(root, config_path)
    if err is not None:
        _no_data(err, query, compact)
        return 0

    # not-a-git-repo / no-tracked-files: the edge source signals it via note +
    # an empty module set.
    note = (graph.get("extractor") or {}).get("note")
    modules = graph.get("modules") or []
    if note in ("not-a-git-repo", "no-tracked-files") or not modules:
        reason = "not a git repository" if note == "not-a-git-repo" else (
            "no tracked files" if note == "no-tracked-files"
            else "no modules (no README-anchored directories found)")
        _no_data(reason, query, compact)
        return 0

    cov = {}
    all_mods = set()
    for m in modules:
        p = m.get("path")
        all_mods.add(p)
        cov[p] = m.get("coverage", "none")

    adj = {}     # from -> set(to)   (out-edges / efferent)
    radj = {}    # to   -> set(from) (reverse / afferent)
    for e in (graph.get("edges") or []):
        f = e.get("from"); t = e.get("to")
        adj.setdefault(f, set()).add(t)
        radj.setdefault(t, set()).add(f)

    ext = graph.get("extractor") or {}
    ext_pass = {"configured": bool(ext.get("configured")),
                "coveredModules": ext.get("coveredModules") or []}

    # ---- resolve verb --------------------------------------------------------
    briefing = ""

    if verb == "cycles":
        sccs = _tarjan_sccs(all_mods, adj)
        cyc_objs = [{"members": c, "size": len(c)} for c in sccs]
        cyc_objs, truncated = _cap(cyc_objs)
        involved = set()
        for c in sccs:
            involved.update(c)
        coverage = _combine_coverage(involved or all_mods, cov)
        if sccs:
            briefing = "[code-graph] %d dependency cycle(s) detected among modules (advisory)" % len(sccs)
        result = cyc_objs

    elif verb in ("deps", "consumers", "impact"):
        tgt = query["target"]
        if tgt not in all_mods:
            _no_data("module '%s' not found" % tgt, query, compact)
            return 0
        if verb == "deps":
            reached = _closure(tgt, adj) if transitive else set(adj.get(tgt, set()))
        elif verb == "consumers":
            reached = _closure(tgt, radj) if transitive else set(radj.get(tgt, set()))
        else:  # impact = transitive consumers
            reached = _closure(tgt, radj)
        result_list = sorted(reached)
        result_list, truncated = _cap(result_list)
        involved = {tgt} | set(result_list)
        coverage = _combine_coverage(involved, cov)
        result = result_list

    else:  # summary (default)
        cyc_objs = _tarjan_sccs(all_mods, adj)
        mods_sorted = sorted(all_mods)
        mods_sorted, truncated = _cap(mods_sorted)
        coverage = _combine_coverage(all_mods, cov)
        if cyc_objs:
            briefing = "[code-graph] %d dependency cycle(s) detected among modules (advisory)" % len(cyc_objs)
        result = {
            "modules": mods_sorted,
            "edgeCount": len(graph.get("edges") or []),
            "cycleCount": len(cyc_objs),
        }

    _emit({
        "status": "ok",
        "schemaVersion": SCHEMA_VERSION,
        "query": query,
        "result": result,
        "coverage": coverage,
        "coverageNote": _coverage_note(coverage),
        "truncated": bool(truncated),
        "extractor": ext_pass,
        "briefing": briefing,
    }, compact)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
