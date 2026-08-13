// Request-stat writes are intentionally durable only after the background coalescer flushes.
// Public reads must not acquire a writer merely to make an in-memory delta visible.
