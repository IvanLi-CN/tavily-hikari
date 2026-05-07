# History

## 2026-05-07

- Created after production showed transient SQLite `database is locked` errors across token billing,
  MCP session locks, scheduled job starts, and LinuxDo OAuth upserts.
- Chose bounded retry hardening as the first repair because the evidence showed short writer
  collisions, while API rebalance selection and research result key pinning were behaving as
  designed.
