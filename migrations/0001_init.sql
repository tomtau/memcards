CREATE TABLE IF NOT EXISTS deck (
	id SERIAL PRIMARY KEY,
	name TEXT NOT NULL,
    user_id TEXT NOT NULL,
    UNIQUE (name, user_id)
);

CREATE TABLE IF NOT EXISTS flashcard (
	id SERIAL PRIMARY KEY,
    deck_id INTEGER NOT NULL,
	front TEXT NOT NULL,
	back TEXT NOT NULL,
    UNIQUE (front, deck_id),
	FOREIGN KEY (deck_id) REFERENCES deck(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS review (
    id SERIAL PRIMARY KEY,
    reviewed DATETIME NOT NULL,
    scheduled DATETIME NOT NULL,
    rating TEXT NOT NULL,
    stability REAL NOT NULL,
    difficulty REAL NOT NULL,
    flashcard_id INTEGER NOT NULL,
    FOREIGN KEY (flashcard_id) REFERENCES flashcard(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS settings (
    user_id TEXT PRIMARY KEY,
    cards_per_day INTEGER NOT NULL DEFAULT 20
);