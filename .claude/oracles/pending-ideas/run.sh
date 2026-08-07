#!/bin/bash
# pending-ideas oracle (Unix) -- INJECT-08 (FROZEN SCHEMA)
#
# Surfaces UN-TRIAGED (status: PROPOSED) items in docs/planning/deferred/ at
# session-start, so a target project notices injected/deferred ideas without a
# human re-driving. Feeder-side visibility for /0-uldf-inject (§ INJECT).
#
# Output: single-line JSON (always-fresh; ~fast lane).
#
# Contract (INJECT-08, widened by DEC-281 -- see PARKED below):
#   - Lists count + {id,title,origin,injected_at,source_project} for status:PROPOSED.
#     TRIAGED / IN-PROGRESS / COMPLETED / DISMISSED are excluded.
#   - Gracefully absent: no docs/planning/deferred/ OR zero PROPOSED items ->
#     empty `briefing` (the session-start fan-out suppresses the line). Never a
#     "0 pending ideas" noise line.
#
# PARKED (DEC-281, DEFER-109) -- the inverted gap. Surfacing ONLY `PROPOSED`
# means a brief that is genuinely still open but has been triaged is invisible at
# every session start, permanently: `PROPOSED` means UN-triaged, so the
# documented, correct workflow -- triage it, then park it with a re-arm
# condition -- is also what switches its visibility off for good. The corpus's
# own history is the evidence: 27 briefs sat at TRIAGED and 21 of them had
# shipped long ago. TRIAGED is where items go to stop being looked at.
#
#   - `PARKED` = triaged, deliberately not being built, with a stated re-arm
#     condition. It is NOT a spelling of done, and NOT a spelling of TRIAGED.
#   - It rides `parked_count` + `parked[]`, NOT `pending_count`/`items[]`. That
#     is deliberate: INJECT-08's frozen clauses say `items` is PROPOSED-only and
#     "un-triaged", and a parked item is by definition triaged. Folding it into
#     the count would silently redefine a frozen field; a new field is additive.
#     The ONE clause deliberately widened is graceful absence -- the briefing is
#     now empty when there is nothing PROPOSED **and** nothing PARKED. Its
#     purpose ("never a 0-noise line") is preserved exactly: a parked item is
#     not noise, it is the thing this field exists to show.
#   - The re-arm condition is reported by PRESENCE ONLY, never evaluated. Whether
#     a condition is satisfiable is a judgment question, and OVALID-02's
#     precedent is explicit that only the presence of a declaration is
#     mechanizable. A PARKED brief with no declared re-arm is still surfaced, and
#     said to have none -- that is itself worth seeing.
#   - There is deliberately NO stale-PARKED signal. A "this has been parked too
#     long" counter is the drive-to-zero ritual OVALID's `bounded` verdict was
#     designed to avoid: it would train agents to unpark items to clear a number.
#     Parked-with-a-reason is a healthy steady state.
#
# What this oracle still does NOT do, stated rather than implied: it says nothing
# about briefs at TRIAGED. Relabelling the corpus is a separate, human-judged
# act -- this only gives the label somewhere to be seen.
#   - NO-DATA honesty: a deferred dir we cannot read, or DEFER-*.md files we
#     cannot parse a status from, -> `no_data:true` + a NO-DATA briefing.
#     Never a silent "none".
#   - Freshness: presence/mtime of deferred/ + the `status` marker; deterministic.
#
# Briefing constraint: the session-start fan-out extracts the briefing with a
# "[^\"]*" regex, so the briefing string MUST NOT contain a double-quote. Use
# arrows/parens/semicolons only.
#
# Format robustness: recognizes BOTH the INJECT-02 YAML front-matter (`status:`)
# and the legacy DEFER-era body form (`**Status**:`) so consolidating defer into
# inject introduces no regression on pre-existing deferred items.
#
# THE STATUS WORD IS THE FIRST TOKEN, NOT THE WHOLE LINE (DEC-317, DEFER-151).
# This oracle used to normalize the entire status line -- uppercase, ALL
# whitespace stripped -- and compare it to `PROPOSED` / `PARKED` exactly. The
# corpus does not write it that way: 28 of 160 briefs carry a rationale after
# the word (`status: PARKED 2026-08-06 -- the fix is SHIPPED (...); RE-ARM ...`),
# which is house style and is how the framework's own briefs are written. Under
# the old compare those collapse to one 400-character token that matches
# nothing, and the file is dropped in SILENCE -- measured live on
# DEFER-146/DEFER-147, two parked-with-a-re-arm briefs that `parked[]` exists to
# show. Removing the rationale and changing nothing else surfaced them.
# So the word is parsed out of the line; the surfaced SET is unchanged.
#
# AND A WORD OUTSIDE THE SET IS REPORTED, NOT DROPPED (DEC-317). The filter
# itself is correct and is NOT widened here (INJECT-08 freezes `items[]` as
# PROPOSED-only; DEC-281 deliberately kept PARKED out of `pending_count`). What
# was wrong is that an unrecognized word left NO trace: a live cross-project
# injection carrying `**Status**: OPEN` -- the framework's own word for an open
# DISCOVERIES.md entry -- was invisible here and unaudited by
# `dec-alloc-guard --check-defer`, and a file this oracle silently drops is
# indistinguishable from a file that is not there (ORACLE-COST-09/10, DEC-292,
# one door over). Words known to mean *done* stay silently invisible, which is
# correct; anything else rides `unknown_status_count`/`unknown_status[]` and a
# briefing clause. It is NOT promoted into `pending_count`.

set +e

DEFERRED_DIR="docs/planning/deferred"

emit_absent() {
    # No deferred dir at all -> legitimately absent (NOT no-data).
    printf '{"pending_count":0,"items":[],"parked_count":0,"parked":[],"unknown_status_count":0,"unknown_status":[],"no_data":false,"briefing":""}\n'
    exit 0
}

# Status words that mean DONE. A brief at one of these is correctly invisible,
# so it must NOT reach the unknown-status clause -- otherwise the signal is 130
# lines of noise and gets ignored, which is how a real unknown word hides.
# Deliberately explicit rather than a pattern: the point is that the set is
# WRITTEN DOWN somewhere a reader and a writer can both see it (DEFER-151 found
# the vocabulary living only in prose comments).
#
# DERIVED FROM A CENSUS, NOT FROM MEMORY (2026-08-06, 160 DEFER-*.md in this
# repo): the leading status words in use are RESOLVED 105, IMPLEMENTED 28,
# PARKED 7, PROPOSED 6, COMPLETED 5, RESOLVED-DECLINED 4, CLOSED 2, APPLIED 1,
# FOLDED 1, PHASE-1-IMPLEMENTED 1. Everything but PROPOSED/PARKED is below.
# TRIAGED / IN-PROGRESS / DISMISSED / SUPERSEDED / DECLINED are added from
# INJECT-10's lifecycle vocabulary though the corpus does not currently use
# them. The first hand-typed version of this list DROPPED
# PHASE-1-IMPLEMENTED and the pre-existing case-6 anti-noise cell reddened on
# it immediately -- re-derive from a census before editing this line.
KNOWN_DONE_STATUSES=" RESOLVED RESOLVED-DECLINED IMPLEMENTED PHASE-1-IMPLEMENTED COMPLETED TRIAGED IN-PROGRESS DISMISSED CLOSED SUPERSEDED DECLINED APPLIED FOLDED "

# ---- Graceful absence: no deferred dir ------------------------------------
[ -d "$DEFERRED_DIR" ] || emit_absent

# ---- Unreadable dir -> NO-DATA --------------------------------------------
if ! ls "$DEFERRED_DIR" >/dev/null 2>&1; then
    printf '{"pending_count":0,"items":[],"parked_count":0,"parked":[],"unknown_status_count":0,"unknown_status":[],"no_data":true,"briefing":"docs/planning/deferred/ present but unreadable -- NO-DATA (cannot confirm pending ideas)"}\n'
    exit 0
fi

esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
# Strip characters that would break the fan-out briefing regex (double-quotes)
# and collapse whitespace. Briefing text only.
brief_safe() { printf '%s' "$1" | tr -d '"' | tr '\r\n\t' '   ' | sed 's/  */ /g'; }

# fm_field <file> <key> : value of a front-matter `key:` line (first match),
# only within the leading `---`..`---` block. CR-stripped, trimmed.
fm_field() {
    awk -v key="$2" '
        NR==1 && $0 !~ /^---[[:space:]]*$/ { exit }   # no front-matter
        NR==1 { infm=1; next }
        infm && $0 ~ /^---[[:space:]]*$/ { exit }
        infm {
            line=$0
            sub(/\r$/,"",line)
            if (line ~ "^"key"[[:space:]]*:") {
                sub("^"key"[[:space:]]*:[[:space:]]*","",line)
                print line
                exit
            }
        }
    ' "$1" 2>/dev/null
}

# legacy_status <file> : value of a `**Status**:` body line (DEFER-era).
legacy_status() {
    grep -m1 -E '^\*\*Status\*\*:' "$1" 2>/dev/null \
        | sed -E 's/\r$//; s/^\*\*Status\*\*:[[:space:]]*//'
}

# heading_title <file> : title from the first `# DEFER-NNN: <title>` heading.
heading_title() {
    grep -m1 -E '^#[[:space:]]+DEFER-[0-9]+:' "$1" 2>/dev/null \
        | sed -E 's/\r$//; s/^#[[:space:]]+DEFER-[0-9]+:[[:space:]]*//'
}

# status_word <raw> : the leading token of a status line, uppercased. Cuts at
# the first byte outside [A-Za-z0-9_-], which handles the space form
# ("PARKED 2026-08-06 -- ..."), the em-dash form ("IMPLEMENTED--2026-07-12"),
# and the paren form ("COMPLETED (2026-07-02 ...)") alike, while keeping
# genuinely hyphenated words whole (RESOLVED-DECLINED, IN-PROGRESS).
# LC_ALL=C: byte-wise, so a multi-byte rationale cannot make sed choke.
status_word() {
    printf '%s' "$1" | LC_ALL=C sed -E 's/^[[:space:]]+//; s/^([A-Za-z0-9_-]*).*$/\1/' \
        | tr '[:lower:]' '[:upper:]'
}

# rearm_from_status <raw> : the re-arm condition when it was written INTO the
# status rationale ("... ; RE-ARM -- next session finding docs/specs clean ...")
# rather than into a `re-arm:` front-matter field. Presence only, never
# evaluated (OVALID-02) -- this exists so the oracle stops SAYING
# "no re-arm condition declared" about a brief that declared one, which is a
# wrong statement rather than a missing one.
rearm_from_status() {
    printf '%s' "$1" | LC_ALL=C sed -nE 's/.*[Rr][Ee]-[Aa][Rr][Mm][[:space:]]*[-:—]*[[:space:]]*(.+)$/\1/p'
}

items_json=""
first=1
count=0
malformed=0
briefing_items=""
parked_json=""
parked_first=1
parked_count=0
parked_briefing=""
unknown_json=""
unknown_first=1
unknown_count=0
unknown_briefing=""

# Iterate DEFER-*.md only. Non-matching files (README.md etc.) are ignored,
# never counted as malformed.
shopt -s nullglob 2>/dev/null
for f in "$DEFERRED_DIR"/DEFER-*.md; do
    [ -f "$f" ] || continue

    status_raw=$(fm_field "$f" "status")
    [ -n "$status_raw" ] || status_raw=$(legacy_status "$f")
    # DEC-317: the WORD, not the line (bash 3.2-safe; no ${x^^}).
    status=$(status_word "$status_raw")

    if [ -z "$status" ]; then
        # A DEFER-*.md we cannot read a status from -> NO-DATA, never silent.
        malformed=$((malformed + 1))
        continue
    fi

    # --- PARKED: triaged, deliberately unbuilt, with a re-arm condition -------
    # Handled BEFORE the PROPOSED filter and on its own fields, so nothing about
    # the frozen PROPOSED contract moves.
    if [ "$status" = "PARKED" ]; then
        pbase=$(basename "$f" .md)
        pid=$(printf '%s' "$pbase" | grep -oE '^DEFER-[0-9]+')
        [ -n "$pid" ] || pid=$(fm_field "$f" "id")
        [ -n "$pid" ] || pid="$pbase"
        ptitle=$(fm_field "$f" "title")
        [ -n "$ptitle" ] || ptitle=$(heading_title "$f")
        [ -n "$ptitle" ] || ptitle="$pbase"
        # PRESENCE only -- never evaluated (OVALID-02). Both spellings, matching
        # the injected-at / injected_at precedent two fields up.
        rearm=$(fm_field "$f" "re-arm")
        [ -n "$rearm" ] || rearm=$(fm_field "$f" "re_arm")
        [ -n "$rearm" ] || rearm=$(rearm_from_status "$status_raw")

        if [ "$parked_first" -eq 1 ]; then parked_first=0; else parked_json="$parked_json,"; fi
        parked_json="$parked_json{\"id\":\"$(esc "$pid")\",\"title\":\"$(esc "$ptitle")\",\"re_arm\":\"$(esc "$rearm")\"}"
        parked_count=$((parked_count + 1))

        pfrag="$pid $(brief_safe "$ptitle")"
        if [ -n "$rearm" ]; then
            pfrag="$pfrag (re-arm: $(brief_safe "$rearm"))"
        else
            pfrag="$pfrag (no re-arm condition declared)"
        fi
        if [ -z "$parked_briefing" ]; then parked_briefing="$pfrag"; else parked_briefing="$parked_briefing; $pfrag"; fi
        continue
    fi

    # --- Words outside the surfaced set (DEC-317) ------------------------------
    # Done-words stay silently invisible (correct). Anything else is REPORTED --
    # never promoted into pending_count, which would widen a frozen contract.
    if [ "$status" != "PROPOSED" ]; then
        case "$KNOWN_DONE_STATUSES" in
            *" $status "*) continue ;;
        esac
        ubase=$(basename "$f" .md)
        uid=$(printf '%s' "$ubase" | grep -oE '^DEFER-[0-9]+')
        [ -n "$uid" ] || uid=$(fm_field "$f" "id")
        [ -n "$uid" ] || uid="$ubase"
        if [ "$unknown_first" -eq 1 ]; then unknown_first=0; else unknown_json="$unknown_json,"; fi
        unknown_json="$unknown_json{\"id\":\"$(esc "$uid")\",\"status\":\"$(esc "$status")\"}"
        unknown_count=$((unknown_count + 1))
        # Cap the briefing list at 3; the array carries them all.
        if [ "$unknown_count" -le 3 ]; then
            ufrag="$(brief_safe "$uid") ($(brief_safe "$status"))"
            if [ -z "$unknown_briefing" ]; then unknown_briefing="$ufrag"; else unknown_briefing="$unknown_briefing; $ufrag"; fi
        fi
        continue
    fi

    # --- Extract the frozen tuple {id,title,origin,injected_at,source_project} ---
    base=$(basename "$f" .md)
    # id: filename-derived (DEFER-NNN), fallback front-matter `id:`, then the
    # basename. The basename leg is DEC-317: a slug-form brief with no
    # front-matter `id:` previously emitted "" into INJECT-08's frozen id field
    # -- an empty required key a consumer joins on silently, which is worse than
    # exclusion, and it rendered a double space in the briefing. The PARKED
    # branch has always had this leg; the PROPOSED branch did not.
    id=$(printf '%s' "$base" | grep -oE '^DEFER-[0-9]+')
    [ -n "$id" ] || id=$(fm_field "$f" "id")
    [ -n "$id" ] || id="$base"

    title=$(fm_field "$f" "title")
    [ -n "$title" ] || title=$(heading_title "$f")
    [ -n "$title" ] || title="$base"

    origin=$(fm_field "$f" "origin")
    [ -n "$origin" ] || origin="defer-local"   # legacy items are local by definition

    injected_at=$(fm_field "$f" "injected-at")
    [ -n "$injected_at" ] || injected_at=$(fm_field "$f" "injected_at")

    source_project=$(fm_field "$f" "source-project")
    [ -n "$source_project" ] || source_project=$(fm_field "$f" "source_project")

    if [ "$first" -eq 1 ]; then first=0; else items_json="$items_json,"; fi
    items_json="$items_json{\"id\":\"$(esc "$id")\",\"title\":\"$(esc "$title")\",\"origin\":\"$(esc "$origin")\",\"injected_at\":\"$(esc "$injected_at")\",\"source_project\":\"$(esc "$source_project")\"}"
    count=$((count + 1))

    # Briefing fragment (double-quote-free).
    frag="$id $(brief_safe "$title")"
    if [ "$origin" = "inject" ] && [ -n "$source_project" ]; then
        frag="$frag (inject <- $(brief_safe "$source_project"))"
    fi
    if [ -z "$briefing_items" ]; then briefing_items="$frag"; else briefing_items="$briefing_items; $frag"; fi
done

# ---- Compose briefing -------------------------------------------------------
no_data="false"
briefing=""

if [ "$count" -gt 0 ]; then
    plural="idea"; [ "$count" -gt 1 ] && plural="ideas"
    briefing="$count pending $plural: $briefing_items"
elif [ "$parked_count" -eq 0 ] && [ "$unknown_count" -eq 0 ] && [ "$malformed" -gt 0 ]; then
    # Zero parseable PROPOSED but files we could not parse -> never silent none.
    no_data="true"
    briefing="docs/planning/deferred/ has $malformed unparseable item(s) -- NO-DATA (cannot confirm pending ideas)"
fi

# PARKED rides its own clause, appended when present. The graceful-absence rule
# is now "nothing PROPOSED and nothing PARKED" -- the ONE deliberately widened
# clause of INJECT-08 (DEC-281). Its purpose is untouched: a parked item is not
# a 0-noise line, it is the thing being surfaced.
if [ "$parked_count" -gt 0 ]; then
    pplural="item"; [ "$parked_count" -gt 1 ] && pplural="items"
    pclause="$parked_count parked $pplural (triaged, open, not being built): $parked_briefing"
    if [ -n "$briefing" ]; then briefing="$briefing; $pclause"; else briefing="$pclause"; fi
fi

# Unrecognized status words ride their own clause (DEC-317). This is the second
# deliberate widening of INJECT-08's graceful-absence clause, and it is named as
# such: a file dropped for a word nobody recognizes is not 0-noise, it is the
# thing being surfaced -- the same argument DEC-281 made for PARKED. It does NOT
# touch pending_count/items[].
if [ "$unknown_count" -gt 0 ]; then
    uplural="item"; [ "$unknown_count" -gt 1 ] && uplural="items"
    uclause="$unknown_count $uplural with an unrecognized status word (not surfaced; surfaced words are PROPOSED and PARKED): $unknown_briefing"
    if [ "$unknown_count" -gt 3 ]; then uclause="$uclause; +$((unknown_count - 3)) more"; fi
    if [ -n "$briefing" ]; then briefing="$briefing; $uclause"; else briefing="$uclause"; fi
fi

# The NO-DATA note trails whatever was surfaced, so it can never displace it.
if [ "$malformed" -gt 0 ] && [ -n "$briefing" ] && [ "$no_data" = "false" ]; then
    no_data="true"
    briefing="$briefing; $malformed unparseable item(s) -- NO-DATA on those"
fi
# else: nothing PROPOSED, nothing PARKED, nothing malformed -> empty briefing.

printf '{"pending_count":%d,"items":[%s],"parked_count":%d,"parked":[%s],"unknown_status_count":%d,"unknown_status":[%s],"no_data":%s,"briefing":"%s"}\n' \
    "$count" "$items_json" "$parked_count" "$parked_json" "$unknown_count" "$unknown_json" "$no_data" "$(esc "$briefing")"
exit 0
