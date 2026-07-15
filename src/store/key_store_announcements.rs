const ANNOUNCEMENT_ID_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
const ANNOUNCEMENT_CONTENT_MAX_LEN: usize = 4200;
const ANNOUNCEMENT_TITLE_MAX_LEN: usize = 120;

struct ParsedAnnouncementContent {
    title_markdown: Option<String>,
    body_markdown: String,
}

fn normalize_announcement_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn normalize_announcement_content(value: &str) -> Result<String, ProxyError> {
    let normalized = normalize_announcement_newlines(value);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Err(ProxyError::Other("content is required".to_string()));
    }
    if trimmed.chars().count() > ANNOUNCEMENT_CONTENT_MAX_LEN {
        return Err(ProxyError::Other("content is too long".to_string()));
    }
    Ok(trimmed.to_string())
}

fn parse_atx_heading(line: &str) -> Option<String> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return None;
    }
    let trimmed = &line[indent..];
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let title = rest
        .trim()
        .trim_end_matches('#')
        .trim_end()
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some(title)
}

fn is_setext_underline(line: &str) -> bool {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return false;
    }
    let trimmed = line[indent..].trim();
    if trimmed.is_empty() {
        return false;
    }
    if !trimmed.chars().all(|c| c == '=' || c == '-') {
        return false;
    }
    matches!(trimmed.chars().next(), Some('=') | Some('-'))
}

fn parse_announcement_content(value: &str) -> ParsedAnnouncementContent {
    let normalized = normalize_announcement_newlines(value);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return ParsedAnnouncementContent {
            title_markdown: None,
            body_markdown: String::new(),
        };
    }

    let lines: Vec<&str> = trimmed.split('\n').collect();
    let mut first_non_empty_index = 0;
    while first_non_empty_index < lines.len() && lines[first_non_empty_index].trim().is_empty() {
        first_non_empty_index += 1;
    }
    if first_non_empty_index >= lines.len() {
        return ParsedAnnouncementContent {
            title_markdown: None,
            body_markdown: String::new(),
        };
    }

    if let Some(title_markdown) = parse_atx_heading(lines[first_non_empty_index]) {
        return ParsedAnnouncementContent {
            title_markdown: Some(title_markdown),
            body_markdown: lines[first_non_empty_index + 1..].join("\n").trim().to_string(),
        };
    }

    if first_non_empty_index + 1 < lines.len()
        && !lines[first_non_empty_index].trim().is_empty()
        && is_setext_underline(lines[first_non_empty_index + 1])
    {
        return ParsedAnnouncementContent {
            title_markdown: Some(lines[first_non_empty_index].trim().to_string()),
            body_markdown: lines[first_non_empty_index + 2..].join("\n").trim().to_string(),
        };
    }

    ParsedAnnouncementContent {
        title_markdown: None,
        body_markdown: trimmed.to_string(),
    }
}

fn normalize_announcement_display(value: &str) -> Result<String, ProxyError> {
    let normalized = value.trim().to_ascii_lowercase();
    if !is_supported_announcement_display(&normalized) {
        return Err(ProxyError::Other(
            "unsupported announcement display kind".to_string(),
        ));
    }
    Ok(normalized)
}

fn normalize_announcement_mutation(input: AnnouncementMutation) -> Result<AnnouncementMutation, ProxyError> {
    let display_kind = normalize_announcement_display(&input.display_kind)?;
    let content = normalize_announcement_content(&input.content)?;
    let parsed = parse_announcement_content(&content);

    if let Some(title) = parsed.title_markdown.as_ref()
        && title.chars().count() > ANNOUNCEMENT_TITLE_MAX_LEN
    {
        return Err(ProxyError::Other("announcement title is too long".to_string()));
    }

    if display_kind == ANNOUNCEMENT_DISPLAY_MODAL {
        if parsed.title_markdown.is_none() {
            return Err(ProxyError::Other(
                "content must start with a markdown title".to_string(),
            ));
        }
        if parsed.body_markdown.is_empty() {
            return Err(ProxyError::Other(
                "modal announcements require body content after the leading title".to_string(),
            ));
        }
    }

    Ok(AnnouncementMutation {
        content,
        display_kind,
    })
}

fn announcement_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Announcement, sqlx::Error> {
    Ok(Announcement {
        id: row.try_get("id")?,
        content: row.try_get("content")?,
        display_kind: row.try_get("display_kind")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        published_at: row.try_get("published_at")?,
        archived_at: row.try_get("archived_at")?,
    })
}

impl KeyStore {
    async fn announcements_table_exists(&self) -> Result<bool, ProxyError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'announcements')",
        )
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    async fn announcement_columns_set(&self) -> Result<std::collections::HashSet<String>, ProxyError> {
        if !self.announcements_table_exists().await? {
            return Ok(std::collections::HashSet::new());
        }
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('announcements', 'main')")
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().collect())
            .map_err(ProxyError::Database)
    }

    async fn create_announcements_table_in_pool(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS announcements (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                display_kind TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                published_at INTEGER,
                archived_at INTEGER
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn legacy_content_sql(source_columns: &std::collections::HashSet<String>) -> String {
        let title_sql = if source_columns.contains("title") {
            "TRIM(COALESCE(legacy.title, ''))".to_string()
        } else {
            "''".to_string()
        };
        let body_sql = if source_columns.contains("body") {
            "TRIM(COALESCE(legacy.body, ''))".to_string()
        } else {
            "''".to_string()
        };
        let fallback_sql = format!(
            "CASE \
                WHEN {title_sql} = '' THEN {body_sql} \
                WHEN {body_sql} = '' THEN '# ' || {title_sql} \
                ELSE '# ' || {title_sql} || char(10) || char(10) || {body_sql} \
             END"
        );

        if source_columns.contains("content") {
            format!(
                "COALESCE(NULLIF(TRIM(COALESCE(legacy.content, '')), ''), {fallback_sql})"
            )
        } else {
            fallback_sql
        }
    }

    async fn rebuild_announcements_as_content_only(
        &self,
        source_columns: &std::collections::HashSet<String>,
    ) -> Result<(), ProxyError> {
        if source_columns.is_empty() {
            return self.create_announcements_table_in_pool().await;
        }

        let content_sql = Self::legacy_content_sql(source_columns);
        let display_sql = if source_columns.contains("display_kind") {
            "legacy.display_kind".to_string()
        } else {
            format!("'{}'", ANNOUNCEMENT_DISPLAY_MODAL)
        };
        let status_sql = if source_columns.contains("status") {
            "legacy.status".to_string()
        } else {
            format!("'{}'", ANNOUNCEMENT_STATUS_DRAFT)
        };
        let created_at_sql = if source_columns.contains("created_at") {
            "legacy.created_at".to_string()
        } else if source_columns.contains("updated_at") {
            "legacy.updated_at".to_string()
        } else {
            "CAST(strftime('%s', 'now') AS INTEGER)".to_string()
        };
        let updated_at_sql = if source_columns.contains("updated_at") {
            "legacy.updated_at".to_string()
        } else if source_columns.contains("created_at") {
            "legacy.created_at".to_string()
        } else {
            "CAST(strftime('%s', 'now') AS INTEGER)".to_string()
        };
        let published_at_sql = if source_columns.contains("published_at") {
            "legacy.published_at".to_string()
        } else {
            "NULL".to_string()
        };
        let archived_at_sql = if source_columns.contains("archived_at") {
            "legacy.archived_at".to_string()
        } else {
            "NULL".to_string()
        };

        let mut conn = self.pool.acquire().await?;
        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS announcements_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE announcements_new (
                    id TEXT PRIMARY KEY,
                    content TEXT NOT NULL,
                    display_kind TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    published_at INTEGER,
                    archived_at INTEGER
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            let copy_sql = format!(
                r#"
                INSERT INTO announcements_new (
                    id,
                    content,
                    display_kind,
                    status,
                    created_at,
                    updated_at,
                    published_at,
                    archived_at
                )
                SELECT
                    legacy.id,
                    {content_sql},
                    {display_sql},
                    {status_sql},
                    {created_at_sql},
                    {updated_at_sql},
                    {published_at_sql},
                    {archived_at_sql}
                FROM announcements AS legacy
                "#
            );
            sqlx::query(&copy_sql).execute(&mut *conn).await?;
            sqlx::query("DROP TABLE announcements")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE announcements_new RENAME TO announcements")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;

        if rebuild_result.is_err() {
            sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();
        }

        rebuild_result.map_err(ProxyError::Database)
    }

    pub(crate) async fn ensure_announcements_schema(&self) -> Result<(), ProxyError> {
        let source_columns = self.announcement_columns_set().await?;
        let has_target_schema =
            source_columns.contains("content") && !source_columns.contains("title") && !source_columns.contains("body");

        if source_columns.is_empty() {
            self.create_announcements_table_in_pool().await?;
        } else if !has_target_schema {
            self.rebuild_announcements_as_content_only(&source_columns)
                .await?;
        }

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_announcements_status_updated
               ON announcements(status, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_announcements_user_display_time
               ON announcements(display_kind, status, published_at DESC, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn new_announcement_id() -> String {
        random_string(ANNOUNCEMENT_ID_ALPHABET, 8)
    }

    async fn insert_announcement_with_status(
        &self,
        input: AnnouncementMutation,
        status: &str,
        now: i64,
    ) -> Result<Announcement, ProxyError> {
        let input = normalize_announcement_mutation(input)?;
        let published_at = (status == ANNOUNCEMENT_STATUS_PUBLISHED).then_some(now);
        loop {
            let id = Self::new_announcement_id();
            let res = sqlx::query(
                r#"
                INSERT INTO announcements (
                    id, content, display_kind, status,
                    created_at, updated_at, published_at, archived_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
                "#,
            )
            .bind(&id)
            .bind(&input.content)
            .bind(&input.display_kind)
            .bind(status)
            .bind(now)
            .bind(now)
            .bind(published_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    return Ok(Announcement {
                        id,
                        content: input.content,
                        display_kind: input.display_kind,
                        status: status.to_string(),
                        created_at: now,
                        updated_at: now,
                        published_at,
                        archived_at: None,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn create_announcement(
        &self,
        input: AnnouncementMutation,
    ) -> Result<Announcement, ProxyError> {
        self.insert_announcement_with_status(
            input,
            ANNOUNCEMENT_STATUS_DRAFT,
            self.backend_time.now_ts(),
        )
        .await
    }

    pub(crate) async fn list_announcements(&self) -> Result<Vec<Announcement>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
              FROM announcements
             ORDER BY
               CASE status
                 WHEN 'published' THEN 0
                 WHEN 'draft' THEN 1
                 ELSE 2
               END,
               COALESCE(published_at, updated_at, created_at) DESC,
               rowid DESC
            "#,
        )
        .try_map(announcement_from_row)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn update_announcement(
        &self,
        id: &str,
        input: AnnouncementMutation,
    ) -> Result<Option<Announcement>, ProxyError> {
        let input = normalize_announcement_mutation(input)?;
        let now = self.backend_time.now_ts();
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
              FROM announcements
             WHERE id = ?
            "#,
        )
        .bind(id)
        .try_map(announcement_from_row)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(existing) = existing else {
            tx.rollback().await?;
            return Ok(None);
        };

        if existing.status == ANNOUNCEMENT_STATUS_PUBLISHED {
            sqlx::query(
                r#"UPDATE announcements
                      SET status = ?, updated_at = ?, archived_at = ?
                    WHERE id = ?"#,
            )
            .bind(ANNOUNCEMENT_STATUS_ARCHIVED)
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;

            loop {
                let new_id = Self::new_announcement_id();
                let res = sqlx::query(
                    r#"
                    INSERT INTO announcements (
                        id, content, display_kind, status,
                        created_at, updated_at, published_at, archived_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
                    "#,
                )
                .bind(&new_id)
                .bind(&input.content)
                .bind(&input.display_kind)
                .bind(ANNOUNCEMENT_STATUS_PUBLISHED)
                .bind(now)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match res {
                    Ok(_) => {
                        tx.commit().await?;
                        return Ok(Some(Announcement {
                            id: new_id,
                            content: input.content,
                            display_kind: input.display_kind,
                            status: ANNOUNCEMENT_STATUS_PUBLISHED.to_string(),
                            created_at: now,
                            updated_at: now,
                            published_at: Some(now),
                            archived_at: None,
                        }));
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => return Err(ProxyError::Database(err)),
                }
            }
        }

        if existing.status == ANNOUNCEMENT_STATUS_ARCHIVED {
            loop {
                let new_id = Self::new_announcement_id();
                let res = sqlx::query(
                    r#"
                    INSERT INTO announcements (
                        id, content, display_kind, status,
                        created_at, updated_at, published_at, archived_at
                    ) VALUES (?, ?, ?, ?, ?, ?, NULL, NULL)
                    "#,
                )
                .bind(&new_id)
                .bind(&input.content)
                .bind(&input.display_kind)
                .bind(ANNOUNCEMENT_STATUS_DRAFT)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match res {
                    Ok(_) => {
                        tx.commit().await?;
                        return Ok(Some(Announcement {
                            id: new_id,
                            content: input.content,
                            display_kind: input.display_kind,
                            status: ANNOUNCEMENT_STATUS_DRAFT.to_string(),
                            created_at: now,
                            updated_at: now,
                            published_at: None,
                            archived_at: None,
                        }));
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => return Err(ProxyError::Database(err)),
                }
            }
        }

        sqlx::query(
            r#"UPDATE announcements
                  SET content = ?, display_kind = ?, updated_at = ?
                WHERE id = ?"#,
        )
        .bind(&input.content)
        .bind(&input.display_kind)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(Some(Announcement {
            id: existing.id,
            content: input.content,
            display_kind: input.display_kind,
            status: existing.status,
            created_at: existing.created_at,
            updated_at: now,
            published_at: existing.published_at,
            archived_at: existing.archived_at,
        }))
    }

    pub(crate) async fn publish_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
              FROM announcements
             WHERE id = ?
            "#,
        )
        .bind(id)
        .try_map(announcement_from_row)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(existing) = existing else {
            tx.rollback().await?;
            return Ok(None);
        };

        if existing.status == ANNOUNCEMENT_STATUS_PUBLISHED {
            tx.commit().await?;
            return Ok(Some(existing));
        }

        if existing.status == ANNOUNCEMENT_STATUS_DRAFT {
            let published_at = existing.published_at.or(Some(now));
            sqlx::query(
                r#"UPDATE announcements
                      SET status = ?, updated_at = ?, published_at = ?, archived_at = NULL
                    WHERE id = ?"#,
            )
            .bind(ANNOUNCEMENT_STATUS_PUBLISHED)
            .bind(now)
            .bind(published_at)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(Some(Announcement {
                id: existing.id,
                content: existing.content,
                display_kind: existing.display_kind,
                status: ANNOUNCEMENT_STATUS_PUBLISHED.to_string(),
                created_at: existing.created_at,
                updated_at: now,
                published_at,
                archived_at: None,
            }));
        }

        loop {
            let new_id = Self::new_announcement_id();
            let res = sqlx::query(
                r#"
                INSERT INTO announcements (
                    id, content, display_kind, status,
                    created_at, updated_at, published_at, archived_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
                "#,
            )
            .bind(&new_id)
            .bind(&existing.content)
            .bind(&existing.display_kind)
            .bind(ANNOUNCEMENT_STATUS_PUBLISHED)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;

            match res {
                Ok(_) => {
                    tx.commit().await?;
                    return Ok(Some(Announcement {
                        id: new_id,
                        content: existing.content,
                        display_kind: existing.display_kind,
                        status: ANNOUNCEMENT_STATUS_PUBLISHED.to_string(),
                        created_at: now,
                        updated_at: now,
                        published_at: Some(now),
                        archived_at: None,
                    }));
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn archive_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE announcements
                  SET status = ?, updated_at = ?, archived_at = COALESCE(archived_at, ?)
                WHERE id = ?
                  AND status != ?"#,
        )
        .bind(ANNOUNCEMENT_STATUS_ARCHIVED)
        .bind(now)
        .bind(now)
        .bind(id)
        .bind(ANNOUNCEMENT_STATUS_ARCHIVED)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if updated == 0 {
            return self.get_announcement(id).await;
        }
        self.get_announcement(id).await
    }

    pub(crate) async fn get_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        sqlx::query(
            r#"
            SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
              FROM announcements
             WHERE id = ?
            "#,
        )
        .bind(id)
        .try_map(announcement_from_row)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn list_user_active_announcements(&self) -> Result<Vec<Announcement>, ProxyError> {
        let mut out = Vec::new();
        for display_kind in [ANNOUNCEMENT_DISPLAY_MODAL, ANNOUNCEMENT_DISPLAY_TICKER] {
            if let Some(item) = sqlx::query(
                r#"
                SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
                  FROM announcements
                 WHERE status = ? AND display_kind = ?
                 ORDER BY COALESCE(published_at, updated_at, created_at) DESC, rowid DESC
                 LIMIT 1
                "#,
            )
            .bind(ANNOUNCEMENT_STATUS_PUBLISHED)
            .bind(display_kind)
            .try_map(announcement_from_row)
            .fetch_optional(&self.pool)
            .await?
            {
                out.push(item);
            }
        }
        Ok(out)
    }

    pub(crate) async fn list_user_announcement_history(&self) -> Result<Vec<Announcement>, ProxyError> {
        sqlx::query(
            r#"
            SELECT id, content, display_kind, status, created_at, updated_at, published_at, archived_at
              FROM announcements
             WHERE status IN (?, ?)
               AND (status != ? OR published_at IS NOT NULL)
             ORDER BY
               CASE
                 WHEN status = 'archived' THEN COALESCE(archived_at, published_at, updated_at, created_at)
                 ELSE COALESCE(published_at, updated_at, created_at)
               END DESC,
               rowid DESC
            "#,
        )
        .bind(ANNOUNCEMENT_STATUS_PUBLISHED)
        .bind(ANNOUNCEMENT_STATUS_ARCHIVED)
        .bind(ANNOUNCEMENT_STATUS_ARCHIVED)
        .try_map(announcement_from_row)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }
}
