use chrono::{DateTime, Utc};
use comfy_table::{Attribute, Cell, CellAlignment, Color, Table, presets};
use console::style;
use serde::{Serialize, Serializer};

#[derive(Serialize)]
pub struct ChatData {
    pub name: String,
    pub chat_type: String,
    pub unread_count: i32,
    pub pinned: bool,
    #[serde(serialize_with = "ser_opt_date")]
    pub date: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct MessageData {
    pub id: i32,
    pub text: String,
    pub chat_name: String,
    pub sender_name: String,
    #[serde(serialize_with = "ser_date")]
    pub date: DateTime<Utc>,
    pub sender_username: Option<String>,
    pub sender_id: Option<i64>,
    pub reply_to_msg_id: Option<i32>,
    #[serde(skip_serializing_if = "is_false")]
    pub target: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub replied_to: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn ser_date<S: Serializer>(date: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&date.to_rfc3339())
}

fn ser_opt_date<S: Serializer>(date: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match date {
        Some(d) => s.serialize_str(&d.to_rfc3339()),
        None => s.serialize_none(),
    }
}

fn truncate(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        text.to_string()
    } else {
        format!("{} ...", lines[..max_lines].join("\n"))
    }
}

pub fn format_message_jsonl(msg: &MessageData) -> String {
    serde_json::to_string(msg).expect("message serialization cannot fail")
}

pub fn format_chat_jsonl(chat: &ChatData) -> String {
    serde_json::to_string(chat).expect("chat serialization cannot fail")
}

pub fn format_search_results(messages: &[MessageData]) -> Table {
    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL_CONDENSED);
    table.set_header(vec![
        Cell::new("Date").add_attribute(Attribute::Bold),
        Cell::new("Chat").add_attribute(Attribute::Bold),
        Cell::new("Sender").add_attribute(Attribute::Bold),
        Cell::new("Message").add_attribute(Attribute::Bold),
    ]);
    for msg in messages {
        table.add_row(vec![
            Cell::new(msg.date.format("%Y-%m-%d %H:%M")).fg(Color::DarkGrey),
            Cell::new(&msg.chat_name),
            Cell::new(&msg.sender_name),
            Cell::new(truncate(&msg.text, 3)),
        ]);
    }
    table
}

pub fn format_context(
    messages: &[MessageData],
    target_id: i32,
    replied_to: Option<&MessageData>,
) -> String {
    let mut output = String::new();

    if let Some(reply) = replied_to {
        output.push_str(
            &style(format!("  >> {}: {}\n", reply.sender_name, reply.text))
                .dim()
                .italic()
                .to_string(),
        );
        output.push_str(&style(format!("  {}\n", "-".repeat(40))).dim().to_string());
    }

    for msg in messages {
        let line = format!(
            "[{}] {}: {}\n",
            msg.date.format("%H:%M"),
            msg.sender_name,
            msg.text
        );
        if msg.id == target_id {
            output.push_str(&style(line).yellow().bold().to_string());
        } else {
            output.push_str(&line);
        }
    }

    output
}

pub fn format_chats_table(chats: &[ChatData]) -> Table {
    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL_CONDENSED);
    table.set_header(vec![
        Cell::new("Name").add_attribute(Attribute::Bold),
        Cell::new("Type").add_attribute(Attribute::Bold),
        Cell::new("Unread").add_attribute(Attribute::Bold),
        Cell::new("Last message").add_attribute(Attribute::Bold),
    ]);
    for chat in chats {
        let unread = if chat.unread_count != 0 {
            chat.unread_count.to_string()
        } else {
            String::new()
        };
        let date = chat
            .date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        table.add_row(vec![
            Cell::new(&chat.name),
            Cell::new(&chat.chat_type).fg(Color::DarkGrey),
            Cell::new(unread).set_alignment(CellAlignment::Right),
            Cell::new(date).fg(Color::DarkGrey),
        ]);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn message(text: &str) -> MessageData {
        MessageData {
            id: 42,
            text: text.to_string(),
            chat_name: "Team".to_string(),
            sender_name: "Alice Doe".to_string(),
            date: Utc.with_ymd_and_hms(2025, 2, 14, 12, 34, 56).unwrap(),
            sender_username: None,
            sender_id: Some(12345),
            reply_to_msg_id: None,
            target: false,
            replied_to: false,
        }
    }

    #[test]
    fn truncate_keeps_short_text() {
        assert_eq!(truncate("a\nb\nc", 3), "a\nb\nc");
    }

    #[test]
    fn truncate_cuts_long_text() {
        assert_eq!(truncate("a\nb\nc\nd", 3), "a\nb\nc ...");
    }

    #[test]
    fn message_jsonl_field_order_and_date() {
        let jsonl = format_message_jsonl(&message("hi"));
        assert_eq!(
            jsonl,
            "{\"id\":42,\"text\":\"hi\",\"chat_name\":\"Team\",\
             \"sender_name\":\"Alice Doe\",\"date\":\"2025-02-14T12:34:56+00:00\",\
             \"sender_username\":null,\"sender_id\":12345,\"reply_to_msg_id\":null}"
        );
    }

    #[test]
    fn message_jsonl_includes_flags_only_when_set() {
        let mut msg = message("hi");
        msg.target = true;
        let jsonl = format_message_jsonl(&msg);
        assert!(jsonl.ends_with(",\"target\":true}"));
        assert!(!jsonl.contains("replied_to"));
    }

    #[test]
    fn message_jsonl_keeps_non_ascii() {
        let jsonl = format_message_jsonl(&message("привет"));
        assert!(jsonl.contains("привет"));
    }

    #[test]
    fn chat_jsonl_null_date() {
        let chat = ChatData {
            name: "Team".to_string(),
            chat_type: "group".to_string(),
            unread_count: 3,
            pinned: true,
            date: None,
        };
        assert_eq!(
            format_chat_jsonl(&chat),
            "{\"name\":\"Team\",\"chat_type\":\"group\",\"unread_count\":3,\
             \"pinned\":true,\"date\":null}"
        );
    }
}

pub fn format_auth_status(
    authenticated: bool,
    phone: Option<&str>,
    session_exists: bool,
) -> String {
    let mut output = String::new();

    output.push_str(&style("Status: ").bold().to_string());
    if authenticated {
        output.push_str(&style("authenticated\n").green().to_string());
    } else {
        output.push_str(&style("not authenticated\n").red().to_string());
    }

    if let Some(phone) = phone {
        output.push_str(&style("Phone: ").bold().to_string());
        output.push_str(&format!("{phone}\n"));
    }

    output.push_str(&style("Session: ").bold().to_string());
    if session_exists {
        output.push_str(&style("stored locally\n").green().to_string());
    } else {
        output.push_str(&style("none (file)\n").red().to_string());
    }

    output
}
