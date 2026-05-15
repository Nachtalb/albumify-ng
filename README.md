# Albumify NG

A small Telegram bot that bundles forwarded media into native Telegram albums
(media groups).

Built in Rust with [teloxide](https://github.com/teloxide/teloxide).

## How it works

1. Send `/start` in a private chat with the bot.
2. Forward or send any number of photos, videos, documents, or animations.
3. Send `/create`. The bot bundles everything in order and sends it back as
   one or more native media groups.

### Grouping rules

Telegram limits what can share an album:

- **Photos + videos** can share one album.
- **Documents** must be in their own album, with only other documents.
- **Animations (GIFs)** must be in their own album, with only other animations.
- Each album holds at most **10 items**.

Albumify NG preserves your insertion order and starts a new album whenever the
next item cannot legally join the current one. So `photo, photo, doc, doc,
video, animation` becomes four albums:
`(photo, photo)(doc, doc)(video)(animation)`.

### Commands

- `/start` — begin a new album session.
- `/create` — bundle the queued media and send it back.
- `/status` — show how many items are queued.
- `/cancel` — discard the queue.
- `/help` — show the command list.

The bot publishes its commands, name, and descriptions to Telegram
automatically on startup (`setMyCommands`, `setMyName`, `setMyDescription`,
`setMyShortDescription`).

## Running

```bash
export TELOXIDE_TOKEN=123456:your-bot-token
cargo run --release
```

Or use a `.env` file:

```
TELOXIDE_TOKEN=123456:your-bot-token
RUST_LOG=info
```

## Scope notes

- **Private chats only.** Group/channel messages are ignored.
- **In-memory state.** Each user's queue lives in a `HashMap<UserId, Vec<…>>`
  and is dropped the moment `/create` (or `/cancel`) runs. Nothing is
  persisted to disk. Restarting the bot loses any in-flight queues.
- **Long polling.** No webhook server — easy to run anywhere.

Webhook mode, persistence, and admin tools are future work.

## License

MIT
