use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use crate::web::partials::ConversationInsightsData;

#[derive(Clone)]
pub struct CachedConversationInsights {
    pub data: ConversationInsightsData,
    pub expires_at: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub conversation_insights_cache: Arc<RwLock<HashMap<i64, CachedConversationInsights>>>,
}
