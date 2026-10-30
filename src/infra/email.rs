//! Password-reset email delivery, mirroring echobackend's
//! `internal/platform/email.Service`: messages are queued on an in-process
//! channel (capacity 128) and sent by 2 background workers, each attempt
//! bounded by `SMTP_TASK_TIMEOUT_SECONDS`, so the forgot-password request
//! never waits on delivery.
//!
//! Transport selection ([`QueuedMailer::from_config`]):
//! 1. SMTP when `SMTP_HOST`, `SMTP_PORT` and `SMTP_FROM` are set (echobackend).
//!    `SMTP_TLS=true` is implicit TLS; otherwise STARTTLS is used when the
//!    server offers it. Credentials are sent when a username or password is set.
//! 2. Otherwise the Resend API when `RESEND_API_KEY` is set (axum-only extra).
//! 3. Otherwise delivery is disabled (`is_configured() == false`) and the
//!    caller records a dev-mode activity log instead.

use crate::config::EmailConfig;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::{
        authentication::{Credentials, Mechanism},
        client::{Tls, TlsParameters},
    },
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

const RESEND_API_URL: &str = "https://api.resend.com/emails";
const QUEUE_CAPACITY: usize = 128;
const WORKER_COUNT: usize = 2;

/// Outgoing email dependency. Handlers and services receive it as
/// `Arc<dyn Mailer>` so tests can swap in a fake.
pub trait Mailer: Send + Sync {
    /// Whether delivery is enabled (echobackend `IsConfigured`).
    fn is_configured(&self) -> bool;
    /// Queues a password reset email for background delivery (echobackend
    /// `EnqueuePasswordResetEmail`). Fails when delivery is not configured or
    /// the queue is full; delivery errors are only logged by the workers.
    fn enqueue_password_reset_email(&self, to: &str, reset_link: &str) -> Result<(), String>;
}

struct PasswordResetPayload {
    to: String,
    reset_link: String,
}

enum Transport {
    Smtp {
        transport: AsyncSmtpTransport<Tokio1Executor>,
        from: Mailbox,
    },
    Resend {
        api_key: String,
        from: String,
        client: reqwest::Client,
    },
}

/// Production mailer: SMTP (or Resend) behind an in-process worker pool.
pub struct QueuedMailer {
    tasks: Option<mpsc::Sender<PasswordResetPayload>>,
}

impl QueuedMailer {
    /// A mailer that never delivers (`is_configured() == false`).
    pub fn disabled() -> Self {
        Self { tasks: None }
    }

    /// Builds the transport from `cfg` and starts the workers. Must be called
    /// inside the Tokio runtime.
    pub fn from_config(cfg: &EmailConfig) -> Self {
        let transport = match build_transport(cfg) {
            Ok(Some(transport)) => transport,
            Ok(None) => {
                tracing::info!(
                    "email delivery disabled (SMTP_HOST / RESEND_API_KEY not set); reset links stay in dev mode"
                );
                return Self::disabled();
            }
            Err(err) => {
                tracing::error!(error = %err, "email: invalid configuration, delivery disabled");
                return Self::disabled();
            }
        };

        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        let transport = Arc::new(transport);
        let task_timeout = cfg.smtp_task_timeout;
        for _ in 0..WORKER_COUNT {
            let rx = rx.clone();
            let transport = transport.clone();
            tokio::spawn(async move {
                loop {
                    let payload = { rx.lock().await.recv().await };
                    let Some(payload) = payload else { break };
                    deliver(&transport, task_timeout, payload).await;
                }
            });
        }
        tracing::info!(workers = WORKER_COUNT, "email background workers started");
        Self { tasks: Some(tx) }
    }
}

impl Mailer for QueuedMailer {
    fn is_configured(&self) -> bool {
        self.tasks.is_some()
    }

    fn enqueue_password_reset_email(&self, to: &str, reset_link: &str) -> Result<(), String> {
        let Some(tasks) = &self.tasks else {
            return Err("email service not configured".to_string());
        };
        tasks
            .try_send(PasswordResetPayload {
                to: to.to_string(),
                reset_link: reset_link.to_string(),
            })
            .map_err(|err| match err {
                mpsc::error::TrySendError::Full(_) => "email queue is full".to_string(),
                mpsc::error::TrySendError::Closed(_) => "email queue is closed".to_string(),
            })
    }
}

fn has_smtp_config(cfg: &EmailConfig) -> bool {
    !cfg.smtp_host.is_empty() && cfg.smtp_port > 0 && !cfg.from.is_empty()
}

fn build_transport(cfg: &EmailConfig) -> Result<Option<Transport>, String> {
    if has_smtp_config(cfg) {
        let tls = TlsParameters::new(cfg.smtp_host.clone()).map_err(|err| err.to_string())?;
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.smtp_host)
            .port(cfg.smtp_port)
            .timeout(Some(cfg.smtp_timeout))
            .tls(if cfg.smtp_use_tls {
                Tls::Wrapper(tls)
            } else {
                Tls::Opportunistic(tls)
            });
        if !cfg.smtp_username.is_empty() || !cfg.smtp_password.is_empty() {
            builder = builder
                .credentials(Credentials::new(
                    cfg.smtp_username.clone(),
                    cfg.smtp_password.clone(),
                ))
                .authentication(vec![Mechanism::Plain]);
        }
        let from = cfg
            .from
            .parse::<Mailbox>()
            .map_err(|err| format!("invalid SMTP_FROM: {err}"))?;
        return Ok(Some(Transport::Smtp {
            transport: builder.build(),
            from,
        }));
    }

    if !cfg.resend_api_key.is_empty() {
        return Ok(Some(Transport::Resend {
            api_key: cfg.resend_api_key.clone(),
            from: cfg.from.clone(),
            client: reqwest::Client::new(),
        }));
    }

    Ok(None)
}

async fn deliver(transport: &Transport, task_timeout: Duration, payload: PasswordResetPayload) {
    let send = send_password_reset_email(transport, &payload.to, &payload.reset_link);
    let result = if task_timeout.is_zero() {
        send.await
    } else {
        tokio::time::timeout(task_timeout, send)
            .await
            .unwrap_or_else(|_| Err("email task timed out".to_string()))
    };
    if let Err(err) = result {
        tracing::error!(error = %err, to = %payload.to, "failed to send password reset email");
    }
}

async fn send_password_reset_email(
    transport: &Transport,
    to: &str,
    reset_link: &str,
) -> Result<(), String> {
    let (text, html) = password_reset_template(reset_link, "1 hour");
    let subject = "Reset your password";
    match transport {
        Transport::Smtp { transport, from } => {
            let to = to
                .parse::<Mailbox>()
                .map_err(|err| format!("invalid smtp recipient address: {err}"))?;
            let message = Message::builder()
                .from(from.clone())
                .to(to)
                .subject(subject)
                .multipart(MultiPart::alternative_plain_html(text, html))
                .map_err(|err| err.to_string())?;
            transport
                .send(message)
                .await
                .map(|_| ())
                .map_err(|err| format!("smtp send failed: {err}"))
        }
        Transport::Resend {
            api_key,
            from,
            client,
        } => send_resend(client, api_key, from, to, subject, &text, &html).await,
    }
}

#[derive(Serialize)]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    text: &'a str,
    html: &'a str,
}

async fn send_resend(
    client: &reqwest::Client,
    api_key: &str,
    from: &str,
    to: &str,
    subject: &str,
    text: &str,
    html: &str,
) -> Result<(), String> {
    let response = client
        .post(RESEND_API_URL)
        .bearer_auth(api_key)
        .json(&SendEmailRequest {
            from,
            to,
            subject,
            text,
            html,
        })
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

/// echobackend's `passwordResetTemplate`, verbatim.
fn password_reset_template(reset_link: &str, expires_in: &str) -> (String, String) {
    let text = format!(
        "We received a request to reset your password.\n\nReset your password here:\n{reset_link}\n\nThis link expires in {expires_in}. If you did not request a password reset, you can safely ignore this email."
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
    body {{ margin: 0; padding: 0; background: #ffffff; color: #111111; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Arial, sans-serif; }}
    .page {{ width: 100%; padding: 32px 16px; background: #ffffff; }}
    .container {{ max-width: 560px; margin: 0 auto; background: #ffffff; border: 1px solid #e5e5e5; }}
    .header {{ padding: 28px 32px 18px; border-bottom: 1px solid #eeeeee; }}
    .brand {{ margin: 0 0 12px; color: #555555; font-size: 13px; font-weight: 600; }}
    h1 {{ margin: 0; color: #111111; font-size: 23px; line-height: 1.3; font-weight: 600; }}
    .content {{ padding: 26px 32px 32px; }}
    p {{ margin: 0 0 16px; color: #333333; font-size: 16px; line-height: 1.6; }}
    .button-wrap {{ margin: 24px 0; }}
    .button {{ display: inline-block; background: #111111; color: #ffffff; padding: 12px 20px; text-decoration: none; border-radius: 4px; font-size: 15px; font-weight: 600; }}
    .meta {{ margin: 22px 0; padding: 14px 0; border-top: 1px solid #eeeeee; border-bottom: 1px solid #eeeeee; color: #555555; font-size: 14px; line-height: 1.5; }}
    .fallback {{ margin-top: 18px; padding-top: 18px; border-top: 1px solid #eeeeee; }}
    .fallback p {{ color: #666666; font-size: 13px; line-height: 1.5; }}
    .link {{ color: #111111; word-break: break-all; overflow-wrap: anywhere; }}
    .footer {{ max-width: 560px; margin: 16px auto 0; text-align: center; }}
    .footer p {{ color: #777777; font-size: 12px; line-height: 1.5; }}
    @media (max-width: 480px) {{
      .page {{ padding: 16px 10px; }}
      .header, .content {{ padding-left: 20px; padding-right: 20px; }}
      h1 {{ font-size: 22px; }}
      .button {{ display: block; text-align: center; }}
    }}
  </style>
</head>
<body>
  <div class="page">
    <div class="container">
      <div class="header">
        <p class="brand">Pilput</p>
        <h1>Reset your password</h1>
      </div>
      <div class="content">
        <p>We received a request to reset the password for your account. Use the button below to choose a new password.</p>
        <div class="button-wrap">
          <a href="{escaped_link}" class="button">Reset password</a>
        </div>
        <div class="meta">This link expires in <strong>{escaped_expires_in}</strong>. For your security, do not forward this email or share the reset link.</div>
        <div class="fallback">
          <p>If the button does not work, copy and paste this link into your browser:</p>
          <p><a href="{escaped_link}" class="link">{escaped_link}</a></p>
        </div>
      </div>
    </div>
    <div class="footer">
      <p>If you did not request a password reset, you can safely ignore this email.</p>
    </div>
  </div>
</body>
</html>"#
    );

    (text, html)
}

/// Go `html.EscapeString`.
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('\'', "&#39;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&#34;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email_config() -> EmailConfig {
        EmailConfig {
            resend_api_key: String::new(),
            from: "noreply@pilput.net".to_string(),
            frontend_reset_password_url: String::new(),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            smtp_use_tls: false,
            smtp_timeout: Duration::from_secs(10),
            smtp_task_timeout: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn unconfigured_mailer_rejects_enqueue() {
        let mailer = QueuedMailer::from_config(&email_config());
        assert!(!mailer.is_configured());
        assert_eq!(
            mailer.enqueue_password_reset_email("a@b.co", "http://x"),
            Err("email service not configured".to_string())
        );
    }

    #[tokio::test]
    async fn smtp_host_enables_delivery() {
        let mut cfg = email_config();
        cfg.smtp_host = "127.0.0.1".to_string();
        // Port 9 (discard) refuses quickly; the worker only logs the failure.
        cfg.smtp_port = 9;
        let mailer = QueuedMailer::from_config(&cfg);
        assert!(mailer.is_configured());
        assert!(
            mailer
                .enqueue_password_reset_email("a@b.co", "http://x")
                .is_ok()
        );
    }

    #[test]
    fn template_escapes_link() {
        let (text, html) = password_reset_template("https://x/?a=1&b=<2>", "1 hour");
        assert!(text.contains("https://x/?a=1&b=<2>"));
        assert!(html.contains("https://x/?a=1&amp;b=&lt;2&gt;"));
        assert!(html.contains("width: 100%;"));
    }
}
