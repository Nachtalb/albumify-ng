//! Bot command definitions and one-shot Telegram metadata setup.

use anyhow::Result;
use teloxide::Bot;
use teloxide::payloads::{SetMyDescriptionSetters, SetMyShortDescriptionSetters};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

/// User-visible commands. The `description` attributes are surfaced both in
/// `/help` and in Telegram's command menu (set via `set_my_commands`).
#[derive(BotCommands, Clone, Debug, PartialEq, Eq)]
#[command(
    rename_rule = "lowercase",
    description = "Albumify NG bundles forwarded media into albums.\n\nSend me photos, videos, documents or audio, then use /create.\n\nCommands:"
)]
pub enum Command {
    #[command(description = "show this help text.")]
    Help,
    #[command(description = "begin a new album session.")]
    Start,
    #[command(description = "bundle queued media into album(s) and send.")]
    Create,
    #[command(description = "discard everything queued so far.")]
    Cancel,
    #[command(description = "show how many items are queued.")]
    Status,
}

/// Push commands, name, and descriptions to Telegram on startup.
///
/// Run once per process. Failures are logged but not fatal — the bot can
/// still serve users even if BotFather metadata is out of sync.
pub async fn publish_bot_metadata(bot: &Bot) -> Result<()> {
    bot.set_my_commands(Command::bot_commands()).await?;

    // Short description — shown on the bot's profile card.
    bot.set_my_short_description()
        .short_description("Bundle forwarded media into Telegram albums.")
        .await?;

    // Long description — shown on the empty-chat splash screen before /start.
    bot.set_my_description()
        .description(
            "Albumify NG\n\n\
             Forward photos, videos, documents and audio, then send /create to \
             receive them back as one or more native Telegram albums.\n\n\
             • Photos and videos share an album.\n\
             • Documents get their own albums.\n\
             • Audio gets its own albums.\n\
             • Animations (GIFs) aren't supported.\n\
             • Order is preserved.\n\n\
             /help for the full command list.",
        )
        .await?;

    // Display name in the chat header. set_my_name is best-effort: Telegram
    // rate-limits it harshly, so we ignore "Too Many Requests" type errors.
    if let Err(err) = bot.set_my_name().name("Albumify NG").await {
        tracing::warn!(?err, "set_my_name failed (likely rate-limited); skipping");
    }

    Ok(())
}
