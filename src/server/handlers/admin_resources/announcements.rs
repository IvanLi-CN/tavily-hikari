fn announcement_not_found() -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, "announcement not found".to_string())
}

fn announcement_response(item: tavily_hikari::Announcement) -> Json<AnnouncementView> {
    Json(AnnouncementView::from(item))
}

async fn get_announcements(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AnnouncementsResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let items = state
        .proxy
        .list_announcements()
        .await
        .map_err(|err| admin_proxy_error_response("list announcements error", err))?
        .into_iter()
        .map(AnnouncementView::from)
        .collect();
    Ok(Json(AnnouncementsResponse { items }))
}

async fn create_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AnnouncementMutationRequest>,
) -> Result<Json<AnnouncementView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    state
        .proxy
        .create_announcement(payload.into())
        .await
        .map(announcement_response)
        .map_err(|err| admin_proxy_error_response("create announcement error", err))
}

async fn update_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<AnnouncementMutationRequest>,
) -> Result<Json<AnnouncementView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let Some(item) = state
        .proxy
        .update_announcement(&id, payload.into())
        .await
        .map_err(|err| admin_proxy_error_response("update announcement error", err))?
    else {
        return Err(announcement_not_found());
    };
    Ok(announcement_response(item))
}

async fn publish_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<AnnouncementView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let Some(item) = state
        .proxy
        .publish_announcement(&id)
        .await
        .map_err(|err| admin_proxy_error_response("publish announcement error", err))?
    else {
        return Err(announcement_not_found());
    };
    Ok(announcement_response(item))
}

async fn archive_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<AnnouncementView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let Some(item) = state
        .proxy
        .archive_announcement(&id)
        .await
        .map_err(|err| admin_proxy_error_response("archive announcement error", err))?
    else {
        return Err(announcement_not_found());
    };
    Ok(announcement_response(item))
}

async fn get_user_announcements(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AnnouncementsResponse>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    if resolve_user_session(state.as_ref(), &headers).await.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let items = state
        .proxy
        .user_active_announcements()
        .await
        .map_err(|err| {
            eprintln!("get user announcements error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load announcements".to_string(),
            )
        })?
        .into_iter()
        .map(AnnouncementView::from)
        .collect();
    Ok(Json(AnnouncementsResponse { items }))
}

async fn get_user_announcement_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AnnouncementsResponse>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    if resolve_user_session(state.as_ref(), &headers).await.is_none() {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let items = state
        .proxy
        .user_announcement_history()
        .await
        .map_err(|err| {
            eprintln!("get user announcement history error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load announcements".to_string(),
            )
        })?
        .into_iter()
        .map(AnnouncementView::from)
        .collect();
    Ok(Json(AnnouncementsResponse { items }))
}
