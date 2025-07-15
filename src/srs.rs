use std::{collections::HashMap, sync::Arc};

use anyhow::bail;
use chrono::{TimeDelta, Utc};
use fsrs::{DEFAULT_PARAMETERS, FSRS, MemoryState};
use futures_util::{SinkExt, stream::SplitSink};
use sqlx::{PgPool, Row};
use tokio::{
    net::TcpStream,
    sync::{Mutex, MutexGuard},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tracing::{debug, error, info};

use crate::sdk::location_manager::{DisplayRequest, Layout, ViewType};
use crate::{
    models::{CardRating, Flashcard, FlashcardReviewNew},
    router::AppState,
    sdk::app_session::AppSession,
};
use anyhow::Result;

pub fn new_review(card: &Flashcard, rating: CardRating) -> Result<FlashcardReviewNew> {
    let next_states = schedule_states(card)?;
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

fn schedule_states(card: &Flashcard) -> anyhow::Result<fsrs::NextStates> {
    let fsrs = FSRS::new(Some(&DEFAULT_PARAMETERS))?;
    let desired_retention = 0.75; // TODO: user settings

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

pub struct SessionState {
    cards: Vec<Flashcard>,
    deck_names: HashMap<i32, String>,
    started: bool,
    app_state: Arc<PgPool>,
    user_id: String,
}

async fn next_card_or_finish<'s>(
    text: String,
    session_state: MutexGuard<'s, SessionState>,
    package_name: String,
    sender: Option<Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>,
) {
    if let Some(sender_arc) = sender {
        let display_request = if session_state.cards.is_empty() {
            info!("All cards reviewed");
            DisplayRequest {
                r#type: "display_event".to_string(),
                package_name,
                session_id: "".to_string(), // Empty like TypeScript version
                view: ViewType::Main,
                layout: Layout::TextWall {
                    text:
                        "All cards reviewed! You can end the session in the Mentra app interface."
                            .to_string(),
                },
                duration_ms: None,
                timestamp: Utc::now().to_rfc3339(),
            }
        } else {
            let last_card = session_state.cards.last().unwrap();
            let deck_name = session_state
                .deck_names
                .get(&last_card.deck_id)
                .cloned()
                .unwrap_or_default();
            DisplayRequest {
                r#type: "display_event".to_string(),
                package_name,
                session_id: "".to_string(), // Empty like TypeScript version
                view: ViewType::Main,
                layout: Layout::DoubleTextWall {
                    top_text: last_card.front.clone(),
                    bottom_text: format!("{deck_name} ({} left)", session_state.cards.len()),
                },
                duration_ms: None,
                timestamp: Utc::now().to_rfc3339(),
            }
        };

        if let Ok(display_json) = serde_json::to_string(&display_request) {
            debug!("📺 Processing: {}", display_json);
            let websocket_msg = Message::Text(display_json.into());

            let mut sender_guard = sender_arc.lock().await;
            if let Err(e) = sender_guard.send(websocket_msg).await {
                error!("Failed to send display_event message: {}", e);
            } else {
                info!("✅ Processing: {}", text);
            }
        }
    }
}

async fn update_rating<'s>(
    card: &Flashcard,
    rating: CardRating,
    session_state: &MutexGuard<'s, SessionState>,
) -> Result<()> {
    let update = new_review(card, rating)?;
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

async fn on_transcription(
    text: String,
    session_state: Arc<Mutex<SessionState>>,
    package_name: String,
    sender: Option<Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>,
) -> Result<()> {
    let started = session_state.lock().await.started;
    info!("Received transcription: {}", text);
    let text = text.trim().to_lowercase();
    if started {
        // If already started, handle the transcription
        if text.contains("reveal") {
            let session_state = session_state.lock().await;

            if let Some(card) = session_state.cards.last() {
                info!("Revealing card: {}", card.front);
                // Here you would send the back of the card to the client
                if let Some(sender_arc) = sender {
                    // Create DisplayRequest matching the Rust DisplayRequest structure
                    let display_request = DisplayRequest {
                        r#type: "display_event".to_string(),
                        package_name,
                        session_id: "".to_string(), // Empty like TypeScript version
                        view: ViewType::Main,
                        layout: Layout::DoubleTextWall {
                            top_text: card.front.clone(),
                            bottom_text: card.back.clone(),
                        },
                        duration_ms: None,
                        timestamp: Utc::now().to_rfc3339(),
                    };

                    if let Ok(display_json) = serde_json::to_string(&display_request) {
                        debug!("📺 Revealing card: {}", display_json);
                        let websocket_msg = Message::Text(display_json.into());

                        let mut sender_guard = sender_arc.lock().await;
                        if let Err(e) = sender_guard.send(websocket_msg).await {
                            error!("Failed to send display_event message: {}", e);
                        } else {
                            info!("✅ Revealed card: {}", text);
                        }
                    }
                }
            }
        } else if let Ok(rating) = text.parse::<CardRating>() {
            let mut session_state = session_state.lock().await;
            if let Some(card) = session_state.cards.pop() {
                info!("Rating card {} as {}", card.id, rating);
                // Here you would handle the rating logic
                if let Err(e) = update_rating(&card, rating, &session_state).await {
                    error!("Failed to update flashcard rating: {}", e);
                } else {
                    info!("Card {} rated as {}", card.id, rating);
                }
            }
            next_card_or_finish(text, session_state, package_name, sender).await;
        }
    } else if text.contains("start") {
        let mut session_state = session_state.lock().await;
        session_state.started = true;
        info!(
            "Starting review session with {} cards",
            session_state.cards.len()
        );
        next_card_or_finish(text, session_state, package_name, sender).await;
    }
    Ok(())
}

impl AppState {
    async fn get_cards(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<(HashMap<i32, String>, Vec<Flashcard>)> {
        let deck_names = sqlx::query(
            r#"
            SELECT id, name FROM deck WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(&*self.db)
        .await?;

        let deck_names = deck_names
            .into_iter()
            .map(|row| {
                let id: i32 = row.get("id");
                let name: String = row.get("name");
                (id, name)
            })
            .collect::<HashMap<_, _>>();

        // Fetch flashcards ordered by scheduled time (with null being first)
        // limited to `limit`
        let flashcards = sqlx::query_as::<_, Flashcard>(
            r#"
            SELECT * FROM flashcard
            WHERE deck_id IN (SELECT id FROM deck WHERE user_id = $1)
            AND last_scheduled <= NOW() OR last_scheduled IS NULL
            ORDER BY last_scheduled NULLS FIRST, id
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(limit as i64)
        .fetch_all(&*self.db)
        .await?;

        Ok((deck_names, flashcards))
    }

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

        // FIXME: take limit from settings
        let (deck_names, cards) = self.get_cards(user_id, 20).await?;
        if cards.is_empty() {
            session
                .show_text(
                    "No flashcards found. Please add flashcards in the Mentra app interface.",
                    None,
                )
                .await?;
        } else {
            session.show_text(format!("{} cards for review. Say 'start' to begin.\nSay 'reveal' to display the back answer on each card.\nSay 'easy', 'good', 'difficult', or 'again'\nto rate your card memorization.", cards.len()), None).await?;
        }
        // Set up transcription handler that echoes text back to the client
        // We need to capture the websocket sender and session info for the handler to use
        let sender_clone = session.websocket_sender.clone();
        let package_name_clone = session.package_name.clone();
        let session_state = Arc::new(Mutex::new(SessionState {
            cards,
            deck_names,
            started: false,
            app_state: self.db.clone(),
            user_id: user_id.to_string(),
        }));
        session.events().on_transcription(move |transcription| {
            info!(
                "🎤 Received transcription: {} (final: {})",
                transcription.text, transcription.is_final
            );

            // Send the transcription text back to the client using display_event
            let text = transcription.text.clone();
            let sender = sender_clone.clone();
            let package_name = package_name_clone.clone();
            let session_state = session_state.clone();
            if transcription.is_final {
                tokio::spawn(async move {
                    let session_state = session_state;
                    if let Err(e) =
                        on_transcription(text, session_state.clone(), package_name, sender).await
                    {
                        error!("Failed to process transcription: {}", e);
                    }
                });
            }
        });
        // Default implementation - can be overridden
        Ok(())
    }
}
