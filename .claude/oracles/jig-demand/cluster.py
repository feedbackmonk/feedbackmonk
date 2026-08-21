#!/usr/bin/env python
# jig-demand clustering core -- SINGLE OWNER of the algorithm (JIG-09, DEC-362).
#
# Both twins (run.sh, run.ps1) invoke THIS file rather than each reimplementing
# IDF weighting, single-link union-find and occasion counting. That is deliberate
# and follows the machine-quiescence precedent (probe.ps1 = raw facts, shared by
# both twins): a clustering algorithm written twice is a TWIN-01 parity defect
# waiting to happen, and parity smokes are a consistency property, never a
# correctness one (DISC-MON-17 -- twin parity passed on the RED run).
#
# Reads:
#   argv[1] = path to aria-probe-candidates.jsonl      (the demand log)
#   argv[2] = path to jig-demand-dispositions.jsonl    (may not exist)
# Env (all optional, already resolved by the caller):
#   JIGD_MIN_CANDIDATES, JIGD_MIN_OCCASIONS, JIGD_SIMILARITY, JIGD_TOP_N
#
# Writes the complete oracle JSON on stdout. Read-only; writes no file.
#
# Why lexical and not exact-key: measured 2026-08-14 over 223 records in 12
# projects -- exact `capability`-string duplicates number 3 groups (1.3%).
# Free-text asks never repeat verbatim, so there is nothing for an exact key to
# dedupe. The proxy's bounds are declared in oracle.json `assertion.known_gaps`.

import json
import os
import re
import sys
import math
import hashlib
from collections import Counter, defaultdict

MIN_CANDIDATES = int(os.environ.get("JIGD_MIN_CANDIDATES") or 3)
MIN_OCCASIONS = int(os.environ.get("JIGD_MIN_OCCASIONS") or 3)
SIMILARITY = float(os.environ.get("JIGD_SIMILARITY") or 0.20)
TOP_N = int(os.environ.get("JIGD_TOP_N") or 5)

# Stopwords: ordinary English function words PLUS the boilerplate vocabulary of
# the logging convention itself. The second half is load-bearing -- measured on
# quiqpic's corpus, "verify" appears in 19 of 39 questions and "command"/"invoke"
# in 18, so without this a cluster forms on the log's own house style rather
# than on a capability.
STOP = set("""
a an the of to in on for and or is are was were be been being do does did it its this that these
those with without by from at as not no can could would should will shall may might must have has
had i we you they them us our your their there here what which who whom when where why how all any
both each few more most other some such only own same so than too very just now then once about
into through during before after above below up down out off over under again further need needs
needed one two via per
""".split())

# Archetype hints -- keyword sets matched against JIG_CATALOG.md recognition
# signatures. A HINT for the route, never a verdict on what to build; declared as
# such in known_gaps. Most specific first is irrelevant (scored by match count),
# but a tie breaks toward the earlier entry.
ARCHETYPES = [
    ("shell-lane", {"webview", "webview2", "electron", "tauri", "shell", "engine", "chromium", "parity"}),
    ("companion", {"otp", "email", "mail", "webhook", "inbox", "sms", "external", "out-of-band"}),
    ("fixture", {"ui", "screen", "screenshot", "render", "renders", "rendered", "click", "clicking",
                 "drive", "driving", "window", "visual", "visually", "pointer", "hover", "tap",
                 "button", "panel", "dialog", "toolbar", "menu", "canvas", "emulator", "device"}),
    ("scenario-replayer", {"flow", "flows", "end-to-end", "multi-step", "replay", "sequence",
                           "onboarding", "walkthrough", "steps"}),
    ("state-fabricator", {"seed", "seeded", "seeding", "setup", "starting", "persona", "sandbox",
                          "state", "fixture", "preload"}),
    ("fabricator", {"mock", "fake", "sample", "synthetic", "payload", "corpus-input", "generate"}),
    ("corpus-harvester", {"corpus", "samples", "collect", "harvest", "real-world"}),
    ("log-distiller", {"log", "logs", "output", "stdout", "trace", "extract", "parse", "grep"}),
    ("diff-summarizer", {"diff", "compare", "comparison", "before", "golden", "baseline"}),
    ("environment-resetter", {"reset", "teardown", "restart", "clean", "cache", "rebuild"}),
    ("oracle", {"investigation", "recurring", "session-start", "project-state"}),
    ("probe", {"assert", "assertion", "behavior", "behaviour", "regression", "crystallize"}),
    ("hook-gate", {"rule", "review", "enforce", "mistake", "guard", "gate"}),
]


# Label-only stoplist. Applied when NAMING a cluster, never when clustering it.
# These are the AOR/logging house style ("command-invoke:", "state-dump:") and
# the generic evaluation verbs every candidate carries; they are real signal for
# similarity (a `command-invoke:` ask genuinely resembles another) and pure noise
# in a headline. Filtering them at label time only keeps the measured clustering
# behaviour untouched -- widening STOP instead would silently change which
# records merge.
LABEL_STOP = set("""
command invoke dump aor verb verify verified agent human actually live end back real
""".split())


def toks(s):
    s = re.sub(r"[^a-z0-9]+", " ", (s or "").lower())
    return set(t for t in s.split() if len(t) > 2 and t not in STOP)


def read_jsonl(path):
    out = []
    if not path or not os.path.isfile(path):
        return out
    try:
        with open(path, "r", encoding="utf-8-sig", errors="replace") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    d = json.loads(line)
                except Exception:
                    continue
                if isinstance(d, dict):
                    out.append(d)
    except Exception:
        return out
    return out


def occasion(rec):
    """Distinct-occasion key. sessionId when the writer could name itself; else
    the calendar date. The date fallback is deliberately CONSERVATIVE -- several
    candidates from one session on one day collapse to one occasion, which is
    exactly the case (a session enumerating its own gaps) this gate exists to
    withhold."""
    sid = rec.get("sessionId")
    if sid:
        return ("sid", str(sid))
    return ("day", str(rec.get("ts") or "")[:10])


def is_dispositioned(rec, covered_ts):
    # In-record triage object: honored for backward compatibility. One project
    # (SquirrelMaid) hand-invented exactly this field and drained 24 candidates
    # with it before any support existed -- evidence for standardising the drain
    # record, so its shape is read rather than orphaned.
    if isinstance(rec.get("triage"), dict) and rec["triage"]:
        return True
    return str(rec.get("ts") or "") in covered_ts


def cluster_indices(recs, thresh):
    T = [toks((r.get("question") or "") + " " + (r.get("capability") or "")) for r in recs]
    n = len(recs)
    df = Counter()
    for t in T:
        df.update(t)

    def idf(w):
        return math.log((n + 1.0) / (df[w] + 0.5))

    norms = [math.sqrt(sum(idf(w) for w in t)) or 0.0 for t in T]
    parent = list(range(n))

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for i in range(n):
        if not T[i]:
            continue
        for j in range(i + 1, n):
            if not T[j]:
                continue
            inter = T[i] & T[j]
            if not inter:
                continue
            den = norms[i] * norms[j]
            if den <= 0:
                continue
            if (sum(idf(w) for w in inter) / den) >= thresh:
                a, b = find(i), find(j)
                if a != b:
                    parent[a] = b
    groups = defaultdict(list)
    for i in range(n):
        groups[find(i)].append(i)
    return list(groups.values()), T, idf


def label_for(members, T, idf):
    """The most distinctive terms shared by at least half the members. A handle
    for a human, explicitly not an identifier -- clusterKey is the identifier."""
    half = max(2, (len(members) + 1) // 2)
    shared = Counter()
    for i in members:
        shared.update(T[i])
    # Sort by breadth FIRST (how many members carry the term), then by
    # distinctiveness. Breadth-first matters: with a 3-member cluster, "shared
    # by half" admits a term present in 2, and ranking on IDF alone then hands
    # the label to the rarest such term -- which is by construction the least
    # representative one.
    cands = [(c, idf(w), w) for w, c in shared.items()
             if c >= half and w not in LABEL_STOP]
    cands.sort(reverse=True)
    picked = [w for _, _, w in cands[:4]]
    if picked:
        return " ".join(picked)
    # Every shared term was house style. Say so rather than emitting a label
    # that reads like a capability; the `sample` field carries the real content.
    return "(shared terms are all house style -- read sample)"


def archetype_for(members, T):
    bag = Counter()
    for i in members:
        bag.update(T[i])
    best, best_score = None, 0
    for slug, kws in ARCHETYPES:
        score = sum(1 for k in kws if k in bag)
        if score > best_score:
            best, best_score = slug, score
    return best


def main():
    log_path = sys.argv[1] if len(sys.argv) > 1 else ""
    disp_path = sys.argv[2] if len(sys.argv) > 2 else ""

    recs = read_jsonl(log_path)
    disps = read_jsonl(disp_path)
    covered = set()
    for d in disps:
        for t in (d.get("covers") or []):
            covered.add(str(t))

    total = len(recs)
    live = [r for r in recs if not is_dispositioned(r, covered)]
    dispositioned = total - len(live)

    out = {
        "status": "ok",
        "clusters": [],
        "totals": {
            "candidates": total,
            "dispositioned": dispositioned,
            "undispositioned": len(live),
        },
        "clustered": True,
        "briefing": "",
    }

    if len(live) < MIN_CANDIDATES:
        sys.stdout.write(json.dumps(out))
        return

    groups, T, idf = cluster_indices(live, SIMILARITY)
    fired = []
    for members in groups:
        if len(members) < MIN_CANDIDATES:
            continue
        occs = set(occasion(live[i]) for i in members)
        if len(occs) < MIN_OCCASIONS:
            continue
        member_ts = sorted(str(live[i].get("ts") or "") for i in members)
        key = hashlib.sha1("\n".join(member_ts).encode("utf-8")).hexdigest()[:12]
        qs = [(live[i].get("question") or "").strip() for i in members]
        qs = [(q[:140] + "...") if len(q) > 140 else q for q in qs]
        sample = qs[:3]
        if len(qs) > 3:
            sample = sample + ["(+%d more)" % (len(qs) - 3)]
        dates = sorted(d[:10] for d in member_ts if d)
        fired.append({
            "label": label_for(members, T, idf),
            "clusterKey": key,
            "candidates": len(members),
            "occasions": len(occs),
            "firstSeen": dates[0] if dates else "",
            "lastSeen": dates[-1] if dates else "",
            "sample": sample,
            "memberTs": member_ts,
            "suggestedArchetype": archetype_for(members, T),
        })

    # Rank by occasions first, then size: recurrence across sessions is the
    # signal that justifies building; a big cluster from few occasions is not.
    fired.sort(key=lambda c: (-c["occasions"], -c["candidates"], c["clusterKey"]))
    shown = fired[:TOP_N]
    out["clusters"] = shown

    if fired:
        out["status"] = "signal"
        top = fired[0]
        more = ""
        if len(fired) > len(shown):
            more = " (+%d more cluster(s))" % (len(fired) - len(shown))
        # When every shared term was house style the label is a confession, not
        # a name -- fall back to the first member's own words so the line still
        # tells the reader what was asked for.
        headline = top["label"]
        if headline.startswith("(") and top["sample"]:
            headline = top["sample"][0][:70].rstrip() + "..."
        out["briefing"] = (
            '%d session(s) asked for "%s" (%d candidates, %s..%s) and nothing was built%s '
            "-- route it (/0-uldf-inject) or record why not: "
            "scripts/aria/jig-demand-triage.sh --cluster %s"
        ) % (top["occasions"], headline, top["candidates"],
             top["firstSeen"], top["lastSeen"], more, top["clusterKey"])

    sys.stdout.write(json.dumps(out))


if __name__ == "__main__":
    main()
