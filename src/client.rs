use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Utc};
use grammers_client::message::Message;
use grammers_client::peer::{Peer, User};
use grammers_client::{Client, SenderPool, tl};
use grammers_session::storages::SqliteSession;
use grammers_session::types::{PeerId, PeerRef};

use crate::config::{self, Config};
use crate::format::{ChatData, MessageData};

/// Open the local session and start the connection runner.
///
/// The session is stored as an SQLite database (not compatible with the
/// Python/Telethon session file of the original tgcli).
pub async fn connect(cfg: &Config) -> Result<Client> {
    let dir = config::config_dir();
    std::fs::create_dir_all(&dir)?;
    config::restrict_permissions(&dir, 0o700)?;

    let session_path = config::session_path();
    let session = SqliteSession::open(&session_path)
        .await
        .map_err(|e| anyhow!("cannot open session at {}: {e}", session_path.display()))?;
    config::restrict_permissions(&session_path, 0o600)?;

    let SenderPool { runner, handle, .. } = SenderPool::new(Arc::new(session), cfg.api_id);
    let client = Client::new(handle);
    tokio::spawn(runner.run());
    Ok(client)
}

pub fn user_display_name(user: &User) -> String {
    let name = user.full_name();
    if name.is_empty() {
        "Unknown".to_string()
    } else {
        name
    }
}

pub fn peer_display_name(peer: &Peer) -> String {
    match peer {
        Peer::User(user) => user_display_name(user),
        _ => peer
            .name()
            .filter(|name| !name.is_empty())
            .unwrap_or("Unknown")
            .to_string(),
    }
}

fn chat_type(peer: &Peer) -> &'static str {
    match peer {
        Peer::User(_) => "user",
        Peer::Group(_) => "group",
        Peer::Channel(_) => "channel",
    }
}

#[cfg(test)]
mod tests {
    use super::is_username_like;

    #[test]
    fn accepts_valid_usernames() {
        assert!(is_username_like("alice"));
        assert!(is_username_like("takeshi55555"));
        assert!(is_username_like("a_b_c_d"));
    }

    #[test]
    fn rejects_invalid_usernames() {
        assert!(!is_username_like("abcd")); // too short
        assert!(!is_username_like("1alice")); // starts with digit
        assert!(!is_username_like("_alice")); // starts with underscore
        assert!(!is_username_like("ali ce")); // space
        assert!(!is_username_like(&"a".repeat(33))); // too long
    }
}

/// Matches Telegram usernames: ^[A-Za-z][A-Za-z0-9_]{4,31}$
fn is_username_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    (5..=32).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

async fn try_resolve_username(client: &Client, username: &str) -> Option<(PeerRef, String)> {
    let peer = client.resolve_username(username).await.ok().flatten()?;
    let name = peer_display_name(&peer);
    let peer_ref = peer.to_ref().await.ok().flatten()?;
    Some((peer_ref, name))
}

/// Resolve a chat argument to a peer.
///
/// Handles `me`, `@username`, numeric IDs, and exact display names
/// (matched case-insensitively against dialog names).
pub async fn resolve_chat(client: &Client, name: &str) -> Result<(PeerRef, String)> {
    if name.eq_ignore_ascii_case("me") {
        let me = client.get_me().await?;
        return Ok((PeerId::self_user().to_ambient_ref(), user_display_name(&me)));
    }

    if let Some(username) = name.strip_prefix('@')
        && let Some(found) = try_resolve_username(client, username).await
    {
        return Ok(found);
    }

    let target_id: Option<i64> = name.parse().ok();
    let name_lower = name.to_lowercase();
    let mut dialogs = client.iter_dialogs();
    while let Some(dialog) = dialogs.next().await? {
        let display = peer_display_name(dialog.peer());
        let id = dialog.peer().id();
        if display.to_lowercase() == name_lower
            || target_id
                .is_some_and(|t| id.bot_api_dialog_id() == Some(t) || id.bare_id() == Some(t))
        {
            return Ok((dialog.peer_ref(), display));
        }
    }

    bail!("Cannot find chat \"{name}\". Use `tg chats --filter` to find exact names.")
}

/// Resolve a `--from` sender filter within a chat to a peer ID.
pub async fn resolve_sender(
    client: &Client,
    chat: PeerRef,
    value: &str,
    me: &User,
) -> Result<PeerId> {
    if value.trim().eq_ignore_ascii_case("me") {
        return Ok(me.id());
    }

    let value = value.trim();
    if let Ok(n) = value.parse::<i64>()
        && let Some(id) = PeerId::from_bot_api_dialog_id(n)
    {
        return Ok(id);
    }

    let username = value.strip_prefix('@').unwrap_or(value);
    if is_username_like(username)
        && let Ok(Some(peer)) = client.resolve_username(username).await
    {
        return Ok(peer.id());
    }

    let mut participants = client.iter_participants(chat).filter(
        tl::enums::ChannelParticipantsFilter::ChannelParticipantsSearch(
            tl::types::ChannelParticipantsSearch {
                q: username.to_string(),
            },
        ),
    );
    let value_lower = value.to_lowercase();
    let mut display_matches: Vec<PeerId> = Vec::new();
    while let Some(participant) = participants.next().await? {
        if let Some(u) = participant.user.username()
            && u.eq_ignore_ascii_case(username)
        {
            return Ok(participant.user.id());
        }
        if user_display_name(&participant.user).to_lowercase() == value_lower {
            display_matches.push(participant.user.id());
        }
    }

    match display_matches.as_slice() {
        [single] => Ok(*single),
        [] => bail!(
            "Cannot find sender \"{value}\" in this chat. Use username, numeric ID, \
             or exact display name."
        ),
        _ => bail!("Found multiple senders named \"{value}\". Use a username or numeric ID."),
    }
}

fn msg_to_data(msg: &Message, chat_name: &str, me: &User) -> MessageData {
    let sender_id_raw = msg.sender_id();
    let is_self = sender_id_raw == Some(PeerId::self_user()) || sender_id_raw == Some(me.id());

    let (sender_name, sender_username, sender_id) = if is_self {
        (
            user_display_name(me),
            me.username().map(str::to_string),
            me.id().bot_api_dialog_id(),
        )
    } else {
        let id = sender_id_raw.and_then(|id| id.bot_api_dialog_id());
        match msg.sender() {
            Some(peer) => (
                peer_display_name(peer),
                peer.username().map(str::to_string),
                id,
            ),
            None => ("Unknown".to_string(), None, id),
        }
    };

    MessageData {
        id: msg.id(),
        text: msg.text().to_string(),
        chat_name: chat_name.to_string(),
        sender_name,
        date: msg.date(),
        sender_username,
        sender_id,
        reply_to_msg_id: msg.reply_to_message_id(),
        target: false,
        replied_to: false,
    }
}

pub async fn list_chats(
    client: &Client,
    filter_name: Option<&str>,
    limit: usize,
) -> Result<Vec<ChatData>> {
    let filter_lower = filter_name.map(str::to_lowercase);
    let mut results = Vec::new();
    let mut dialogs = client.iter_dialogs().limit(limit);
    while let Some(dialog) = dialogs.next().await? {
        let name = peer_display_name(dialog.peer());
        if let Some(filter) = &filter_lower
            && !name.to_lowercase().contains(filter.as_str())
        {
            continue;
        }
        let (unread_count, pinned) = match &dialog.raw {
            tl::enums::Dialog::Dialog(d) => (d.unread_count, d.pinned),
            tl::enums::Dialog::Folder(d) => (
                d.unread_muted_messages_count + d.unread_unmuted_messages_count,
                d.pinned,
            ),
        };
        results.push(ChatData {
            name,
            chat_type: chat_type(dialog.peer()).to_string(),
            unread_count,
            pinned,
            date: dialog.last_message.as_ref().map(|m| m.date()),
        });
    }
    Ok(results)
}

pub struct ReadOptions {
    pub query: Option<String>,
    pub from: Option<String>,
    pub limit: usize,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub reverse: bool,
}

/// Read messages from a chat.
///
/// Default order is newest first (tail); `reverse` gives oldest first (head).
/// `query` filters client-side by substring (case-insensitive), `from` by sender.
pub async fn read_messages(
    client: &Client,
    chat: &str,
    opts: ReadOptions,
) -> Result<Vec<MessageData>> {
    let (peer, chat_name) = resolve_chat(client, chat).await?;
    let me = client.get_me().await?;

    let from_id = match &opts.from {
        Some(value) => Some(resolve_sender(client, peer, value, &me).await?),
        None => None,
    };

    let filtering = opts.query.is_some() || from_id.is_some();
    let filter_query = opts.query.as_ref().map(|q| q.to_lowercase());

    let mut iter = client.iter_messages(peer).reverse(opts.reverse);
    if let Some(before) = &opts.before
        && !opts.reverse
    {
        iter = iter.offset_date(before.timestamp() as i32);
    }
    if !filtering {
        iter = iter.limit(opts.limit);
    }

    let mut results = Vec::new();
    while let Some(msg) = iter.next().await? {
        let date = msg.date();
        if let Some(before) = &opts.before
            && date >= *before
        {
            if opts.reverse {
                break;
            }
            continue;
        }
        if let Some(after) = &opts.after
            && date < *after
        {
            if opts.reverse {
                continue;
            }
            break;
        }
        if let Some(query) = &filter_query
            && !msg.text().to_lowercase().contains(query.as_str())
        {
            continue;
        }
        if let Some(target) = from_id {
            // Outgoing messages report the sentinel "self" ID; normalize it.
            let sender = msg.sender_id().map(|id| {
                if id == PeerId::self_user() {
                    me.id()
                } else {
                    id
                }
            });
            if sender != Some(target) {
                continue;
            }
        }
        results.push(msg_to_data(&msg, &chat_name, &me));
        if results.len() >= opts.limit {
            break;
        }
    }
    Ok(results)
}

/// Get a message and its surrounding context.
///
/// Returns (messages, target_id, replied_to_message).
pub async fn get_context(
    client: &Client,
    chat: &str,
    message_id: i32,
    context: usize,
) -> Result<(Vec<MessageData>, i32, Option<MessageData>)> {
    let (peer, chat_name) = resolve_chat(client, chat).await?;
    let me = client.get_me().await?;

    let mut raw_messages: Vec<Message> = Vec::new();

    // Messages after (newer than) the target.
    let mut newer = client
        .iter_messages(peer)
        .offset_id(message_id)
        .reverse(true)
        .limit(context);
    while let Some(msg) = newer.next().await? {
        raw_messages.push(msg);
    }

    // The target message itself + messages before (older than) it.
    let mut older = client
        .iter_messages(peer)
        .offset_id(message_id + 1)
        .limit(context + 1);
    while let Some(msg) = older.next().await? {
        raw_messages.push(msg);
    }

    raw_messages.sort_by_key(|m| m.id());

    let mut replied_to = None;
    if let Some(target) = raw_messages.iter().find(|m| m.id() == message_id)
        && target.reply_to_message_id().is_some()
    {
        replied_to = client
            .get_reply_to_message(target)
            .await?
            .map(|m| msg_to_data(&m, &chat_name, &me));
    }

    let messages = raw_messages
        .iter()
        .map(|m| msg_to_data(m, &chat_name, &me))
        .collect();

    Ok((messages, message_id, replied_to))
}
