-- Trigger port for the nine derived-state triggers that the baseline schema left out.
--
-- The concurrency and comparison details that are not a mechanical rename:
--   * Body JSON is parsed only when the payload decodes as UTF-8 text and passes
--     `pg_input_is_valid(..., 'jsonb')`; undecodable bytes are treated as "not JSON",
--     which is what the retired engine reported for the same bytes.
--   * Kind derivation reaches array items through `jsonb_array_elements` and reads
--     fields with the `#>>` path operator, so no SQL text is re-parsed as JSON.
--   * Predicates run over values that are not lowercased, so their pattern matches use
--     `ILIKE` to keep the retired engine's ASCII case-insensitive matching.
--   * Timestamps come from `extract(epoch FROM now())::bigint`; nothing reconstructs a
--     timestamp from an integer column.
--   * Aggregated text identities use `COLLATE "C"` so `min()` keeps the byte order the
--     retired engine used for its BINARY collation instead of the database locale.
--   * Local-day boundaries are never guessed: `observability.rollup_local_day_end()`
--     takes the zone from the `app.local_timezone` setting, which the server must set on
--     every pooled connection from the same timezone contract Rust computes its local
--     day thresholds with. An unset setting aborts loudly instead of silently using a
--     different day boundary.
--
-- Nested trigger firing: the retired engine did not fire triggers for changes made by a
-- trigger, PostgreSQL does. Both request_logs rollup functions therefore return early
-- unless `pg_trigger_depth() = 1` (a top-level trigger call), so the request_kind
-- canonicalisation update and any other trigger-owned write cannot manufacture an extra
-- source-revision bump or reaudit row. The guard has to live in the function body: a
-- trigger `WHEN` clause sees the depth before the firing trigger increments it.

-- ########## shared helpers ##########

-- Byte payload -> text, without raising on payloads that are not valid UTF-8 text.
-- A payload that cannot be decoded is not JSON; the retired engine reported the same
-- verdict for the same bytes.
CREATE OR REPLACE FUNCTION observability.request_body_text(payload bytea)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
BEGIN
    IF payload IS NULL THEN
        RETURN NULL;
    END IF;
    RETURN convert_from(payload, 'UTF8');
EXCEPTION
    WHEN data_exception THEN
        RETURN NULL;
END;
$$;

COMMENT ON FUNCTION observability.request_body_text(bytea) IS
    'Decodes a request body as UTF-8 text; returns NULL for payloads that are not valid UTF-8 text and therefore cannot be JSON.';

-- Parsed request body, or NULL when the payload is not valid JSON.
CREATE OR REPLACE FUNCTION observability.request_body_document(payload bytea)
RETURNS jsonb
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
DECLARE
    body_text text;
BEGIN
    body_text := observability.request_body_text(payload);
    IF body_text IS NULL OR NOT pg_input_is_valid(body_text, 'jsonb') THEN
        RETURN NULL;
    END IF;
    RETURN body_text::jsonb;
END;
$$;

COMMENT ON FUNCTION observability.request_body_document(bytea) IS
    'Request body as jsonb; NULL when the payload is absent or is not valid JSON.';

-- Label for one canonical request kind. Mirrors canonical_request_kind_label_sql().
CREATE OR REPLACE FUNCTION observability.request_kind_label(kind text)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT CASE
        WHEN lower(trim(coalesce(kind, ''))) = 'api:search' THEN 'API | search'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:extract' THEN 'API | extract'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:crawl' THEN 'API | crawl'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:map' THEN 'API | map'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:research' THEN 'API | research'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:research-result' THEN 'API | research result'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:usage' THEN 'API | usage'
        WHEN lower(trim(coalesce(kind, ''))) = 'api:unknown-path' THEN 'API | unknown path'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:search' THEN 'MCP | search'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:extract' THEN 'MCP | extract'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:crawl' THEN 'MCP | crawl'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:map' THEN 'MCP | map'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:research' THEN 'MCP | research'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:batch' THEN 'MCP | batch'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:initialize' THEN 'MCP | initialize'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:ping' THEN 'MCP | ping'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:tools/list' THEN 'MCP | tools/list'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:session-delete-unsupported'
            THEN 'MCP | session delete unsupported'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:unsupported-path'
            THEN 'MCP | unsupported path'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:unknown-payload'
            THEN 'MCP | unknown payload'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:unknown-method'
            THEN 'MCP | unknown method'
        WHEN lower(trim(coalesce(kind, ''))) = 'mcp:third-party-tool'
            THEN 'MCP | third-party tool'
        WHEN lower(trim(coalesce(kind, ''))) LIKE 'mcp:resources/%'
            OR lower(trim(coalesce(kind, ''))) LIKE 'mcp:prompts/%'
            OR lower(trim(coalesce(kind, ''))) LIKE 'mcp:notifications/%'
            THEN 'MCP | ' || substr(lower(trim(coalesce(kind, ''))), 5)
        ELSE trim(coalesce(kind, ''))
    END;
$$;

-- Predicate for a stored kind that the canonicalisation pass has already produced.
-- Mirrors canonical_request_kind_stored_predicate_sql().
CREATE OR REPLACE FUNCTION observability.request_kind_is_canonical(kind text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT coalesce(kind, '') IN (
               'api:search',
               'api:extract',
               'api:crawl',
               'api:map',
               'api:research',
               'api:research-result',
               'api:usage',
               'api:unknown-path',
               'mcp:search',
               'mcp:extract',
               'mcp:crawl',
               'mcp:map',
               'mcp:research',
               'mcp:batch',
               'mcp:initialize',
               'mcp:ping',
               'mcp:tools/list',
               'mcp:session-delete-unsupported',
               'mcp:unsupported-path',
               'mcp:unknown-payload',
               'mcp:unknown-method',
               'mcp:third-party-tool'
           )
        OR coalesce(kind, '') ILIKE 'mcp:resources/%'
        OR coalesce(kind, '') ILIKE 'mcp:prompts/%'
        OR coalesce(kind, '') ILIKE 'mcp:notifications/%';
$$;

-- Predicate for a kind that still needs canonicalisation.
-- Mirrors legacy_request_kind_stored_predicate_sql().
CREATE OR REPLACE FUNCTION observability.request_kind_is_legacy(kind text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT coalesce(kind, '') = ''
        OR coalesce(kind, '') ILIKE 'api:raw:%'
        OR coalesce(kind, '') ILIKE 'mcp:tool:%'
        OR coalesce(kind, '') = 'mcp:tools/call'
        OR coalesce(kind, '') ILIKE 'mcp:raw:%'
        OR (
            coalesce(kind, '') ILIKE 'api:%'
            AND NOT observability.request_kind_is_canonical(kind)
        )
        OR (
            coalesce(kind, '') ILIKE 'mcp:%'
            AND NOT observability.request_kind_is_canonical(kind)
        );
$$;

-- Kind derived from one MCP JSON-RPC message. Returns NULL when the message carries no
-- recognisable method, which the array branch treats as an unknown payload.
-- Mirrors mcp_message_request_kind_sql().
CREATE OR REPLACE FUNCTION observability.mcp_message_request_kind(document jsonb)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT CASE
        WHEN method IN ('initialize', 'ping', 'tools/list') THEN 'mcp:' || method
        WHEN method LIKE 'resources/%'
            OR method LIKE 'prompts/%'
            OR method LIKE 'notifications/%'
            THEN 'mcp:' || method
        WHEN method = 'tools/call' AND tool_name IN (
                'tavily-search',
                'tavily_search',
                'tavily_extract',
                'tavily-extract',
                'tavily-crawl',
                'tavily_crawl',
                'tavily_map',
                'tavily-map',
                'tavily-research',
                'tavily_research'
            )
            THEN CASE replace(tool_name, '_', '-')
                WHEN 'tavily-search' THEN 'mcp:search'
                WHEN 'tavily-extract' THEN 'mcp:extract'
                WHEN 'tavily-crawl' THEN 'mcp:crawl'
                WHEN 'tavily-map' THEN 'mcp:map'
                WHEN 'tavily-research' THEN 'mcp:research'
                ELSE 'mcp:unknown-payload'
            END
        WHEN method = 'tools/call' AND tool_name <> '' THEN 'mcp:third-party-tool'
        WHEN method = 'tools/call' THEN 'mcp:unknown-payload'
        WHEN method <> '' THEN 'mcp:unknown-method'
        ELSE NULL
    END
    FROM (
        SELECT lower(coalesce(nullif(document #>> '{method}', ''), '')) AS method,
               lower(coalesce(nullif(document #>> '{params,name}', ''), '')) AS tool_name
    ) AS fields;
$$;

-- Kind derived from the stored token-log style kind or the request path.
-- Mirrors token_log_stored_kind_sql().
CREATE OR REPLACE FUNCTION observability.request_kind_stored_fallback(
    request_path text,
    stored_kind text
)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT CASE
        WHEN path = '/api/tavily/search' THEN 'api:search'
        WHEN path = '/api/tavily/extract' THEN 'api:extract'
        WHEN path = '/api/tavily/crawl' THEN 'api:crawl'
        WHEN path = '/api/tavily/map' THEN 'api:map'
        WHEN path = '/api/tavily/research' THEN 'api:research'
        WHEN path = '/api/tavily/usage' THEN 'api:usage'
        WHEN path LIKE '/api/tavily/research/%' THEN 'api:research-result'
        WHEN path LIKE '/mcp/%' THEN 'mcp:unsupported-path'
        WHEN stored_key IN (
                'api:search',
                'api:extract',
                'api:crawl',
                'api:map',
                'api:research',
                'api:research-result',
                'api:usage',
                'api:unknown-path',
                'mcp:search',
                'mcp:extract',
                'mcp:crawl',
                'mcp:map',
                'mcp:research',
                'mcp:batch',
                'mcp:initialize',
                'mcp:ping',
                'mcp:tools/list',
                'mcp:session-delete-unsupported',
                'mcp:unsupported-path',
                'mcp:unknown-payload',
                'mcp:unknown-method',
                'mcp:third-party-tool'
            )
            OR stored_key LIKE 'mcp:resources/%'
            OR stored_key LIKE 'mcp:prompts/%'
            OR stored_key LIKE 'mcp:notifications/%'
            THEN stored_key
        WHEN stored_key LIKE 'api:raw:%'
            OR (path NOT LIKE '/mcp%' AND path NOT LIKE '/api/tavily/%')
            THEN 'api:unknown-path'
        WHEN stored_key LIKE 'mcp:tool:%' THEN 'mcp:third-party-tool'
        WHEN stored_key = 'mcp:tools/call' OR stored_key LIKE 'mcp:raw:%'
            THEN 'mcp:unknown-payload'
        WHEN stored_key LIKE 'mcp:%' AND path = '/mcp' THEN 'mcp:unknown-method'
        WHEN path = '/mcp' THEN 'mcp:unknown-payload'
        ELSE 'api:unknown-path'
    END
    FROM (
        SELECT lower(coalesce(request_path, '')) AS path,
               lower(trim(coalesce(stored_kind, ''))) AS stored_key
    ) AS fields;
$$;

-- Canonical kind for a request_logs row.
-- Mirrors request_log_request_kind_key_sql().
CREATE OR REPLACE FUNCTION observability.request_kind_canonical(
    request_path text,
    payload bytea,
    stored_kind text
)
RETURNS text
LANGUAGE plpgsql
IMMUTABLE
PARALLEL SAFE
AS $$
DECLARE
    normalized_path text := lower(coalesce(request_path, ''));
    document jsonb := observability.request_body_document(payload);
    array_item_count bigint;
    array_null_items bigint;
    array_distinct_kinds bigint;
    array_single_kind text;
    document_kind text;
BEGIN
    IF normalized_path = '/mcp' AND document IS NOT NULL THEN
        CASE jsonb_typeof(document)
            WHEN 'object' THEN
                document_kind := observability.mcp_message_request_kind(document);
                RETURN coalesce(document_kind, 'mcp:unknown-payload');
            WHEN 'array' THEN
                SELECT count(*),
                       count(*) FILTER (WHERE item_kind IS NULL),
                       count(DISTINCT item_kind),
                       min(item_kind COLLATE "C")
                  INTO array_item_count, array_null_items, array_distinct_kinds, array_single_kind
                  FROM (
                      SELECT observability.mcp_message_request_kind(item) AS item_kind
                        FROM jsonb_array_elements(document) AS item
                  ) AS items;
                IF array_item_count = 0 OR array_null_items > 0 THEN
                    RETURN 'mcp:unknown-payload';
                END IF;
                IF array_distinct_kinds = 1 THEN
                    RETURN array_single_kind;
                END IF;
                RETURN 'mcp:batch';
            ELSE
                RETURN observability.request_kind_stored_fallback(normalized_path, stored_kind);
        END CASE;
    END IF;
    RETURN observability.request_kind_stored_fallback(normalized_path, stored_kind);
END;
$$;

-- Detail column for the canonicalised row. Mirrors the detail expression in
-- ensure_request_log_catalog_rollup_schema().
CREATE OR REPLACE FUNCTION observability.request_kind_detail(
    request_path text,
    stored_kind text,
    canonical_kind text
)
RETURNS text
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT CASE
        WHEN lower(coalesce(request_path, '')) LIKE '/mcp/%' THEN request_path
        WHEN lower(trim(coalesce(stored_kind, ''))) LIKE 'mcp:tool:%'
            THEN substr(trim(stored_kind), 10)
        WHEN canonical_kind = 'mcp:unknown-payload' THEN request_path
        ELSE NULL
    END;
$$;

-- End of the local day that contains `day_start`, as a UTC epoch second.
--
-- The zone is an explicit parameter: the server sets `app.local_timezone` on every
-- pooled connection from the same timezone contract Rust uses for its local-day
-- thresholds. An unset setting raises instead of falling back to a different boundary.
CREATE OR REPLACE FUNCTION observability.rollup_local_day_end(day_start bigint)
RETURNS bigint
LANGUAGE sql
STABLE
AS $$
    SELECT extract(epoch FROM (
        (
            to_timestamp(day_start) AT TIME ZONE current_setting('app.local_timezone')
            + interval '1 day'
        ) AT TIME ZONE current_setting('app.local_timezone')
    ))::bigint;
$$;

-- ########## core: upstream_reconciliation_work maintenance ##########
--
-- The five reconciliation triggers keep one aggregated work row per
-- (token_id, period_code). Concurrency: every writer of upstream_reconciliation_usage
-- takes the row lock on the matching work row first (see src/store/pg/locks.rs,
-- LockScope::reconciliation_work), so the read-modify-write below is serialised per
-- work identity; the upserts themselves are atomic on the work primary key.

CREATE OR REPLACE FUNCTION core.upstream_reconciliation_usage_work_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- The retried MIN/MAX merge keeps the byte-order identity the retired engine used;
    -- every merged column is NOT NULL, so LEAST/GREATEST matches scalar MIN/MAX.
    INSERT INTO core.upstream_reconciliation_work (
        token_id,
        period_code,
        project_id,
        billing_subject,
        settlement_mode,
        period_start,
        period_end,
        scheduling_key_id,
        updated_at,
        work_generation,
        completed_generation,
        next_attempt_at,
        last_outcome
    ) VALUES (
        NEW.token_id,
        NEW.period_code,
        NEW.project_id,
        NEW.billing_subject,
        NEW.settlement_mode,
        NEW.period_start,
        NEW.period_end,
        NEW.key_id,
        NEW.updated_at,
        1,
        0,
        0,
        NULL
    )
    ON CONFLICT (token_id, period_code) DO UPDATE SET
        project_id = LEAST(
            upstream_reconciliation_work.project_id COLLATE "C",
            excluded.project_id COLLATE "C"
        ),
        billing_subject = LEAST(
            upstream_reconciliation_work.billing_subject COLLATE "C",
            excluded.billing_subject COLLATE "C"
        ),
        settlement_mode = LEAST(
            upstream_reconciliation_work.settlement_mode COLLATE "C",
            excluded.settlement_mode COLLATE "C"
        ),
        period_start = LEAST(
            upstream_reconciliation_work.period_start,
            excluded.period_start
        ),
        period_end = GREATEST(
            upstream_reconciliation_work.period_end,
            excluded.period_end
        ),
        scheduling_key_id = LEAST(
            upstream_reconciliation_work.scheduling_key_id COLLATE "C",
            excluded.scheduling_key_id COLLATE "C"
        ),
        updated_at = GREATEST(
            upstream_reconciliation_work.updated_at,
            excluded.updated_at
        ),
        work_generation = upstream_reconciliation_work.work_generation + 1,
        next_attempt_at = 0,
        last_outcome = NULL;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION core.upstream_reconciliation_usage_work_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    identity_changed boolean := NEW.token_id IS DISTINCT FROM OLD.token_id
        OR NEW.period_code IS DISTINCT FROM OLD.period_code;
BEGIN
    -- Rebuild the aggregate for the updated identity from every usage row that is still
    -- present, including the row this trigger fired for.
    INSERT INTO core.upstream_reconciliation_work (
        token_id,
        period_code,
        project_id,
        billing_subject,
        settlement_mode,
        period_start,
        period_end,
        scheduling_key_id,
        updated_at,
        work_generation,
        completed_generation,
        next_attempt_at,
        last_outcome
    )
    SELECT
        NEW.token_id,
        NEW.period_code,
        min(u.project_id COLLATE "C"),
        min(u.billing_subject COLLATE "C"),
        min(u.settlement_mode COLLATE "C"),
        min(u.period_start),
        max(u.period_end),
        min(u.key_id COLLATE "C"),
        max(u.updated_at),
        1,
        0,
        0,
        NULL
      FROM core.upstream_reconciliation_usage AS u
     WHERE u.token_id = NEW.token_id
       AND u.period_code = NEW.period_code
    ON CONFLICT (token_id, period_code) DO UPDATE SET
        project_id = excluded.project_id,
        billing_subject = excluded.billing_subject,
        settlement_mode = excluded.settlement_mode,
        period_start = excluded.period_start,
        period_end = excluded.period_end,
        scheduling_key_id = excluded.scheduling_key_id,
        updated_at = GREATEST(
            upstream_reconciliation_work.updated_at,
            excluded.updated_at
        ),
        work_generation = upstream_reconciliation_work.work_generation + 1,
        next_attempt_at = 0,
        last_outcome = NULL;

    -- A moved source identity also leaves the old work row behind; re-aggregate it from
    -- the rows that remain so it cannot keep the identity of the moved usage row.
    UPDATE core.upstream_reconciliation_work AS work
       SET project_id = (
               SELECT min(u.project_id COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           billing_subject = (
               SELECT min(u.billing_subject COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           settlement_mode = (
               SELECT min(u.settlement_mode COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           period_start = (
               SELECT min(u.period_start)
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           period_end = (
               SELECT max(u.period_end)
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           scheduling_key_id = (
               SELECT min(u.key_id COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           updated_at = GREATEST(
               work.updated_at,
               coalesce(
                   (
                       SELECT max(u.updated_at)
                         FROM core.upstream_reconciliation_usage AS u
                        WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
                   ),
                   work.updated_at
               )
           )
     WHERE identity_changed
       AND work.token_id = OLD.token_id
       AND work.period_code = OLD.period_code
       AND EXISTS (
               SELECT 1
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           );

    UPDATE core.upstream_reconciliation_work AS work
       SET work_generation = work.work_generation + 1,
           next_attempt_at = 0,
           last_outcome = NULL
     WHERE identity_changed
       AND work.token_id = OLD.token_id
       AND work.period_code = OLD.period_code;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION core.upstream_reconciliation_usage_work_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- The durable work row stays for audit/recovery; only its identity and scheduling
    -- state are recomputed from the usage rows that survived the delete.
    UPDATE core.upstream_reconciliation_work AS work
       SET project_id = (
               SELECT min(u.project_id COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           billing_subject = (
               SELECT min(u.billing_subject COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           settlement_mode = (
               SELECT min(u.settlement_mode COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           period_start = (
               SELECT min(u.period_start)
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           period_end = (
               SELECT max(u.period_end)
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           scheduling_key_id = (
               SELECT min(u.key_id COLLATE "C")
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           ),
           updated_at = GREATEST(
               work.updated_at,
               coalesce(
                   (
                       SELECT max(u.updated_at)
                         FROM core.upstream_reconciliation_usage AS u
                        WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
                   ),
                   work.updated_at
               )
           ),
           work_generation = work.work_generation + 1,
           next_attempt_at = 0,
           last_outcome = NULL
     WHERE work.token_id = OLD.token_id
       AND work.period_code = OLD.period_code
       AND EXISTS (
               SELECT 1
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           );

    UPDATE core.upstream_reconciliation_work AS work
       SET work_generation = work.work_generation + 1,
           next_attempt_at = 0,
           last_outcome = NULL
     WHERE work.token_id = OLD.token_id
       AND work.period_code = OLD.period_code
       AND NOT EXISTS (
               SELECT 1
                 FROM core.upstream_reconciliation_usage AS u
                WHERE u.token_id = OLD.token_id AND u.period_code = OLD.period_code
           );
    RETURN NULL;
END;
$$;

-- A new or updated usage row proves the upstream path works again, so the failure
-- streak and its retry deadline are cleared for that work identity.
CREATE OR REPLACE FUNCTION core.upstream_reconciliation_work_failure_reset()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE core.upstream_reconciliation_work
       SET transport_failure_streak = 0,
           transport_retry_at = 0,
           semantic_failure_streak = 0,
           semantic_retry_at = 0
     WHERE token_id = NEW.token_id
       AND period_code = NEW.period_code;
    RETURN NULL;
END;
$$;

-- ########## observability: request_kind canonicalisation ##########

CREATE OR REPLACE FUNCTION observability.request_logs_canonical_request_kind_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    canonical_kind text;
BEGIN
    IF NOT observability.request_kind_is_legacy(NEW.request_kind_key) THEN
        RETURN NULL;
    END IF;
    canonical_kind := observability.request_kind_canonical(
        NEW.path,
        NEW.request_body,
        NEW.request_kind_key
    );
    UPDATE observability.request_logs
       SET request_kind_key = coalesce(
               nullif(trim(canonical_kind), ''),
               'api:unknown-path'
           ),
           request_kind_label = coalesce(
               nullif(trim(observability.request_kind_label(canonical_kind)), ''),
               'Unknown'
           ),
           request_kind_detail = coalesce(
               nullif(trim(NEW.request_kind_detail), ''),
               observability.request_kind_detail(
                   NEW.path,
                   NEW.request_kind_key,
                   canonical_kind
               )
           )
     WHERE id = NEW.id
       AND observability.request_kind_is_legacy(request_kind_key);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION observability.request_logs_canonical_request_kind_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    canonical_kind text;
BEGIN
    IF NOT observability.request_kind_is_legacy(NEW.request_kind_key) THEN
        RETURN NULL;
    END IF;
    canonical_kind := observability.request_kind_canonical(
        NEW.path,
        NEW.request_body,
        NEW.request_kind_key
    );
    UPDATE observability.request_logs
       SET request_kind_key = coalesce(
               nullif(trim(canonical_kind), ''),
               'api:unknown-path'
           ),
           request_kind_label = coalesce(
               nullif(trim(observability.request_kind_label(canonical_kind)), ''),
               'Unknown'
           ),
           request_kind_detail = coalesce(
               nullif(trim(NEW.request_kind_detail), ''),
               observability.request_kind_detail(
                   NEW.path,
                   NEW.request_kind_key,
                   canonical_kind
               )
           )
     WHERE id = NEW.id
       AND observability.request_kind_is_legacy(request_kind_key);
    RETURN NULL;
END;
$$;

-- ########## observability: dashboard rollup source revisions ##########

CREATE OR REPLACE FUNCTION observability.dashboard_rollup_source_revision_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    observed_at bigint := (extract(epoch FROM now()))::bigint;
BEGIN
    -- Trigger-owned writes must not bump revisions, matching the retired engine's
    -- no-recursive-trigger default.
    IF pg_trigger_depth() > 1 THEN
        RETURN NULL;
    END IF;

    INSERT INTO observability.dashboard_rollup_source_revisions (bucket_start, revision)
    VALUES (OLD.created_at - (OLD.created_at % 300), 1)
    ON CONFLICT (bucket_start) DO UPDATE
       SET revision = observability.dashboard_rollup_source_revisions.revision
                      + excluded.revision;

    INSERT INTO observability.dashboard_rollup_source_revisions (bucket_start, revision)
    VALUES (
        NEW.created_at - (NEW.created_at % 300),
        CASE
            WHEN OLD.created_at - (OLD.created_at % 300)
                 = NEW.created_at - (NEW.created_at % 300)
                THEN 0
            ELSE 1
        END
    )
    ON CONFLICT (bucket_start) DO UPDATE
       SET revision = observability.dashboard_rollup_source_revisions.revision
                      + excluded.revision;

    INSERT INTO observability.dashboard_rollup_integrity_day_reaudits (
        bucket_start,
        bucket_end,
        cursor,
        status,
        updated_at,
        source_fence,
        gc_blocking
    )
    SELECT
        seal.bucket_start,
        observability.rollup_local_day_end(seal.bucket_start),
        seal.bucket_start,
        'pending',
        observed_at,
        NULL,
        1
      FROM observability.dashboard_rollup_daily_seals AS seal
     WHERE seal.bucket_start <= OLD.created_at
       AND observability.rollup_local_day_end(seal.bucket_start) > OLD.created_at
    ON CONFLICT (bucket_start) DO NOTHING;

    INSERT INTO observability.dashboard_rollup_integrity_day_reaudits (
        bucket_start,
        bucket_end,
        cursor,
        status,
        updated_at,
        source_fence,
        gc_blocking
    )
    SELECT
        seal.bucket_start,
        observability.rollup_local_day_end(seal.bucket_start),
        seal.bucket_start,
        'pending',
        observed_at,
        NULL,
        1
      FROM observability.dashboard_rollup_daily_seals AS seal
     WHERE seal.bucket_start <= NEW.created_at
       AND observability.rollup_local_day_end(seal.bucket_start) > NEW.created_at
    ON CONFLICT (bucket_start) DO NOTHING;

    UPDATE observability.dashboard_rollup_integrity_day_reaudits
       SET cursor = bucket_start,
           status = 'pending',
           updated_at = observed_at
     WHERE bucket_start <= OLD.created_at
       AND bucket_end > OLD.created_at;

    UPDATE observability.dashboard_rollup_integrity_day_reaudits
       SET cursor = bucket_start,
           status = 'pending',
           updated_at = observed_at
     WHERE bucket_start <= NEW.created_at
       AND bucket_end > NEW.created_at;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION observability.dashboard_rollup_source_revision_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    observed_at bigint := (extract(epoch FROM now()))::bigint;
BEGIN
    -- Deletion driven by the retention GC keeps its own bookkeeping; it must not bump
    -- source revisions. Nested trigger firing is suppressed as well, matching the
    -- retired engine's default of not firing triggers for trigger-owned writes.
    IF pg_trigger_depth() > 1 THEN
        RETURN NULL;
    END IF;
    IF EXISTS (
        SELECT 1
          FROM observability.dashboard_rollup_gc_delete_state
         WHERE id = 1
           AND active = 1
    ) THEN
        RETURN NULL;
    END IF;

    INSERT INTO observability.dashboard_rollup_source_revisions (bucket_start, revision)
    VALUES (OLD.created_at - (OLD.created_at % 300), 1)
    ON CONFLICT (bucket_start) DO UPDATE
       SET revision = observability.dashboard_rollup_source_revisions.revision
                      + excluded.revision;

    INSERT INTO observability.dashboard_rollup_integrity_day_reaudits (
        bucket_start,
        bucket_end,
        cursor,
        status,
        updated_at,
        source_fence,
        gc_blocking
    )
    SELECT
        seal.bucket_start,
        observability.rollup_local_day_end(seal.bucket_start),
        seal.bucket_start,
        'pending',
        observed_at,
        NULL,
        1
      FROM observability.dashboard_rollup_daily_seals AS seal
     WHERE seal.bucket_start <= OLD.created_at
       AND observability.rollup_local_day_end(seal.bucket_start) > OLD.created_at
    ON CONFLICT (bucket_start) DO NOTHING;

    UPDATE observability.dashboard_rollup_integrity_day_reaudits
       SET cursor = bucket_start,
           status = 'pending',
           updated_at = observed_at
     WHERE bucket_start <= OLD.created_at
       AND bucket_end > OLD.created_at;
    RETURN NULL;
END;
$$;

-- ########## trigger wiring ##########
--
-- PostgreSQL fires same-event triggers in name order. These names keep the order the
-- retired engine created them in: the work aggregate runs before the failure reset.

DROP TRIGGER IF EXISTS trg_upstream_reconciliation_usage_work_insert
    ON core.upstream_reconciliation_usage;
CREATE TRIGGER trg_upstream_reconciliation_usage_work_insert
    AFTER INSERT ON core.upstream_reconciliation_usage
    FOR EACH ROW
    EXECUTE FUNCTION core.upstream_reconciliation_usage_work_insert();

DROP TRIGGER IF EXISTS trg_upstream_reconciliation_usage_work_update
    ON core.upstream_reconciliation_usage;
CREATE TRIGGER trg_upstream_reconciliation_usage_work_update
    AFTER UPDATE ON core.upstream_reconciliation_usage
    FOR EACH ROW
    WHEN (
        NEW.token_id IS DISTINCT FROM OLD.token_id
        OR NEW.key_id IS DISTINCT FROM OLD.key_id
        OR NEW.period_code IS DISTINCT FROM OLD.period_code
        OR NEW.project_id IS DISTINCT FROM OLD.project_id
        OR NEW.billing_subject IS DISTINCT FROM OLD.billing_subject
        OR NEW.settlement_mode IS DISTINCT FROM OLD.settlement_mode
        OR NEW.period_start IS DISTINCT FROM OLD.period_start
        OR NEW.period_end IS DISTINCT FROM OLD.period_end
        OR NEW.request_count IS DISTINCT FROM OLD.request_count
        OR NEW.first_used_at IS DISTINCT FROM OLD.first_used_at
        OR NEW.last_used_at IS DISTINCT FROM OLD.last_used_at
    )
    EXECUTE FUNCTION core.upstream_reconciliation_usage_work_update();

DROP TRIGGER IF EXISTS trg_upstream_reconciliation_usage_work_delete
    ON core.upstream_reconciliation_usage;
CREATE TRIGGER trg_upstream_reconciliation_usage_work_delete
    AFTER DELETE ON core.upstream_reconciliation_usage
    FOR EACH ROW
    EXECUTE FUNCTION core.upstream_reconciliation_usage_work_delete();

DROP TRIGGER IF EXISTS trg_upstream_reconciliation_work_failure_reset_insert
    ON core.upstream_reconciliation_usage;
CREATE TRIGGER trg_upstream_reconciliation_work_failure_reset_insert
    AFTER INSERT ON core.upstream_reconciliation_usage
    FOR EACH ROW
    EXECUTE FUNCTION core.upstream_reconciliation_work_failure_reset();

DROP TRIGGER IF EXISTS trg_upstream_reconciliation_work_failure_reset_update
    ON core.upstream_reconciliation_usage;
CREATE TRIGGER trg_upstream_reconciliation_work_failure_reset_update
    AFTER UPDATE ON core.upstream_reconciliation_usage
    FOR EACH ROW
    EXECUTE FUNCTION core.upstream_reconciliation_work_failure_reset();

DROP TRIGGER IF EXISTS trg_request_logs_canonical_request_kind_insert
    ON observability.request_logs;
CREATE TRIGGER trg_request_logs_canonical_request_kind_insert
    AFTER INSERT ON observability.request_logs
    FOR EACH ROW
    EXECUTE FUNCTION observability.request_logs_canonical_request_kind_insert();

DROP TRIGGER IF EXISTS trg_request_logs_canonical_request_kind_update
    ON observability.request_logs;
CREATE TRIGGER trg_request_logs_canonical_request_kind_update
    AFTER UPDATE OF path, request_body, request_kind_key ON observability.request_logs
    FOR EACH ROW
    EXECUTE FUNCTION observability.request_logs_canonical_request_kind_update();

DROP TRIGGER IF EXISTS trg_dashboard_rollup_source_revision
    ON observability.request_logs;
CREATE TRIGGER trg_dashboard_rollup_source_revision
    AFTER UPDATE OF created_at,
        result_status,
        failure_kind,
        request_kind_key,
        request_body,
        path,
        business_credits,
        counts_business_quota,
        visibility ON observability.request_logs
    FOR EACH ROW
    EXECUTE FUNCTION observability.dashboard_rollup_source_revision_update();

DROP TRIGGER IF EXISTS trg_dashboard_rollup_source_revision_delete
    ON observability.request_logs;
CREATE TRIGGER trg_dashboard_rollup_source_revision_delete
    AFTER DELETE ON observability.request_logs
    FOR EACH ROW
    EXECUTE FUNCTION observability.dashboard_rollup_source_revision_delete();
