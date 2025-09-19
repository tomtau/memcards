# MemCards

MemCards is a flashcard learning app designed for smart glasses, such as Even Realities G1, in MentraOS.
The card management frontend uses HTMX and Alpine.js, the backend uses Axum as a web server, Askama as a template renderer, and sqlx with PostgreSQL as a data store. The deployment uses Shuttle.
The Spaced Repetition System scheduling is currently done using the Free Spaced Repetition Scheduler ([FSRS](https://github.com/open-spaced-repetition/fsrs4anki/wiki/The-Algorithm)) algorithm.

## Prerequisites

Create an application in the [Mentra developer console](https://console.mentra.glass) and get the API key and package name.

Insert the API and package name in the `Secrets.toml` (or `Secrets.dev.toml`) file:
```toml
API_KEY = '...'
PACKAGE_NAME = '...'
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

Besides the Mentra app setup, you need to install [Shuttle](https://docs.shuttle.dev/getting-started/installation), the Rust toolchain, and Docker.

## Local development or self-hosting

You can run the local instance with `shuttle run`.

If you want to self-host the application and you need it to listen on 0.0.0.0 instead of localhost, you can use the following command: `shuttle run --external --release --port <port>`

The local instance will use Docker to run a PostgreSQL database. If you need it to connect to a custom external database instance, you can add the database connection string on the `main` method in `src/main.rs` as described in the [Shuttle documentation](https://docs.shuttle.dev/docs/local-run#local-runs-with-databases).

## Deployment

If you have created the application in the Shuttle console, you can deploy it with: `shuttle deploy`.

## Misc

You can use the common Rust tooling for other operations, e.g. you can run local tests with `cargo test`.
