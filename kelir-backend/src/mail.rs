//! Outbound mail.
//!
//! **This is not the notification module.** Phase 6 owns notification
//! templates, channels, retries and the outbox (SDD §6.11); this sends one
//! message directly over SMTP, because the password-reset flow (FR-AUTH-006,
//! #17) needs a link to reach a person and everything else about notifications
//! is a later phase's problem. When that module lands it should absorb this —
//! the seam is [`Mailer::send`], and there is exactly one caller.
//!
//! **A failure to send is not a failure of the request that caused it.** The
//! reset flow answers the same way whether the address existed or not, so a
//! send that fails cannot be reported to the caller without disclosing that it
//! was attempted. It is logged at `error` and swallowed, and
//! [`Mailer::send`]'s signature says so by returning `()`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::AppConfig;

/// One outbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// How mail leaves the process.
#[derive(Clone)]
pub enum Mailer {
    /// A real SMTP server — mailpit locally, a relay in a deployment.
    Smtp {
        transport: Arc<AsyncSmtpTransport<Tokio1Executor>>,
        from: String,
    },
    /// Logged and dropped.
    ///
    /// What a deployment with no SMTP host configured gets. It is not a silent
    /// no-op: the subject and recipient are logged at `warn`, because an
    /// operator who has not configured mail should be able to find out that the
    /// product tried to send some.
    Logged { from: String },
    /// Kept in memory, for tests.
    ///
    /// **The reason it exists rather than tests reading the token out of the
    /// database.** A test that fetches the token from `password_reset_tokens`
    /// proves the row was written; a test that reads it out of the delivered
    /// message proves the person could actually have used it — including that
    /// the link was built, addressed and formatted. The second is the flow.
    Captured {
        sent: Arc<Mutex<Vec<Mail>>>,
        from: String,
        /// How long a send takes before the message is kept.
        ///
        /// Zero for the harness's ordinary mailer. It exists because #202 was a
        /// timing defect that the suite could not see: `Mailer` is an enum, so
        /// a slow transport could not be injected, and a captured send is free
        /// — which made the one property worth asserting (that a caller waits
        /// for no part of delivery) unassertable. A delay here is that slow
        /// transport.
        delay: Duration,
    },
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Smtp { from, .. } => formatter.debug_struct("Smtp").field("from", from).finish(),
            Self::Logged { from } => formatter
                .debug_struct("Logged")
                .field("from", from)
                .finish(),
            Self::Captured { sent, from, delay } => formatter
                .debug_struct("Captured")
                .field("from", from)
                .field("delay", delay)
                .field(
                    "sent",
                    &sent.lock().map(|sent| sent.len()).unwrap_or_default(),
                )
                .finish(),
        }
    }
}

impl Mailer {
    /// Builds the mailer a configuration asks for.
    ///
    /// An empty `KELIR_SMTP_HOST` selects [`Mailer::Logged`]. That is the
    /// deployment saying it has no mail server, which is a legitimate state —
    /// the product still runs, and the one flow that sends mail degrades to a
    /// log line rather than a startup failure.
    pub fn from_config(config: &AppConfig) -> Result<Self, anyhow::Error> {
        let from = config.mail_from.clone();

        if config.smtp_host.trim().is_empty() {
            return Ok(Self::Logged { from });
        }

        // `builder_dangerous` is the plaintext builder, and the name is
        // accurate: it is right for mailpit on a loopback interface and wrong
        // for anything else. A host that is not local gets STARTTLS, so a
        // deployment cannot accidentally send reset links in the clear across a
        // network.
        let local = matches!(
            config.smtp_host.trim(),
            "localhost" | "127.0.0.1" | "::1" | "mailpit"
        );

        let transport = if local {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.smtp_host.trim())
                .port(config.smtp_port)
                .build()
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.smtp_host.trim())
                .port(config.smtp_port)
                .tls(Tls::Required(TlsParameters::new(
                    config.smtp_host.trim().to_owned(),
                )?))
                .build()
        };

        Ok(Self::Smtp {
            transport: Arc::new(transport),
            from,
        })
    }

    /// A mailer that keeps what it is given, for tests.
    pub fn captured() -> Self {
        Self::captured_taking(Duration::ZERO)
    }

    /// A [`Mailer::captured`] whose send takes `delay` before the message
    /// appears.
    ///
    /// The injectable slow transport [`Mailer::Captured::delay`] describes. A
    /// test that wants to prove a caller is not waiting for delivery needs
    /// delivery to be slow enough that waiting for it would be obvious.
    pub fn captured_taking(delay: Duration) -> Self {
        Self::Captured {
            sent: Arc::new(Mutex::new(Vec::new())),
            from: "no-reply@kelir.test".to_owned(),
            delay,
        }
    }

    /// Everything [`Mailer::Captured`] has been given. Empty for any other
    /// variant.
    pub fn captured_messages(&self) -> Vec<Mail> {
        match self {
            Self::Captured { sent, .. } => sent.lock().map(|sent| sent.clone()).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Hands a message to the runtime and returns without waiting for it.
    ///
    /// **The request that causes a send must not wait for one** (#202).
    /// `request_reset` awaited [`Mailer::send`], and `Smtp`'s send is a
    /// complete SMTP transaction: measured against mailpit on the loopback
    /// interface, a request for a known account answered in a p50 of 90ms and
    /// one for an unknown account in 9.8ms, with the two ranges not
    /// overlapping. That is an account-enumeration oracle on an endpoint that
    /// is deliberately not rate-limited, and it sat behind a module comment
    /// promising "no branch a caller can time".
    ///
    /// Nothing is lost by detaching it: [`Mailer::send`] already reports no
    /// failure to anybody, so awaiting it only ever bought the caller a
    /// measurement.
    ///
    /// **Two things it does cost, named rather than discovered.** A message
    /// still in flight when the process stops is lost — the person asks again
    /// and the next request sends one, which is the same outcome a failed
    /// delivery already had. And the send is no longer throttled by the request
    /// that caused it, so a flood of requests is a flood of concurrent SMTP
    /// transactions; what bounds that today is
    /// [`crate::modules::auth::reset::RESEND_COOLDOWN_SECONDS`], one message
    /// per account per minute, which is a bound on the flood and not on the
    /// number of accounts in it. Both belong to the notification module that
    /// absorbs this one in Phase 6 (SDD §6.11), where the queue this should
    /// hand to actually exists.
    pub fn send_detached(&self, mail: Mail) {
        let mailer = self.clone();

        tokio::spawn(async move { mailer.send(mail).await });
    }

    /// Sends, and never reports a failure to the caller.
    ///
    /// See the module comment: the one flow that sends mail must answer
    /// identically whether an address existed, so it cannot surface a delivery
    /// error either.
    ///
    /// **Called on the request's own path by nobody** — see
    /// [`Mailer::send_detached`], which is what the reset flow uses. It stays
    /// `pub` because the detached path calls it and its unit tests drive it
    /// directly.
    pub async fn send(&self, mail: Mail) {
        match self {
            Self::Logged { from } => {
                tracing::warn!(
                    to = %mail.to,
                    from = %from,
                    subject = %mail.subject,
                    "no SMTP host is configured, so this message was not sent"
                );
            }
            Self::Captured { sent, delay, .. } => {
                if !delay.is_zero() {
                    tokio::time::sleep(*delay).await;
                }

                if let Ok(mut sent) = sent.lock() {
                    sent.push(mail);
                }
            }
            Self::Smtp { transport, from } => {
                let built = Message::builder()
                    .from(match from.parse() {
                        Ok(address) => address,
                        Err(error) => {
                            tracing::error!(%error, from = %from, "the configured mail sender address is not valid");
                            return;
                        }
                    })
                    .to(match mail.to.parse() {
                        Ok(address) => address,
                        Err(error) => {
                            // Not logged with the address: it is a user's email,
                            // and an invalid one is still personal data.
                            tracing::error!(%error, "a recipient address was not valid");
                            return;
                        }
                    })
                    .subject(mail.subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(mail.body);

                match built {
                    Ok(message) => {
                        if let Err(error) = transport.send(message).await {
                            tracing::error!(%error, "sending mail failed");
                        }
                    }
                    Err(error) => tracing::error!(%error, "building the message failed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_captured_mailer_keeps_what_it_is_given() {
        let mailer = Mailer::captured();

        mailer
            .send(Mail {
                to: "someone@kelir.test".to_owned(),
                subject: "Reset your password".to_owned(),
                body: "link".to_owned(),
            })
            .await;

        let sent = mailer.captured_messages();

        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "someone@kelir.test");
    }

    #[tokio::test]
    async fn a_logged_mailer_drops_the_message_without_failing() {
        // The property that matters: no panic, no error, nothing for a caller
        // to branch on. A deployment without SMTP still serves the flow.
        let mailer = Mailer::Logged {
            from: "no-reply@kelir.test".to_owned(),
        };

        mailer
            .send(Mail {
                to: "someone@kelir.test".to_owned(),
                subject: "Reset your password".to_owned(),
                body: "link".to_owned(),
            })
            .await;

        assert!(mailer.captured_messages().is_empty());
    }

    #[test]
    fn no_smtp_host_selects_the_logged_mailer() {
        let mut config = AppConfig::test_default();
        config.smtp_host = String::new();

        assert!(matches!(
            Mailer::from_config(&config).expect("builds"),
            Mailer::Logged { .. }
        ));
    }

    #[test]
    fn an_smtp_host_selects_the_smtp_mailer() {
        let mut config = AppConfig::test_default();
        config.smtp_host = "mailpit".to_owned();

        assert!(matches!(
            Mailer::from_config(&config).expect("builds"),
            Mailer::Smtp { .. }
        ));
    }

    /// The swallowing property, against a transport that genuinely fails.
    ///
    /// [`Mailer::Captured`] and [`Mailer::Logged`] cannot demonstrate it: they
    /// never fail, so a test using them would pass even if `send` had been
    /// written to panic on a transport error. This one points a real
    /// [`AsyncSmtpTransport`] at a port nothing is listening on, so the send
    /// path runs end to end — address parsing, message building, connect — and
    /// the connect loses. The flow above it must not be able to tell.
    #[tokio::test]
    async fn an_smtp_send_that_fails_is_swallowed_rather_than_raised() {
        let mut config = AppConfig::test_default();
        config.smtp_host = "127.0.0.1".to_owned();
        // Port 1 needs no privilege to *connect* to and has nothing behind it.
        config.smtp_port = 1;

        let mailer = Mailer::from_config(&config).expect("builds");
        assert!(matches!(mailer, Mailer::Smtp { .. }));

        // The assertion is that this returns at all: `send` has no error type,
        // so a caller has nothing to branch on, and a panic here would take the
        // request down and make a failed delivery observable to the caller —
        // which is what the flow's uniform answer depends on not happening.
        mailer
            .send(Mail {
                to: "someone@kelir.test".to_owned(),
                subject: "Reset your password".to_owned(),
                body: "link".to_owned(),
            })
            .await;
    }

    /// A recipient address that cannot be parsed is dropped, not panicked on.
    ///
    /// It reaches `send` from the database, so it is not the request's to
    /// validate — an address that was acceptable when the account was created
    /// can still be unbuildable here.
    #[tokio::test]
    async fn an_unparseable_recipient_is_dropped_rather_than_raised() {
        let mut config = AppConfig::test_default();
        config.smtp_host = "127.0.0.1".to_owned();
        config.smtp_port = 1;

        let mailer = Mailer::from_config(&config).expect("builds");

        mailer
            .send(Mail {
                to: "not an address".to_owned(),
                subject: "Reset your password".to_owned(),
                body: "link".to_owned(),
            })
            .await;
    }
}
