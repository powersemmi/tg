mod auth;
mod client;
mod config;
mod format;

use std::process::ExitCode;

use chrono::{DateTime, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use console::style;
use grammers_client::Client;

const MAIN_ABOUT: &str = "Read Telegram messages from the terminal.\n\n\
Default output is JSONL. Use --pretty for human-readable output.\n\n\
Message JSONL fields: id, text, chat_name, sender_name, sender_username, \
sender_id, date, reply_to_msg_id. sender_username may be null; sender_id is \
the most stable sender key.\n\n\
Common flow: `tg chats` to find chat names, `tg read <chat>` to fetch or \
filter messages, `tg context <chat> <message_id>` to inspect nearby messages, \
`tg auth` for login and status.";

const READ_ABOUT: &str = "Read recent messages from a chat. Newest first by default (--head for \
oldest).\n\n\
Default JSONL includes id, text, chat_name, sender_name, sender_username, \
sender_id, date, and reply_to_msg_id. sender_username may be null; sender_id \
is the most stable sender key.\n\n\
--from accepts me, @username, bare username, numeric user ID, or exact \
display name. Display names can be ambiguous; prefer username or numeric ID \
for automation.\n\n\
Examples: `tg read Team --from takeshi55555`, `tg read Team --from @takeshi55555`, \
`tg read Team --from \"Takeshi\"`.";

const CONTEXT_ABOUT: &str = "View a message with surrounding context.\n\n\
Default JSONL uses the same message fields as `read` and adds boolean flags \
such as target and replied_to on relevant lines.";

#[derive(Parser)]
#[command(name = "tg", version, about = "Read Telegram messages from the terminal.", long_about = MAIN_ABOUT)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage Telegram authentication.
    Auth {
        #[command(subcommand)]
        command: Option<AuthCommand>,
    },
    /// List your Telegram chats.
    Chats {
        /// Fuzzy filter by chat name.
        #[arg(long)]
        filter: Option<String>,
        /// Max chats to list.
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Rich table output.
        #[arg(long)]
        pretty: bool,
    },
    /// Read recent messages from a chat.
    #[command(long_about = READ_ABOUT)]
    Read {
        /// Chat or person to read messages from.
        chat: String,
        /// Filter messages by text.
        #[arg(short, long)]
        query: Option<String>,
        /// Filter by sender: me, @username, bare username, numeric user ID, or exact display name.
        #[arg(long)]
        from: Option<String>,
        /// Max messages to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Oldest messages first.
        #[arg(long)]
        head: bool,
        /// Only messages after this date (YYYY-MM-DD).
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this date (YYYY-MM-DD).
        #[arg(long)]
        before: Option<String>,
        /// Rich table output instead of JSONL.
        #[arg(long)]
        pretty: bool,
    },
    /// View a message with surrounding context.
    #[command(long_about = CONTEXT_ABOUT)]
    Context {
        chat: String,
        message_id: i32,
        /// Messages before/after the target.
        #[arg(long = "context", default_value_t = 5)]
        context_size: usize,
        /// Rich text output instead of JSONL.
        #[arg(long)]
        pretty: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Interactive login: phone + verification code (or 2FA password).
    Login,
    /// Remove the local session.
    Logout,
    /// Show current auth state.
    Status,
}

fn error(prefix: &str, detail: &str) {
    eprintln!("{} {detail}", style(prefix).red());
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Auth { command } => match command {
            None => cmd_auth_default().await,
            Some(AuthCommand::Login) => cmd_auth_login().await,
            Some(AuthCommand::Logout) => cmd_auth_logout().await,
            Some(AuthCommand::Status) => cmd_auth_status().await,
        },
        Command::Chats {
            filter,
            limit,
            pretty,
        } => cmd_chats(filter, limit, pretty).await,
        Command::Read {
            chat,
            query,
            from,
            limit,
            head,
            after,
            before,
            pretty,
        } => cmd_read(chat, query, from, limit, head, after, before, pretty).await,
        Command::Context {
            chat,
            message_id,
            context_size,
            pretty,
        } => cmd_context(chat, message_id, context_size, pretty).await,
    }
}

/// Load config and connect an authorized client, or print the appropriate
/// error and return the exit code (1 for config errors, 2 for auth errors).
async fn ready_client() -> Result<Client, ExitCode> {
    let cfg = match config::load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error("Configuration error:", &e.to_string());
            eprintln!("Run `tg auth` to set up.");
            return Err(ExitCode::FAILURE);
        }
    };
    let client = match client::connect(&cfg).await {
        Ok(client) => client,
        Err(e) => {
            error("Connection failed:", &e.to_string());
            return Err(ExitCode::FAILURE);
        }
    };
    match client.is_authorized().await {
        Ok(true) => Ok(client),
        Ok(false) => {
            error("Not authenticated.", "Run `tg auth login` first.");
            Err(ExitCode::from(2))
        }
        Err(e) => {
            error("Connection failed:", &e.to_string());
            Err(ExitCode::FAILURE)
        }
    }
}

async fn cmd_chats(filter: Option<String>, limit: usize, pretty: bool) -> ExitCode {
    let client = match ready_client().await {
        Ok(client) => client,
        Err(code) => return code,
    };
    let results = match client::list_chats(&client, filter.as_deref(), limit).await {
        Ok(results) => results,
        Err(e) => {
            error("Failed to list chats:", &e.to_string());
            return ExitCode::FAILURE;
        }
    };

    if results.is_empty() {
        println!("No chats found.");
        return ExitCode::SUCCESS;
    }

    if pretty {
        println!("{}", format::format_chats_table(&results));
    } else {
        for chat in &results {
            println!("{}", format::format_chat_jsonl(chat));
        }
    }
    ExitCode::SUCCESS
}

fn parse_date(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|d| d.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_read(
    chat: String,
    query: Option<String>,
    from: Option<String>,
    limit: usize,
    head: bool,
    after: Option<String>,
    before: Option<String>,
    pretty: bool,
) -> ExitCode {
    let parse = |value: Option<String>| -> Result<Option<DateTime<Utc>>, ExitCode> {
        match value {
            None => Ok(None),
            Some(v) => match parse_date(&v) {
                Ok(d) => Ok(Some(d)),
                Err(e) => {
                    error("Invalid date format:", &format!("{v}: {e}"));
                    eprintln!("Expected format: YYYY-MM-DD");
                    Err(ExitCode::FAILURE)
                }
            },
        }
    };
    let (after, before) = match (parse(after), parse(before)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(code), _) | (_, Err(code)) => return code,
    };

    let client = match ready_client().await {
        Ok(client) => client,
        Err(code) => return code,
    };

    let opts = client::ReadOptions {
        query,
        from,
        limit,
        after,
        before,
        reverse: head,
    };
    let results = match client::read_messages(&client, &chat, opts).await {
        Ok(results) => results,
        Err(e) => {
            error("Read failed:", &e.to_string());
            return ExitCode::FAILURE;
        }
    };

    if results.is_empty() {
        println!("No messages found.");
        return ExitCode::SUCCESS;
    }

    if pretty {
        println!("{}", format::format_search_results(&results));
    } else {
        for msg in &results {
            println!("{}", format::format_message_jsonl(msg));
        }
    }
    ExitCode::SUCCESS
}

async fn cmd_context(chat: String, message_id: i32, context_size: usize, pretty: bool) -> ExitCode {
    let client = match ready_client().await {
        Ok(client) => client,
        Err(code) => return code,
    };

    let (mut messages, target_id, replied_to) =
        match client::get_context(&client, &chat, message_id, context_size).await {
            Ok(result) => result,
            Err(e) => {
                error("Context fetch failed:", &e.to_string());
                return ExitCode::FAILURE;
            }
        };

    if messages.is_empty() {
        println!("No messages found.");
        return ExitCode::SUCCESS;
    }

    if pretty {
        print!(
            "{}",
            format::format_context(&messages, target_id, replied_to.as_ref())
        );
    } else {
        let replied_to_id = replied_to.as_ref().map(|m| m.id);
        for msg in &mut messages {
            msg.target = msg.id == target_id;
            msg.replied_to = Some(msg.id) == replied_to_id;
            println!("{}", format::format_message_jsonl(msg));
        }
    }
    ExitCode::SUCCESS
}

/// Smart auth: configure, login, or show status as needed.
async fn cmd_auth_default() -> ExitCode {
    // Step 1: ensure config exists.
    let cfg = match config::load_config() {
        Ok(cfg) => cfg,
        Err(config::ConfigError::Missing(_)) => match setup_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                error("Config error:", &e.to_string());
                return ExitCode::FAILURE;
            }
        },
        Err(e) => {
            error("Config error:", &e.to_string());
            eprintln!("Check your config at {}", config::config_path().display());
            return ExitCode::FAILURE;
        }
    };

    // Step 2: check auth state.
    let info = match auth::get_status(&cfg).await {
        Ok(info) => info,
        Err(e) => {
            error("Error checking status:", &e.to_string());
            return ExitCode::FAILURE;
        }
    };

    if info.authenticated {
        print!(
            "{}",
            format::format_auth_status(
                info.authenticated,
                info.phone.as_deref(),
                info.session_exists
            )
        );
        println!("Run `tg auth logout` to log out.");
        return ExitCode::SUCCESS;
    }

    // Step 3: not authenticated, run login.
    run_login(&cfg).await
}

fn setup_config() -> anyhow::Result<config::Config> {
    eprintln!("No config found at {}\n", config::config_path().display());
    eprintln!("You need a Telegram API app to use this tool.");
    auth::prompt("Press Enter to open my.telegram.org/apps ")?;
    let _ = open::that("https://my.telegram.org/apps");

    let api_id: i32 = loop {
        match auth::prompt("\nAPI ID: ")?.parse() {
            Ok(id) => break id,
            Err(_) => eprintln!("API ID must be a number."),
        }
    };
    let api_hash = auth::prompt("API Hash: ")?;

    let path = config::write_config(api_id, &api_hash)?;
    eprintln!("Config written to {}\n", path.display());
    config::load_config().map_err(|e| anyhow::anyhow!("{e}"))
}

/// Shared login flow for both `tg auth` and `tg auth login`.
async fn run_login(cfg: &config::Config) -> ExitCode {
    eprintln!(
        "\nLogging in to Telegram. You'll be asked for your phone number\n\
         including country code (e.g. +81 90 1234 5678). The + and any\n\
         spaces/dashes are optional, but the country code is required.\n\
         Telegram will send a verification code to your account, like\n\
         logging in on a new device. Your phone number is sent to\n\
         Telegram's API only; tg does not store or transmit it.\n"
    );

    let client = match client::connect(cfg).await {
        Ok(client) => client,
        Err(e) => {
            error("Login failed:", &e.to_string());
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = auth::login(&client, cfg).await {
        error("Login failed:", &e.to_string());
        return ExitCode::FAILURE;
    }

    eprintln!(
        "\nNote: unofficial API clients are under observation by Telegram.\n\
         Normal interactive use (searching, reading your own messages) is\n\
         fine. Avoid bulk scraping, spamming, or using results for AI/ML\n\
         model training.\n\
         Full terms: https://core.telegram.org/api/terms\n"
    );
    println!("{}", style("Login successful.").green());
    ExitCode::SUCCESS
}

async fn cmd_auth_login() -> ExitCode {
    let cfg = match config::load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error("Configuration error:", &e.to_string());
            eprintln!("Run `tg auth` to set up.");
            return ExitCode::FAILURE;
        }
    };
    run_login(&cfg).await
}

async fn cmd_auth_logout() -> ExitCode {
    let cfg = config::load_config().ok();
    match auth::logout(cfg.as_ref()).await {
        Ok(()) => {
            println!("Logged out.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error("Logout failed:", &e.to_string());
            ExitCode::FAILURE
        }
    }
}

async fn cmd_auth_status() -> ExitCode {
    let cfg = match config::load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error("Configuration error:", &e.to_string());
            eprintln!("Run `tg auth` to set up.");
            return ExitCode::FAILURE;
        }
    };
    match auth::get_status(&cfg).await {
        Ok(info) => {
            print!(
                "{}",
                format::format_auth_status(
                    info.authenticated,
                    info.phone.as_deref(),
                    info.session_exists
                )
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            error("Error checking status:", &e.to_string());
            ExitCode::FAILURE
        }
    }
}
