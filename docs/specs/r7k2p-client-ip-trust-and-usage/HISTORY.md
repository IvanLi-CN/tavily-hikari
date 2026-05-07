# History

- 2026-05-08: Added spec for trusted client IP parsing, 7-day distinct IP usage, and admin confirmation UI.
- 2026-05-08: Kept default trusted proxy CIDRs loopback-only so private-network direct clients cannot spoof client IP headers unless an admin explicitly trusts those proxy ranges.
- 2026-05-08: Denied sensitive header names in trusted client IP header settings so diagnostic snapshots cannot persist request secrets through operator typo.
