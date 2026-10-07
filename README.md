# tg

Read Telegram messages from the terminal. A Rust rewrite of
[tksohishi/tgcli](https://github.com/tksohishi/tgcli), built on
[grammers](https://github.com/Lonami/grammers) (MTProto).

Default output is JSONL — one JSON object per line; agents parse it natively,
scripts pipe it freely. Use `--pretty` for human-readable tables.

## Install

```sh
cargo install --path .
```

## Setup

You need a Telegram API app (get one at [my.telegram.org/apps](https://my.telegram.org/apps)):

```sh
tg auth          # guided setup: writes config, runs login, shows status
```

Configuration lives at `~/.config/tgcli/config.toml`:

```toml
api_id = 123456
api_hash = "your_api_hash"
```

`TELEGRAM_API_ID` / `TELEGRAM_API_HASH` environment variables are used as a
fallback when the file doesn't have them. The session is stored at
`~/.config/tgcli/session.db` (mode 0600).

## Usage

```sh
tg chats                                   # list chats (JSONL)
tg chats --filter team --pretty            # fuzzy filter, table output
tg read "Finance Team" --limit 20          # recent group messages
tg read "Finance Team" -q "budget"         # keyword search
tg read Alice -q deployment --after 2025-02-14
tg read Team --from @takeshi55555          # filter by sender
tg read Alerts --limit 100 | jq 'select(.text | test("ERROR"))'
tg context "Finance Team" 12345            # a message with surrounding context
tg auth status                             # current auth state
tg auth logout                             # remove the local session
```

Message JSONL fields: `id`, `text`, `chat_name`, `sender_name`, `date`,
`sender_username`, `sender_id`, `reply_to_msg_id`. `sender_username` may be
null; `sender_id` is the most stable sender key. `tg context` adds boolean
`target` / `replied_to` flags on relevant lines.

Exit codes: `1` for configuration and general errors, `2` when not
authenticated.

## Differences from the Python original

- The session is a grammers SQLite database (`session.db`), not a Telethon
  `StringSession`. Sessions are not interchangeable between the two tools, so
  `tg auth migrate-session` does not exist — log in once with `tg auth login`.
- Only the `file` session store is supported; there is no keychain backend.
- `tg update` and the PyPI update check are gone (install/update via cargo).
- Resolving a chat or sender by phone number is not supported; use `me`,
  `@username`, a numeric ID, or an exact display name.

## License

MIT
