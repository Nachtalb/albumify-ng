//! Handler functions for commands and incoming media.

use anyhow::Result;
use teloxide::prelude::*;
use teloxide::types::{MediaKind, MessageKind};
use teloxide::utils::command::BotCommands;

use crate::ack::AckDebouncer;
use crate::album::{plan_groups, resolve_group};
use crate::commands::Command;
use crate::state::{MediaStore, PendingMedia};

/// Dispatch a parsed command to the right handler.
pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    store: MediaStore,
    ack: AckDebouncer,
) -> Result<()> {
    match cmd {
        Command::Help => help(&bot, &msg).await,
        Command::Start => start(&bot, &msg, &store, &ack).await,
        Command::Status => status(&bot, &msg, &store, &ack).await,
        Command::Cancel => cancel(&bot, &msg, &store, &ack).await,
        Command::Create => create(&bot, &msg, &store, &ack).await,
    }
}

async fn help(bot: &Bot, msg: &Message) -> Result<()> {
    bot.send_message(msg.chat.id, Command::descriptions().to_string())
        .await?;
    Ok(())
}

async fn start(bot: &Bot, msg: &Message, store: &MediaStore, ack: &AckDebouncer) -> Result<()> {
    if let Some(user) = msg.from.as_ref() {
        // Throw away any pending debounced ack — /start is itself a status
        // message, no need to follow it with "Queued 0 items".
        ack.cancel(user.id).await;
        let prior = store.clear(user.id).await;
        let extra = if prior > 0 {
            format!(" (discarded {prior} item(s) from a previous session)")
        } else {
            String::new()
        };
        bot.send_message(
            msg.chat.id,
            format!(
                "Send me photos, videos, documents or audio.{extra}\n\n\
                 When you're done, send /create.\n\
                 /status to peek at the queue, /cancel to throw it all out."
            ),
        )
        .await?;
    }
    Ok(())
}

async fn status(bot: &Bot, msg: &Message, store: &MediaStore, ack: &AckDebouncer) -> Result<()> {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    ack.cancel(user.id).await;
    let n = store.len(user.id).await;
    let text = if n == 0 {
        "Queue is empty. Send me some media first.".to_string()
    } else {
        format!("{n} item(s) queued. Send /create to bundle them.")
    };
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

async fn cancel(bot: &Bot, msg: &Message, store: &MediaStore, ack: &AckDebouncer) -> Result<()> {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    ack.cancel(user.id).await;
    let removed = store.clear(user.id).await;
    let text = if removed == 0 {
        "Nothing to cancel — queue was already empty.".to_string()
    } else {
        format!("Discarded {removed} item(s). Send /start to begin again.")
    };
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

async fn create(bot: &Bot, msg: &Message, store: &MediaStore, ack: &AckDebouncer) -> Result<()> {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    // Drop any pending debounced ack — /create supersedes it.
    ack.cancel(user.id).await;

    // `take` drains the buffer — even if sending fails, we don't leak state.
    // Users can simply re-forward the items and try again.
    let items = store.take(user.id).await;
    if items.is_empty() {
        bot.send_message(
            msg.chat.id,
            "Nothing queued. Send some media first, then /create.",
        )
        .await?;
        return Ok(());
    }

    let total = items.len();
    let groups = plan_groups(items);
    let group_count = groups.len();

    for (i, group) in groups.into_iter().enumerate() {
        let resolved = resolve_group(group);
        if let Err(err) = bot.send_media_group(msg.chat.id, resolved).await {
            tracing::warn!(?err, group_index = i, "send_media_group failed");
            bot.send_message(
                msg.chat.id,
                format!("Failed to send album {}/{group_count}: {err}", i + 1),
            )
            .await?;
            return Ok(());
        }
    }

    bot.send_message(
        msg.chat.id,
        format!("Sent {total} item(s) in {group_count} album(s). Send /start for another."),
    )
    .await?;
    Ok(())
}

/// Inspect a non-command message and, if it contains supported media, queue it.
///
/// Photos arrive as a `Vec<PhotoSize>` — Telegram's resolution ladder; we
/// take the largest by `file_size` (falling back to the last entry).
///
/// The user-visible ack is debounced (`AckDebouncer`) so a rapid burst of
/// uploads collapses into one summary message instead of N spammy replies.
pub async fn handle_media(
    bot: Bot,
    msg: Message,
    store: MediaStore,
    ack: AckDebouncer,
) -> Result<()> {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    let user_id = user.id;

    let MessageKind::Common(common) = &msg.kind else {
        return Ok(());
    };

    let queued = match &common.media_kind {
        MediaKind::Photo(p) => {
            let best = p
                .photo
                .iter()
                .max_by_key(|s| s.file.size)
                .or_else(|| p.photo.last());
            match best {
                Some(size) => {
                    store
                        .push(user_id, PendingMedia::Photo(size.file.id.0.clone()))
                        .await;
                    true
                }
                None => false,
            }
        }
        MediaKind::Video(v) => {
            store
                .push(user_id, PendingMedia::Video(v.video.file.id.0.clone()))
                .await;
            true
        }
        MediaKind::Animation(_) => {
            bot.send_message(
                msg.chat.id,
                "Animations (GIFs) aren't supported. Send the file as a video or document instead.",
            )
            .await?;
            false
        }
        MediaKind::Document(d) => {
            store
                .push(
                    user_id,
                    PendingMedia::Document(d.document.file.id.0.clone()),
                )
                .await;
            true
        }
        MediaKind::Audio(a) => {
            store
                .push(user_id, PendingMedia::Audio(a.audio.file.id.0.clone()))
                .await;
            true
        }
        _ => false,
    };

    if queued {
        ack.schedule(bot, msg.chat.id, user_id, store).await;
    }

    Ok(())
}
