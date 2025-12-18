# MemCards

MemCards is a flashcard learning app designed for smart glasses, such as Even Realities G1, in MentraOS.
The card management frontend uses HTMX and Alpine.js, the backend uses Axum as a web server, Askama as a template renderer, and SQLx with PostgreSQL as a data store.
The Spaced Repetition System scheduling is currently done using the Free Spaced Repetition Scheduler ([FSRS](https://github.com/open-spaced-repetition/fsrs4anki/wiki/The-Algorithm)) algorithm.

## Prerequisites

Create an application in the [Mentra developer console](https://console.mentra.glass) and get the API key and package name.

Set the following environment variables (or use a `.env` file):
```bash
DATABASE_URL=postgres://user:password@localhost:5432/memcards
API_KEY=your_api_key
PACKAGE_NAME=your_package_name
# Optional:
HOST=127.0.0.1  # Default: 127.0.0.1
PORT=8000  # Default: 8000
CLOUD_API_URL=https://prod.augmentos.cloud  # Default
USER_TOKEN_PUBLIC_KEY=...  # Optional, has a default value
```

For the app configuration, you can modify and import the following `app_config.json`:
```json
{
  "name": "MemCards",
  "description": "MemCards is a flashcard learning app designed for smart glasses, such as Even Realities G1, in MentraOS.",
  "onboardingInstructions": "Manage flashcards in the Mentra app interface.\nWhen you have some cards to review, say 'start' to begin reviewing cards.\nWhile reviewing cards, you can look up and down or say 'reveal' to see answers.\nSay 'easy', 'good', 'difficult', or 'again' to rate your card memorization.",
  "publicUrl": "<URL>",
  "logoURL": "https://imagedelivery.net/nrc8B2Lk8UIoyW7fY8uHVg/7bd531eb-731b-43e7-c6f2-8c99d5865800/square",
  "appType": "standard",
  "permissions": [
    {
      "type": "MICROPHONE",
      "description": "Voice commands are used for controlling the flashcard display on the glasses."
    }
  ],
  "settings": [
    {
      "type": "numeric_input",
      "key": "max_cards_per_session",
      "label": "Maximum number of cards in each review session",
      "defaultValue": 20,
      "min": 1,
      "max": 100,
      "step": 1,
      "placeholder": "Enter a maximum number of cards per review session"
    },
    {
      "type": "numeric_input",
      "key": "desired_retention",
      "label": "The desired minimum retention rate: a card will be scheduled at a time in the future when the predicted probability of you correctly recalling that card falls to the set value (e.g. 90%). A higher rate will lead to more reviews and a lower rate will lead to fewer reviews.",
      "defaultValue": 75,
      "min": 5,
      "max": 95,
      "step": 5,
      "placeholder": "the desired minimum retention rate for cards when scheduled"
    }
  ],
  "tools": [],
  "webviewURL": "<URL>/webview"
}
```

At the time of writing, the `app_config.json` file did not contain the Hardware Requirements section. You can add it manually in the developer console (you can add "Display" and "Microphone").

You need to install the Rust toolchain and have access to a PostgreSQL database.

## Local development

1. Make sure you have a PostgreSQL database running and set the `DATABASE_URL` environment variable.

2. Run the application:
   ```bash
   cargo run
   ```

   The server will start on `127.0.0.1:8000` by default. You can change this using the `HOST` and `PORT` environment variables.

3. To listen on all interfaces (e.g., for external access or Docker), set:
   ```bash
   HOST=0.0.0.0
   ```

## Database Migrations

The application automatically runs migrations on startup using SQLx. Migration files are located in the `migrations` directory.

## Self-Hosting / Deployment

1. Set up a PostgreSQL database and note the connection string.

2. Set the required environment variables:
   ```bash
   export DATABASE_URL="postgres://user:password@host:5432/memcards"
   export API_KEY="your_api_key"
   export PACKAGE_NAME="your_package_name"
   export HOST="0.0.0.0"
   export PORT="8000"
   ```

3. Build and run the release binary:
   ```bash
   cargo build --release
   ./target/release/memcards
   ```

Alternatively, you can use Docker or any container platform to deploy the application.

## Misc

You can use the common Rust tooling for other operations, e.g. you can run local tests with `cargo test`.
