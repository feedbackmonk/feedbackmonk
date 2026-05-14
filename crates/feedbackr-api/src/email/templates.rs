//! Plain-text email templates parameterised by `EmailTenantBrand`
//! (Contract C10). One renderer per email kind, each producing a
//! `RenderedEmail { subject, body }`. The mailer composes the From/To
//! envelope from the brand + submitter context.
//!
//! Plain-text only per FR-FBR-09 ("Status emails (plain-text)") and the
//! P1 plan's Deferred Decisions resolution. Markdown / HTML is NOT a
//! Stage-2 deliverable; the body field is the final wire bytes.
//!
//! Ports the parameterization shape from
//! `gitcellar-cloud/src/feedback/email_templates.rs` (READ-ONLY reference
//! per DEC-FBR-07).

use feedbackr_core::{FeedbackId, FeedbackStatus};
use feedbackr_repository::EmailTenantBrand;

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
pub fn render_confirmation(brand: &EmailTenantBrand, ctx: &ConfirmationContext<'_>) -> RenderedEmail {
    let subject = format_subject(brand, ctx.feedback_id, "We received your feedback");
    let body = format!(
        "Thanks for sending feedback to {brand_name}.\n\
         \n\
         Reference: {fb_id}\n\
         \n\
         Your message:\n\
         {body_excerpt}\n\
         \n\
         {footer}",
        brand_name = brand.brand_name,
        fb_id = ctx.feedback_id,
        body_excerpt = ctx.body_excerpt,
        footer = render_footer(brand),
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
) -> RenderedEmail {
    let short_subject = format!(
        "Status updated: {}",
        status_human(ctx.to_status)
    );
    let subject = format_subject(brand, ctx.feedback_id, &short_subject);

    let mut body = format!(
        "Your feedback {fb_id} was updated.\n\
         \n\
         Previous status: {from}\n\
         New status:      {to}\n",
        fb_id = ctx.feedback_id,
        from = status_human(ctx.from_status),
        to = status_human(ctx.to_status),
    );
    if let Some(note) = ctx.reason_note {
        body.push_str("\nNote from the team:\n");
        body.push_str(note);
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&render_footer(brand));
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
) -> RenderedEmail {
    let subject = format_subject(brand, ctx.feedback_id, "Reply from the team");
    let body = format!(
        "The {brand_name} team replied to your feedback {fb_id}.\n\
         \n\
         {reply_body}\n\
         \n\
         {footer}",
        brand_name = brand.brand_name,
        fb_id = ctx.feedback_id,
        reply_body = ctx.reply_body,
        footer = render_footer(brand),
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
fn render_footer(brand: &EmailTenantBrand) -> String {
    let mut s = format!(
        "{footer_signature}\n\
         ---\n\
         You are receiving this because you submitted feedback to {brand_name}.\n\
         Reply to this email or contact {support_email}.\n",
        footer_signature = brand.footer_signature,
        brand_name = brand.brand_name,
        support_email = brand.support_email,
    );
    if let Some(url) = &brand.unsubscribe_url {
        s.push_str("Unsubscribe: ");
        s.push_str(url);
        s.push('\n');
    }
    s
}

fn status_human(s: FeedbackStatus) -> &'static str {
    match s {
        FeedbackStatus::Submitted => "Submitted",
        FeedbackStatus::Triaged => "Triaged",
        FeedbackStatus::InProgress => "In progress",
        FeedbackStatus::Shipped => "Shipped",
        FeedbackStatus::WontFix => "Won't fix",
        FeedbackStatus::Duplicate => "Duplicate",
    }
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
            ).body,
            render_status_change(
                &brand,
                &StatusChangeContext {
                    feedback_id: &id,
                    from_status: FeedbackStatus::InProgress,
                    to_status: FeedbackStatus::Shipped,
                    reason_note: None,
                },
            ).body,
            render_public_reply(
                &brand,
                &PublicReplyContext { feedback_id: &id, reply_body: "y" },
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
