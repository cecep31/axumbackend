use crate::config::EmailConfig;
use serde::Serialize;

const RESEND_API_URL: &str = "https://api.resend.com/emails";

#[derive(Serialize)]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

pub fn is_configured() -> bool {
    !EmailConfig::get().resend_api_key.is_empty()
}

pub async fn send_password_reset_email(to: &str, reset_link: &str) -> Result<(), String> {
    let config = EmailConfig::get();
    if config.resend_api_key.is_empty() {
        return Err("email service not configured".to_string());
    }

    let (text, html) = password_reset_template(reset_link, "1 hour");
    send_email(config, to, "Reset your password", &text, &html).await
}

async fn send_email(
    config: &EmailConfig,
    to: &str,
    subject: &str,
    text: &str,
    html: &str,
) -> Result<(), String> {
    let payload = SendEmailRequest {
        from: &config.from,
        to,
        subject,
        text,
        html,
    };

    let response = reqwest::Client::new()
        .post(RESEND_API_URL)
        .bearer_auth(&config.resend_api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(format!("resend returned {status}: {body}"))
}

fn password_reset_template(reset_link: &str, expires_in: &str) -> (String, String) {
    let text = format!(
        "You requested a password reset. Click the link below to reset your password:\n\n{reset_link}\n\nThis link expires in {expires_in}. If you didn't request this, please ignore this email."
    );

    let escaped_link = escape_html(reset_link);
    let escaped_expires_in = escape_html(expires_in);
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Reset Your Password</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; max-width: 600px; margin: 0 auto; padding: 20px; }}
    .container {{ background: #f9f9f9; border-radius: 8px; padding: 30px; }}
    h1 {{ color: #2563eb; font-size: 24px; margin-bottom: 20px; }}
    .button {{ display: inline-block; background: #2563eb; color: white; padding: 12px 24px; text-decoration: none; border-radius: 6px; margin: 20px 0; }}
    .link {{ word-break: break-all; color: #2563eb; }}
    .footer {{ margin-top: 30px; font-size: 14px; color: #666; }}
  </style>
</head>
<body>
  <div class="container">
    <h1>Reset Your Password</h1>
    <p>You requested a password reset. Click the button below to reset your password:</p>
    <a href="{escaped_link}" class="button">Reset Password</a>
    <p>Or copy and paste this link into your browser:</p>
    <p class="link">{escaped_link}</p>
    <div class="footer">
      <p>This link expires in <strong>{escaped_expires_in}</strong>.</p>
      <p>If you didn't request this, please ignore this email.</p>
    </div>
  </div>
</body>
</html>"#
    );

    (text, html)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
