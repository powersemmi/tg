use std::io::{self, BufRead, Write};

use anyhow::{Result, anyhow};
use grammers_client::{Client, SignInError};

use crate::client::connect;
use crate::config::{self, Config};

pub fn prompt(message: &str) -> Result<String> {
    let mut stderr = io::stderr();
    stderr.write_all(message.as_bytes())?;
    stderr.flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Interactive login: phone + code (or 2FA password). The session is
/// persisted automatically by the SQLite session storage.
pub async fn login(client: &Client, cfg: &Config) -> Result<()> {
    if client.is_authorized().await? {
        return Ok(());
    }

    let phone = prompt("Phone number: ")?;
    let token = client.request_login_code(&phone, &cfg.api_hash).await?;
    let code = prompt("Please enter the code you received: ")?;

    match client.sign_in(&token, &code).await {
        Ok(_) => Ok(()),
        Err(SignInError::PasswordRequired(password_token)) => {
            let hint = password_token.hint().unwrap_or("none").to_string();
            let password =
                rpassword::prompt_password(format!("Please enter your password (hint: {hint}): "))?;
            client
                .check_password(password_token, password.trim())
                .await
                .map(|_| ())
                .map_err(|e| anyhow!("{e}"))
        }
        Err(e) => Err(anyhow!("{e}")),
    }
}

/// Log out and remove the local session.
///
/// Always deletes the local session, even if the remote logout fails.
pub async fn logout(cfg: Option<&Config>) -> Result<()> {
    if let Some(cfg) = cfg
        && config::session_path().exists()
        && let Ok(client) = connect(cfg).await
        && client.is_authorized().await.unwrap_or(false)
    {
        let _ = client.sign_out().await;
    }
    match std::fs::remove_file(config::session_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub struct StatusInfo {
    pub authenticated: bool,
    pub phone: Option<String>,
    pub session_exists: bool,
}

pub async fn get_status(cfg: &Config) -> Result<StatusInfo> {
    if !config::session_path().exists() {
        return Ok(StatusInfo {
            authenticated: false,
            phone: None,
            session_exists: false,
        });
    }

    let client = connect(cfg).await?;
    let authenticated = client.is_authorized().await?;
    let mut phone = None;
    if authenticated {
        let me = client.get_me().await?;
        phone = me.phone().map(mask_phone);
    }

    Ok(StatusInfo {
        authenticated,
        phone,
        session_exists: true,
    })
}

#[cfg(test)]
mod tests {
    use super::mask_phone;

    #[test]
    fn masks_long_phone() {
        assert_eq!(mask_phone("819012345678"), "819*******78");
    }

    #[test]
    fn keeps_short_phone() {
        assert_eq!(mask_phone("12345"), "12345");
    }
}

/// Mask a phone number: show the first 3 and last 2 digits.
fn mask_phone(phone: &str) -> String {
    if phone.len() > 5 {
        format!(
            "{}{}{}",
            &phone[..3],
            "*".repeat(phone.len() - 5),
            &phone[phone.len() - 2..]
        )
    } else {
        phone.to_string()
    }
}
