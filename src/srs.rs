use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use anyhow::{Context, bail};
use chrono::{TimeDelta, Utc};
use crossbeam_queue::ArrayQueue;
use dashmap::DashMap;
use fsrs::{DEFAULT_PARAMETERS, FSRS, MemoryState};
use futures_util::{SinkExt, stream::SplitSink};
use sqlx::{PgPool, Row};
use tokio::{net::TcpStream, sync::Mutex};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tracing::{debug, error, info};

use crate::sdk::layout_manager::LayoutManager;
use crate::sdk::{events::SystemEvent, layout_manager::DisplayRequest};
use crate::{
    models::{CardRating, Flashcard, FlashcardReviewNew},
    router::AppState,
    sdk::app_session::AppSession,
};
use anyhow::Result;
use serde_json::Value;

#[derive(Debug)]
pub struct UserSettings {
    max_cards_per_session: AtomicU8,
    desired_retention: AtomicU8,
}

impl UserSettings {
    pub fn new(max_cards_per_session: u8, desired_retention: u8) -> Self {
        Self {
            max_cards_per_session: AtomicU8::new(max_cards_per_session),
            desired_retention: AtomicU8::new(desired_retention),
        }
    }

    pub fn max_cards_per_session(&self) -> u8 {
        self.max_cards_per_session.load(Ordering::Relaxed)
    }

    pub fn desired_retention(&self) -> u8 {
        self.desired_retention.load(Ordering::Relaxed)
    }

    pub fn set_max_cards_per_session(&self, value: u8) {
        if value <= 100 && value > 0 {
            self.max_cards_per_session.store(value, Ordering::Relaxed);
        } else {
            error!("Invalid max cards per session: {}", value);
        }
    }

    pub fn set_desired_retention(&self, value: u8) {
        if value <= 100 && value > 0 {
            self.desired_retention.store(value, Ordering::Relaxed);
        } else {
            error!("Invalid desired retention: {}", value);
        }
    }
}

pub fn new_review(
    card: &Flashcard,
    rating: CardRating,
    desired_retention: f32,
) -> Result<FlashcardReviewNew> {
    let next_states = schedule_states(card, desired_retention)?;
    let next_state = match rating {
        CardRating::Easy => next_states.easy,
        CardRating::Good => next_states.good,
        CardRating::Difficult => next_states.hard,
        CardRating::Again => next_states.again,
    };
    let time_delta = TimeDelta::minutes((next_state.interval * 24.0 * 60.0) as i64);
    let reviewed = Utc::now().naive_utc();
    let scheduled = reviewed + time_delta;

    Ok(FlashcardReviewNew {
        flashcard_id: card.id,
        reviewed,
        scheduled,
        rating,
        stability: next_state.memory.stability,
        difficulty: next_state.memory.difficulty,
    })
}

fn schedule_states(card: &Flashcard, desired_retention: f32) -> Result<fsrs::NextStates> {
    let fsrs = FSRS::new(Some(&DEFAULT_PARAMETERS))?;

    let next_states = if card.last_reviewed.is_none() {
        // If no reviews, initialize with default memory state
        fsrs.next_states(None, desired_retention, 0)?
    } else {
        // Use the last review's memory state
        let current_memory_state = MemoryState::try_from(card)?;
        let last_review = card
            .last_reviewed
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Flashcard does not have a last review date"))?;
        let elapsed = (Utc::now().naive_utc() - *last_review).num_days() as u32;
        fsrs.next_states(Some(current_memory_state), desired_retention, elapsed)?
    };
    Ok(next_states)
}

pub(crate) type WebSocketSender =
    Option<Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>;

pub struct SessionState {
    cards: ArrayQueue<Flashcard>,
    deck_names: DashMap<i32, String>,
    started: AtomicBool,
    app_state: Arc<PgPool>,
    user_id: String,
    last_card: Arc<Mutex<Option<Flashcard>>>,
    user_settings: Arc<UserSettings>,
    sender: WebSocketSender,
    package_name: String,
    layout_manager: LayoutManager,
}

impl SessionState {
    /// Send a display request to AugmentOS Cloud
    pub async fn send_display_request(&self, display_request: &DisplayRequest) -> Result<()> {
        let display_json = serde_json::to_string(display_request)
            .context("Failed to serialize display request")?;
        debug!(
            "📺 [{}] Sending display request: {}",
            self.package_name, display_json
        );
        if let Some(sender) = &self.sender {
            let mut ws_sender = sender.lock().await;
            if let Err(e) = ws_sender.send(Message::Text(display_json.into())).await {
                bail!("Failed to send display request: {e}");
            }
            debug!("📺 [{}] Sent display request", self.package_name);
            Ok(())
        } else {
            bail!("WebSocket sender not available");
        }
    }
}

async fn next_card_or_finish(text: String, session_state: &SessionState) {
    info!("Next command: {text}");
    let display_request = if let Some(last_card) = session_state.cards.pop() {
        let deck_name = session_state
            .deck_names
            .get(&last_card.deck_id)
            .map(|d| d.to_string())
            .unwrap_or_default();
        let top_text = last_card.front.clone();
        session_state.last_card.lock().await.replace(last_card);
        session_state.layout_manager.show_double_text_wall(
            top_text,
            format!("{deck_name} ({} left)", session_state.cards.len()),
            None,
            None,
        )
    } else {
        info!("All cards reviewed");
        session_state.layout_manager.show_text_wall(
            "All cards reviewed! You can end the session in the Mentra app\ninterface.",
            None,
            None,
        )
    };
    if let Err(e) = session_state.send_display_request(&display_request).await {
        error!("Failed to send display request: {e}");
    }
}

async fn update_rating(
    card: &Flashcard,
    rating: CardRating,
    session_state: &SessionState,
) -> Result<()> {
    let update = new_review(
        card,
        rating,
        session_state.user_settings.desired_retention() as f32 / 100.0,
    )?;
    let flashcard = sqlx::query_as::<_, Flashcard>(
        r#"
        UPDATE flashcard 
        SET last_rating = $1, 
            last_reviewed = $2, 
            last_scheduled = $3,
            last_stability = $4,
            last_difficulty = $5
        WHERE id = $6 AND deck_id IN (SELECT id FROM deck WHERE user_id = $7)
        RETURNING *
        "#,
    )
    .bind(update.rating)
    .bind(update.reviewed)
    .bind(update.scheduled)
    .bind(update.stability)
    .bind(update.difficulty)
    .bind(update.flashcard_id)
    .bind(session_state.user_id.clone())
    .fetch_optional(&*session_state.app_state)
    .await?;
    if flashcard.is_none() {
        bail!("Flashcard not found or user not authorized");
    }
    Ok(())
}

async fn on_reveal(session_state: Arc<SessionState>) {
    if let Some(card) = session_state.last_card.lock().await.clone() {
        info!("Revealing card: {}", card.front);
        let display_request =
            session_state
                .layout_manager
                .show_double_text_wall(&card.front, card.back, None, None);
        if let Err(e) = session_state.send_display_request(&display_request).await {
            error!("Failed to send display request: {e}");
        }
    }
}

async fn on_transcription(text: String, session_state: Arc<SessionState>) -> Result<()> {
    let started = session_state.started.load(Ordering::Relaxed);
    info!("Received transcription: {}", text);
    let text = text.trim().to_lowercase();
    if started {
        // If already started, handle the transcription
        if text.contains("reveal") {
            on_reveal(session_state).await;
        } else if let Ok(rating) = text.parse::<CardRating>() {
            if let Some(card) = session_state.last_card.lock().await.clone() {
                info!("Rating card {} as {}", card.id, rating);
                // Here you would handle the rating logic
                if let Err(e) = update_rating(&card, rating, &session_state).await {
                    error!("Failed to update flashcard rating: {}", e);
                } else {
                    info!("Card {} rated as {}", card.id, rating);
                }
            }
            next_card_or_finish(text, &session_state).await;
        }
    } else if text.contains("start") {
        session_state.started.store(true, Ordering::Relaxed);
        info!(
            "Starting review session with {} cards",
            session_state.cards.len()
        );
        next_card_or_finish(text, &session_state).await;
    }
    Ok(())
}

fn update_user_settings(user_settings: Arc<UserSettings>, payload: &Value) {
    let mut new_max_cards_per_session = None;
    let mut new_desired_retention = None;
    if let Some(settings) = payload.as_array() {
        for setting in settings {
            extract_settings(
                &mut new_max_cards_per_session,
                &mut new_desired_retention,
                setting,
            );
        }
    }
    if let Some(max_cards) = new_max_cards_per_session {
        user_settings.set_max_cards_per_session(max_cards as u8);
    }
    if let Some(retention) = new_desired_retention {
        user_settings.set_desired_retention(retention as u8);
    }
}

pub(crate) fn extract_settings(
    new_max_cards_per_session: &mut Option<u64>,
    new_desired_retention: &mut Option<u64>,
    setting: &Value,
) {
    if let Some(key) = setting.get("key").and_then(|k| k.as_str()) {
        if key == "max_cards_per_session" {
            *new_max_cards_per_session = setting
                .get("value")
                .and_then(|v| v.as_u64())
                .filter(|x| *x > 0 && *x <= 100);
        } else if key == "desired_retention" {
            *new_desired_retention = setting
                .get("value")
                .and_then(|v| v.as_u64())
                .filter(|x| *x > 0 && *x <= 100);
        }
    }
}

async fn get_cards(
    db: Arc<PgPool>,
    user_id: &str,
    limit: usize,
) -> Result<(DashMap<i32, String>, ArrayQueue<Flashcard>)> {
    let deck_names = sqlx::query(
        r#"
            SELECT id, name FROM deck WHERE user_id = $1
            "#,
    )
    .bind(user_id)
    .fetch_all(&*db)
    .await?;

    let deck_names = deck_names
        .into_iter()
        .map(|row| {
            let id: i32 = row.get("id");
            let name: String = row.get("name");
            (id, name)
        })
        .collect::<DashMap<_, _>>();

    // Fetch flashcards ordered by scheduled time (with null being first)
    // limited to `limit`
    let flashcards = sqlx::query_as::<_, Flashcard>(
        r#"
            SELECT * FROM flashcard
            WHERE deck_id IN (SELECT id FROM deck WHERE user_id = $1)
            AND last_scheduled <= NOW() OR last_scheduled IS NULL
            ORDER BY last_scheduled NULLS LAST, id
            LIMIT $2
            "#,
    )
    .bind(user_id)
    .bind(limit as i64)
    .fetch_all(&*db)
    .await?;
    let cards = ArrayQueue::new(100);
    for card in flashcards {
        cards.force_push(card);
    }

    Ok((deck_names, cards))
}

async fn on_init(session_state: Arc<SessionState>) {
    let text = if session_state.cards.is_empty() {
        "No flashcards scheduled for review now.\nPlease add flashcards in the Mentra app interface.".to_string()
    } else {
        let card_count = if session_state.cards.len() == 1 {
            "1 card".to_string()
        } else {
            format!("{} cards", session_state.cards.len())
        };
        format!(
            "{card_count} for review. Say 'start' to begin.\nLook up or say 'reveal' to display the back answer on each card.\nSay 'easy', 'good', 'difficult', or 'again'\nto rate your card memorization."
        )
    };
    // Create DisplayRequest matching the Rust DisplayRequest structure
    let display_request = session_state
        .layout_manager
        .show_text_wall(text, None, None);

    if let Err(e) = session_state.send_display_request(&display_request).await {
        error!("Error sending display request: {e}");
    }
}

impl AppState {
    /// Called when a new session is created and connected
    pub async fn on_session(
        &self,
        session: &AppSession,
        session_id: &str,
        user_id: &str,
    ) -> Result<()> {
        info!(
            "🚀 Default session handling for session {} and user {}",
            session_id, user_id
        );

        // Subscribe to some default streams
        session
            .subscribe_to_streams(vec![
                "transcription:en-US".to_string(),
                "button_press".to_string(),
                "head_position".to_string(),
            ])
            .await
            .map_err(|e| {
                error!("Failed to subscribe to streams: {}", e);
                e
            })?;

        let (deck_names, cards) = get_cards(
            self.db.clone(),
            user_id,
            session.user_settings.max_cards_per_session() as usize,
        )
        .await?;
        if cards.is_empty() {
            session
                .show_text(
                    "No flashcards scheduled for review now.\nPlease add flashcards in the Mentra app interface.",
                    None,
                )
                .await?;
        } else {
            let card_count = if cards.len() == 1 {
                "1 card".to_string()
            } else {
                format!("{} cards", cards.len())
            };
            session.show_text(format!("{card_count} for review. Say 'start' to begin.\nSay 'reveal' to display the back answer on each card.\nSay 'easy', 'good', 'difficult', or 'again'\nto rate your card memorization."), None).await?;
        }

        let sender_clone = session.websocket_sender.clone();
        let session_state = Arc::new(SessionState {
            cards,
            deck_names,
            started: AtomicBool::new(false),
            app_state: self.db.clone(),
            user_id: user_id.to_string(),
            last_card: Arc::new(Mutex::new(None)),
            user_settings: session.user_settings.clone(),
            sender: sender_clone,
            package_name: session.package_name.clone(),
            layout_manager: LayoutManager::new(
                session.package_name.clone(),
                session_id.to_string(),
            ),
        });
        let user_settings: Arc<UserSettings> = session.user_settings.clone();
        let session_state_in = session_state.clone();
        let db = self.db.clone();
        session.events().on_system("connected", move |event| {
            if let SystemEvent::Connected(Some(settings)) = event {
                update_user_settings(user_settings.clone(), settings);
                let session_state_in = session_state_in.clone();
                let db = db.clone();
                Self::refetch_cards_initial_change(session_state_in, db);
            }
        });
        let user_settings: Arc<UserSettings> = session.user_settings.clone();
        let session_state_in = session_state.clone();
        let db = self.db.clone();
        session.events().on_system("settings_update", move |event| {
            if let SystemEvent::SettingsUpdate(settings) = event {
                update_user_settings(user_settings.clone(), settings);
                let session_state_in = session_state_in.clone();
                let db = db.clone();
                Self::refetch_cards_initial_change(session_state_in, db);
            }
        });
        let session_state_in = session_state.clone();
        session.events().on_head_position(move |head_position| {
            info!("Received head position: {:?}", head_position);
            if head_position.position.to_lowercase().contains("up") {
                tokio::spawn(on_reveal(session_state_in.clone()));
            }
        });
        let session_state_in = session_state.clone();
        session.events().on_button_press(move |button_press| {
            info!("Received button press: {:?}", button_press);
            tokio::spawn(on_reveal(session_state_in.clone()));
        });
        session.events().on_transcription(move |transcription| {
            info!(
                "🎤 Received transcription: {} (final: {})",
                transcription.text, transcription.is_final
            );

            // Send the transcription text back to the client using display_event
            let text = transcription.text.clone();
            let session_state: Arc<SessionState> = session_state.clone();
            if transcription.is_final {
                tokio::spawn(async move {
                    let session_state = session_state;
                    if let Err(e) = on_transcription(text, session_state.clone()).await {
                        error!("Failed to process transcription: {}", e);
                    }
                });
            }
        });
        // Default implementation - can be overridden
        Ok(())
    }

    fn refetch_cards_initial_change(session_state_in: Arc<SessionState>, db: Arc<PgPool>) {
        if !session_state_in.started.load(Ordering::Relaxed)
            && !session_state_in.cards.is_empty()
            && session_state_in.cards.len()
                != session_state_in.user_settings.max_cards_per_session() as usize
        {
            let db = db.clone();
            let session_state_in = session_state_in.clone();

            tokio::spawn(async move {
                match get_cards(
                    db.clone(),
                    &session_state_in.user_id,
                    session_state_in.user_settings.max_cards_per_session() as usize,
                )
                .await
                {
                    Ok((deck_names, cards)) => {
                        while !session_state_in.cards.is_empty() {
                            let _ = session_state_in.cards.pop().is_some();
                        }
                        for card in cards {
                            session_state_in.cards.force_push(card);
                        }
                        session_state_in.deck_names.clear();
                        for (id, name) in deck_names {
                            session_state_in.deck_names.insert(id, name);
                        }
                        info!("Updated session state with new cards and deck names");
                        on_init(session_state_in).await;
                    }
                    Err(e) => {
                        error!("Failed to fetch cards: {}", e);
                    }
                }
            });
        }
    }
}
