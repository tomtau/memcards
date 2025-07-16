CREATE TABLE IF NOT EXISTS deck (
	id SERIAL PRIMARY KEY,
	name TEXT NOT NULL,
    user_id TEXT NOT NULL,
    UNIQUE (name, user_id)
);

CREATE TYPE card_rating AS ENUM ('easy', 'good', 'difficult', 'again');

CREATE TABLE IF NOT EXISTS flashcard (
	id SERIAL PRIMARY KEY,
    deck_id INTEGER NOT NULL,
	front TEXT NOT NULL,
	back TEXT NOT NULL,
    last_rating card_rating,
    last_reviewed TIMESTAMP,
    last_scheduled TIMESTAMP,
    last_stability REAL,
    last_difficulty REAL,
    UNIQUE (front, deck_id),
	FOREIGN KEY (deck_id) REFERENCES deck(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_flashcard_scheduled ON flashcard(last_scheduled);
