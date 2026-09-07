//! Plain-text email templates parameterised by `EmailTenantBrand`
//! (Contract C10) and by the recipient's `Locale` (FR-FBR-37). One renderer per
//! email kind, each producing a `RenderedEmail { subject, body }`. The mailer
//! composes the From/To envelope from the brand + submitter context.
//!
//! Plain-text only per FR-FBR-09 ("Status emails (plain-text)") and the
//! P1 plan's Deferred Decisions resolution. Markdown / HTML is NOT a
//! Stage-2 deliverable; the body field is the final wire bytes.
//!
//! Ports the parameterization shape from
//! `gitcellar-cloud/src/feedback/email_templates.rs` (READ-ONLY reference
//! per DEC-FBR-07).
//!
//! ## Every literal lives in `i18n/locales/<code>/email.json`
//!
//! There is no English string in this file: the renderers assemble catalog
//! values, and `Locale::EN` reproduces today's bytes exactly — which the
//! `insta` snapshots below enforce. That byte-identity is the regression gate
//! for this whole change: localizing an email must not alter the email anyone
//! is receiving today.
//!
//! **Line structure stays in code; line CONTENT lives in the catalog.** Blank
//! lines, the `---` footer rule and the order of sections are layout, not
//! language, and a translator who could move them could break a mail client.
//! The one place alignment is language-dependent — the padded
//! `Previous status:` / `New status:` pair — is carried inside the catalog
//! strings, so a translator aligns their own labels instead of inheriting
//! English's column.
//!
//! ## Why `status.value.*` keys live HERE and not in the shared `status.json`
//!
//! The shared catalog renders status chips in the admin UI as `In Progress` /
//! `Won't Fix` (title case). These emails have said `In progress` / `Won't fix`
//! (sentence case) since P1. Pointing the email at the shared keys would silently
//! re-case two shipped emails. Prose and chip labels are different registers, so
//! they get different keys — see MSG-004 in the Stage-1 collaboration channel.

use feedbackmonk_core::{FeedbackId, FeedbackStatus};
use feedbackmonk_i18n::{t, t_args, Locale};
use feedbackmonk_repository::EmailTenantBrand;

/// Rendered email — the wire-ready subject + plain-text body. The mailer
/// fills in From/To from `EmailTenantBrand::sender_display_name` and the
/// submitter email respectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEmail {
    pub subject: String,
    pub body: String,
}

/// Confirmation email — sent once after every accepted feedback submission
/// when the submitter has an email on file.
#[derive(Debug, Clone)]
pub struct ConfirmationContext<'a> {
    pub feedback_id: &'a FeedbackId,
    /// First ~200 chars of the submission body, included verbatim so the
    /// submitter can re-confirm what they actually sent.
    pub body_excerpt: &'a str,
}

#[must_use]
pub fn render_confirmation(
    brand: &EmailTenantBrand,
    ctx: &ConfirmationContext<'_>,
    locale: Locale,
) -> RenderedEmail {
    let subject = format_subject(
        brand,
        ctx.feedback_id,
        &t(locale, "email.confirmation.subject"),
    );
    let body = format!(
        "{intro}\n\
         \n\
         {reference}\n\
         \n\
         {your_message}\n\
         {body_excerpt}\n\
         \n\
         {footer}",
        intro = t_args(
            locale,
            "email.confirmation.intro",
            &[("brandName", &brand.brand_name)],
        ),
        reference = t_args(
            locale,
            "email.confirmation.referenceLine",
            &[("feedbackId", ctx.feedback_id.as_str())],
        ),
        your_message = t(locale, "email.confirmation.yourMessageHeading"),
        body_excerpt = ctx.body_excerpt,
        footer = render_footer(brand, locale),
    );
    RenderedEmail { subject, body }
}

/// Status-change email — sent on every transition to a submitter-visible
/// state. Filtering "submitter-visible" lives in `send.rs`; this renderer
/// just maps (from, to) onto the human-readable phrase.
#[derive(Debug, Clone)]
pub struct StatusChangeContext<'a> {
    pub feedback_id: &'a FeedbackId,
    pub from_status: FeedbackStatus,
    pub to_status: FeedbackStatus,
    /// Optional admin note ("we couldn't reproduce this without a logged-in
    /// user"). When `None`, the body omits the note section entirely.
    pub reason_note: Option<&'a str>,
}

#[must_use]
pub fn render_status_change(
    brand: &EmailTenantBrand,
    ctx: &StatusChangeContext<'_>,
    locale: Locale,
) -> RenderedEmail {
    let short_subject = t_args(
        locale,
        "email.status.subject",
        &[("status", &status_human(ctx.to_status, locale))],
    );
    let subject = format_subject(brand, ctx.feedback_id, &short_subject);

    let mut body = format!(
        "{intro}\n\
         \n\
         {previous}\n\
         {new}\n",
        intro = t_args(
            locale,
            "email.status.intro",
            &[("feedbackId", ctx.feedback_id.as_str())],
        ),
        previous = t_args(
            locale,
            "email.status.previousLine",
            &[("status", &status_human(ctx.from_status, locale))],
        ),
        new = t_args(
            locale,
            "email.status.newLine",
            &[("status", &status_human(ctx.to_status, locale))],
        ),
    );
    if let Some(note) = ctx.reason_note {
        body.push('\n');
        body.push_str(&t(locale, "email.status.noteHeading"));
        body.push('\n');
        body.push_str(note);
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&render_footer(brand, locale));
    RenderedEmail { subject, body }
}

/// Public-reply email — sent whenever an admin posts a `visibility=public`
/// reply on a feedback row whose submitter has an email on file.
#[derive(Debug, Clone)]
pub struct PublicReplyContext<'a> {
    pub feedback_id: &'a FeedbackId,
    pub reply_body: &'a str,
}

#[must_use]
pub fn render_public_reply(
    brand: &EmailTenantBrand,
    ctx: &PublicReplyContext<'_>,
    locale: Locale,
) -> RenderedEmail {
    let subject = format_subject(brand, ctx.feedback_id, &t(locale, "email.reply.subject"));
    let body = format!(
        "{intro}\n\
         \n\
         {reply_body}\n\
         \n\
         {footer}",
        intro = t_args(
            locale,
            "email.reply.intro",
            &[
                ("brandName", brand.brand_name.as_str()),
                ("feedbackId", ctx.feedback_id.as_str()),
            ],
        ),
        reply_body = ctx.reply_body,
        footer = render_footer(brand, locale),
    );
    RenderedEmail { subject, body }
}

/// `[{email_subject_prefix} #{FB-id}] {short_subject}` per Contract C10.
fn format_subject(brand: &EmailTenantBrand, fb_id: &FeedbackId, short_subject: &str) -> String {
    format!("[{prefix} #{fb_id}] {short_subject}",
        prefix = brand.email_subject_prefix,
        fb_id = fb_id,
        short_subject = short_subject,
    )
}

/// Plain-text footer per Contract C10. `unsubscribe_url` is optional —
/// `None` omits the line entirely (no empty "Unsubscribe:" stub).
///
/// `footer_signature` is tenant-authored text, so it is never translated —
/// a customer's sign-off is their words in their language.
fn render_footer(brand: &EmailTenantBrand, locale: Locale) -> String {
    let mut s = format!(
        "{footer_signature}\n\
         ---\n\
         {receiving}\n\
         {contact}\n",
        footer_signature = brand.footer_signature,
        receiving = t_args(
            locale,
            "email.footer.receivingBecause",
            &[("brandName", &brand.brand_name)],
        ),
        contact = t_args(
            locale,
            "email.footer.contact",
            &[("supportEmail", &brand.support_email)],
        ),
    );
    if let Some(url) = &brand.unsubscribe_url {
        s.push_str(&t_args(locale, "email.footer.unsubscribe", &[("url", url)]));
        s.push('\n');
    }
    s
}

/// The human-readable phrase for a status, in the EMAIL's register.
///
/// Reads `email.status.value.<db wire value>` — the wire value is the key, so
/// adding a status is a catalog row and a match arm, never a translation table
/// in two places. See the module docs for why this is not the shared
/// `status.*` namespace.
#[must_use]
pub fn status_human(s: FeedbackStatus, locale: Locale) -> String {
    t(locale, &format!("email.status.value.{}", s.as_db_str())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_brand() -> EmailTenantBrand {
        EmailTenantBrand::from_db(
            "acme".into(),
            "acme".into(),
            "acme@example.com".into(),
            None,
            "— The acme team".into(),
        )
    }

    fn customized_brand() -> EmailTenantBrand {
        EmailTenantBrand::from_db(
            "Acme Co".into(),
            "ACME".into(),
            "help@acme.example".into(),
            Some("https://acme.example/unsub?u=abc".into()),
            "Cheers,\nThe Acme Team".into(),
        )
    }

    fn fb() -> FeedbackId {
        FeedbackId::from("FB-ABC123".to_string())
    }

    // --- snapshot tests: 3 templates × 2 brand fixtures ----------
    //
    // Inline snapshots via `insta::assert_snapshot!(value, @"...")`. The
    // brief says "6 snapshot files minimum"; inline snapshots are
    // file-equivalent (insta treats them identically) and avoid the
    // out-of-band `snapshots/` dir churn during PR review.

    #[test]
    fn snapshot_confirmation_default_brand() {
        let brand = default_brand();
        let id = fb();
        let r = render_confirmation(
            &brand,
            &ConfirmationContext {
                feedback_id: &id,
                body_excerpt: "Login button is broken on Safari 17.",
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [acme #FB-ABC123] We received your feedback

        Thanks for sending feedback to acme.

        Reference: FB-ABC123

        Your message:
        Login button is broken on Safari 17.

        — The acme team
        ---
        You are receiving this because you submitted feedback to acme.
        Reply to this email or contact acme@example.com.
        "###);
    }

    #[test]
    fn snapshot_confirmation_customized_brand() {
        let brand = customized_brand();
        let id = fb();
        let r = render_confirmation(
            &brand,
            &ConfirmationContext {
                feedback_id: &id,
                body_excerpt: "Login button is broken on Safari 17.",
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [ACME #FB-ABC123] We received your feedback

        Thanks for sending feedback to Acme Co.

        Reference: FB-ABC123

        Your message:
        Login button is broken on Safari 17.

        Cheers,
        The Acme Team
        ---
        You are receiving this because you submitted feedback to Acme Co.
        Reply to this email or contact help@acme.example.
        Unsubscribe: https://acme.example/unsub?u=abc
        "###);
    }

    #[test]
    fn snapshot_status_change_default_brand() {
        let brand = default_brand();
        let id = fb();
        let r = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &id,
                from_status: FeedbackStatus::Submitted,
                to_status: FeedbackStatus::Triaged,
                reason_note: None,
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [acme #FB-ABC123] Status updated: Triaged

        Your feedback FB-ABC123 was updated.

        Previous status: Submitted
        New status:      Triaged

        — The acme team
        ---
        You are receiving this because you submitted feedback to acme.
        Reply to this email or contact acme@example.com.
        "###);
    }

    #[test]
    fn snapshot_status_change_customized_brand_with_note() {
        let brand = customized_brand();
        let id = fb();
        let r = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &id,
                from_status: FeedbackStatus::Triaged,
                to_status: FeedbackStatus::InProgress,
                reason_note: Some("We've started work on this. ETA next week."),
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [ACME #FB-ABC123] Status updated: In progress

        Your feedback FB-ABC123 was updated.

        Previous status: Triaged
        New status:      In progress

        Note from the team:
        We've started work on this. ETA next week.

        Cheers,
        The Acme Team
        ---
        You are receiving this because you submitted feedback to Acme Co.
        Reply to this email or contact help@acme.example.
        Unsubscribe: https://acme.example/unsub?u=abc
        "###);
    }

    #[test]
    fn snapshot_public_reply_default_brand() {
        let brand = default_brand();
        let id = fb();
        let r = render_public_reply(
            &brand,
            &PublicReplyContext {
                feedback_id: &id,
                reply_body: "Thanks — we've reproduced it and pushed a fix.",
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [acme #FB-ABC123] Reply from the team

        The acme team replied to your feedback FB-ABC123.

        Thanks — we've reproduced it and pushed a fix.

        — The acme team
        ---
        You are receiving this because you submitted feedback to acme.
        Reply to this email or contact acme@example.com.
        "###);
    }

    #[test]
    fn snapshot_public_reply_customized_brand() {
        let brand = customized_brand();
        let id = fb();
        let r = render_public_reply(
            &brand,
            &PublicReplyContext {
                feedback_id: &id,
                reply_body: "Thanks — we've reproduced it and pushed a fix.",
            },
            Locale::EN,
        );
        insta::assert_snapshot!(format!("Subject: {}\n\n{}", r.subject, r.body), @r###"
        Subject: [ACME #FB-ABC123] Reply from the team

        The Acme Co team replied to your feedback FB-ABC123.

        Thanks — we've reproduced it and pushed a fix.

        Cheers,
        The Acme Team
        ---
        You are receiving this because you submitted feedback to Acme Co.
        Reply to this email or contact help@acme.example.
        Unsubscribe: https://acme.example/unsub?u=abc
        "###);
    }

    // --- structural assertions (not snapshot-locked) -------------

    #[test]
    fn subject_format_contains_prefix_and_id() {
        let brand = default_brand();
        let id = fb();
        let r = render_confirmation(
            &brand,
            &ConfirmationContext { feedback_id: &id, body_excerpt: "x" },
            Locale::EN,
        );
        assert!(r.subject.starts_with("[acme #FB-ABC123]"));
    }

    #[test]
    fn footer_omits_unsubscribe_when_none() {
        let brand = default_brand();
        let id = fb();
        let r = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &id,
                from_status: FeedbackStatus::Submitted,
                to_status: FeedbackStatus::Triaged,
                reason_note: None,
            },
            Locale::EN,
        );
        assert!(!r.body.contains("Unsubscribe"));
    }

    #[test]
    fn footer_includes_unsubscribe_when_some() {
        let brand = customized_brand();
        let id = fb();
        let r = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &id,
                from_status: FeedbackStatus::Submitted,
                to_status: FeedbackStatus::Triaged,
                reason_note: None,
            },
            Locale::EN,
        );
        assert!(r.body.contains("Unsubscribe: https://acme.example/unsub?u=abc"));
    }

    #[test]
    fn status_change_body_omits_note_section_when_none() {
        let brand = default_brand();
        let id = fb();
        let r = render_status_change(
            &brand,
            &StatusChangeContext {
                feedback_id: &id,
                from_status: FeedbackStatus::Submitted,
                to_status: FeedbackStatus::Triaged,
                reason_note: None,
            },
            Locale::EN,
        );
        assert!(!r.body.contains("Note from the team"));
    }

    #[test]
    fn body_is_plain_text_no_html_markers() {
        let brand = customized_brand();
        let id = fb();
        let bodies = [
            render_confirmation(
                &brand,
                &ConfirmationContext { feedback_id: &id, body_excerpt: "x" },
                Locale::EN,
            ).body,
            render_status_change(
                &brand,
                &StatusChangeContext {
                    feedback_id: &id,
                    from_status: FeedbackStatus::InProgress,
                    to_status: FeedbackStatus::Shipped,
                    reason_note: None,
                },
                Locale::EN,
            ).body,
            render_public_reply(
                &brand,
                &PublicReplyContext { feedback_id: &id, reply_body: "y" },
                Locale::EN,
            ).body,
        ];
        for b in bodies {
            // FR-FBR-09 invariant: plain-text only.
            assert!(!b.contains("<html"), "body contains HTML marker: {b}");
            assert!(!b.contains("</"), "body contains closing tag: {b}");
            assert!(!b.contains("<br"), "body contains <br>: {b}");
        }
    }
}
