impl TavilyProxy {
    pub async fn list_announcements(&self) -> Result<Vec<Announcement>, ProxyError> {
        self.key_store.list_announcements().await
    }

    pub async fn create_announcement(
        &self,
        input: AnnouncementMutation,
    ) -> Result<Announcement, ProxyError> {
        self.key_store.create_announcement(input).await
    }

    pub async fn update_announcement(
        &self,
        id: &str,
        input: AnnouncementMutation,
    ) -> Result<Option<Announcement>, ProxyError> {
        self.key_store.update_announcement(id, input).await
    }

    pub async fn publish_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        self.key_store.publish_announcement(id).await
    }

    pub async fn archive_announcement(&self, id: &str) -> Result<Option<Announcement>, ProxyError> {
        self.key_store.archive_announcement(id).await
    }

    pub async fn user_active_announcements(&self) -> Result<Vec<Announcement>, ProxyError> {
        self.key_store.list_user_active_announcements().await
    }

    pub async fn user_announcement_history(&self) -> Result<Vec<Announcement>, ProxyError> {
        self.key_store.list_user_announcement_history().await
    }
}
