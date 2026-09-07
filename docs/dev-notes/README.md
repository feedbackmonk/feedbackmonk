# `docs/dev-notes/` — notes that are true for one area

A dev note is content that is first-hour-critical for **one part** of this project rather than for
every session, so it lives here instead of in the project index.

- At most 30 lines per note.
- Every block sits under a delivery marker naming what trips it: `#!binds <glob>` for a path, or
  `#!delivers-on tool:Bash(<cmd>)` for a command.
- A note with no marker, or a marker whose glob matches nothing, is a note nobody will ever
  receive: `python <home>/hooks/resolver.py lint --project .` grades that.
