//! Albumify NG — Telegram bot that bundles forwarded media into albums.

mod ack;
mod album;
mod commands;
mod handlers;
mod state;

use anyhow::{Context, Result};
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::ChatKind;

use crate::ack::AckDebouncer;
use crate::commands::{Command, publish_bot_metadata};
use crate::handlers::{handle_command, handle_media};
use crate::state::MediaStore;

#[tokio::main]
async fn main() -> Result<()> {
    // .env is optional — environment variables win either way.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,teloxide=info")),
        )
        .init();

    let token = std::env::var("TELOXIDE_TOKEN")
        .context("TELOXIDE_TOKEN must be set (.env or environment)")?;
    let bot = Bot::new(token);

    if let Err(err) = publish_bot_metadata(&bot).await {
        tracing::warn!(?err, "failed to publish bot metadata on startup");
    } else {
        tracing::info!("bot metadata (commands, descriptions, name) published");
    }

    let store = MediaStore::new();
    let ack = AckDebouncer::new();

    // Reject group/channel messages early — Albumify NG is a private-chat bot.
    let private_only = dptree::filter(|msg: Message| matches!(msg.chat.kind, ChatKind::Private(_)));

    let handler = Update::filter_message().branch(
        private_only
            .clone()
            .branch(
                dptree::entry()
                    .filter_command::<Command>()
                    .endpoint(handle_command),
            )
            .branch(dptree::endpoint(handle_media)),
    );

    tracing::info!("starting long-polling dispatcher");

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![store, ack])
        .default_handler(|_| async {})
        .error_handler(LoggingErrorHandler::with_custom_text(
            "an error in the update handler",
        ))
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
