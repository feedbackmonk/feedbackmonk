<!-- #!delivers-on tool:Bash(git push|store push|git remote|git config) -->

## Git remote (GitHub, not local Gitea) — HTTP/1.1 required

Unlike most projects on this machine (which push to local Gitea), this repo's
`origin` is the real public remote `github.com/feedbackmonk/feedbackmonk`. Over
HTTP/2 the push transfer **hangs indefinitely** when run from an automated shell
(the Claude Bash tool), even though `fetch`/`ls-remote` succeed — it is not an
auth prompt, it is an HTTP/2 send-pack stall. Fix (already set repo-local in
`.git/config`, but `.git/config` isn't version-controlled, so re-apply if the repo
is re-cloned): `git config http.version HTTP/1.1`. With that set, `git push`
completes normally from any context.

**Second obstacle — the credential helper order breaks automated pushes.** `credential.helper`
resolves to `manager` (Git Credential Manager) then `store`. GCM runs **first**, tries to prompt on
`/dev/tty`, and — with no TTY in the Claude Bash tool — **hard-errors the entire credential lookup**
(`failed to execute prompt script` → `fatal: could not read Username for 'https://github.com'`).
It does *not* fall through, so a perfectly valid credential sitting in `~/.git-credentials` is never
consulted. Symptom is identical to "not logged in", which is misleading: logging in again does not
help, because the stored credential was never the problem.

Workaround that works from an automated shell (one-shot, mutates nothing):

```bash
git -c credential.helper=store push
```

Two plain `git push` attempts failed with valid stored credentials; the `-c` form pushed
immediately. A persistent fix would reorder or drop the `manager` helper — that is a `git config`
write, so it needs the owner's explicit word; until then, use the `-c` form above.
