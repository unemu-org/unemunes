# Where to capture a decision

| Surface | Use |
| --- | --- |
| `backlog/decisions/` | Durable architecture, public API, source policy or cross-cutting workflow choices |
| Bead append-note | A choice bound to one task |

```sh
backlog decision create "<summary>"
ls backlog/decisions/
bd update <id> --append-notes "Decided: <choice> over <alternative> — <reason>; revisit if <trigger>."
```

A decision records status, date, decider, context, choice, alternatives,
consequences and trigger to revisit. Supersede an accepted decision in a new
record; do not silently change its substance. Tag cross-repository references,
for example `decision-41 (unemupc)`.
