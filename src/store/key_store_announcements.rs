const ANNOUNCEMENT_ID_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";

fn normalize_announcement_text(value: &str, field: &str, max_len: usize) -> Result<String, ProxyError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ProxyError::Other(format!("{field} is required")));
    }
    if trimmed.chars().count() > max_len {
        return Err(ProxyError::Other(format!("{field} is too long")));
    }
    Ok(trimmed.to_string())
}

fn normalize_announcement_body(value: &str, display_kind: &str) -> Result<String, ProxyError> {
    let trimmed = value.trim();
    if trimmed.is_empty() && display_kind == ANNOUNCEMENT_DISPLAY_TICKER {
        return Ok(String::new());
    }
    normalize_announcement_text(value, "body", 4000)
}

fn normalize_announcement_display(value: &str) -> Result<String, ProxyError> {
    let normalized = value.trim().to_ascii_lowercase();
    if !is_supported_announcement_display(&normalized) {
        return Err(ProxyError::Other("unsupported announcement display kind".to_string()));
    }
    Ok(normalized)
}

fn normalize_announcement_mutation(input: AnnouncementMutation) -> Result<AnnouncementMutation, ProxyError> {
    let display_kind = normalize_announcement_display(&input.display_kind)?;
    Ok(AnnouncementMutation {
        title: normalize_announcement_text(&input.title, "title", 120)?,
        body: normalize_announcement_body(&input.body, &display_kind)?,
        display_kind,
    })
}

fn announcement_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Announcement, sqlx::Error> {
    Ok(Announcement {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        display_kind: row.try_get("display_kind")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        published_at: row.try_get("published_at")?,
        archived_at: row.try_get("archived_at")?,
    })
}

impl KeyStore {
    pub(crate) async fn ensure_announcements_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS announcements (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
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
                    id, title, body, display_kind, status,
                    created_at, updated_at, published_at, archived_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
                "#,
            )
            .bind(&id)
            .bind(&input.title)
            .bind(&input.body)
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
                        title: input.title,
                        body: input.body,
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
        self.insert_announcement_with_status(input, ANNOUNCEMENT_STATUS_DRAFT, Utc::now().timestamp())
            .await
    }

    pub(crate) async fn list_announcements(&self) -> Result<Vec<Announcement>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
                        id, title, body, display_kind, status,
                        created_at, updated_at, published_at, archived_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
                    "#,
                )
                .bind(&new_id)
                .bind(&input.title)
                .bind(&input.body)
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
                            title: input.title,
                            body: input.body,
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
                        id, title, body, display_kind, status,
                        created_at, updated_at, published_at, archived_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL)
                    "#,
                )
                .bind(&new_id)
                .bind(&input.title)
                .bind(&input.body)
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
                            title: input.title,
                            body: input.body,
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
                  SET title = ?, body = ?, display_kind = ?, updated_at = ?
                WHERE id = ?"#,
        )
        .bind(&input.title)
        .bind(&input.body)
        .bind(&input.display_kind)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(Some(Announcement {
            id: existing.id,
            title: input.title,
            body: input.body,
            display_kind: input.display_kind,
            status: existing.status,
            created_at: existing.created_at,
            updated_at: now,
            published_at: existing.published_at,
            archived_at: existing.archived_at,
        }))
    }

    pub(crate) async fn publish_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
                title: existing.title,
                body: existing.body,
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
                    id, title, body, display_kind, status,
                    created_at, updated_at, published_at, archived_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
                "#,
            )
            .bind(&new_id)
            .bind(&existing.title)
            .bind(&existing.body)
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
                        title: existing.title,
                        body: existing.body,
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
        let now = Utc::now().timestamp();
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
            SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
                SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
            SELECT id, title, body, display_kind, status, created_at, updated_at, published_at, archived_at
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
