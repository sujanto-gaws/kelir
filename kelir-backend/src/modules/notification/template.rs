//! Rendering what an outbound channel sends (FR-NTF-004; [#257] AC4, AC5).
//!
//! # A substitution, not a template engine
//!
//! `{{title}}` and `{{body}}`, and that is the whole language. A notification
//! row carries a title and a body; those are the values the sender can resolve,
//! so those are the placeholders a template may use.
//!
//! **The bound is not a style rule.** A template naming `{{dueDate}}` is a
//! template asking for something the sender was never given, and the honest
//! answer is that it does not render — see [`render`]. Widening the vocabulary
//! means widening what `service::notify` is handed, which is a change to every
//! caller rather than a string in a table.
//!
//! **And it is deliberately not a JSON Logic expression.** **D-10** bought one
//! evaluator for rules that decide things; a subject line is not a decision, and
//! putting the workflow's evaluator behind an email would make a mail template
//! able to divide by zero.
//!
//! [#257]: https://github.com/sujanto-gaws/kelir/issues/257

/// What a template may name, which is what a notification carries.
pub struct Context<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

/// The template with its placeholders filled, or **nothing**.
///
/// `None` means *this template asked for something that is not here* — an
/// unresolved `{{…}}` survived the substitution. The caller sends the
/// notification's own words instead and logs, which is [#257] AC5: a template
/// somebody mistyped must not turn into silence, and it must not turn into an
/// email with `{{dueDate}}` in the subject either.
///
/// **Unbalanced or empty braces are not a placeholder** and are left alone: a
/// body that legitimately contains `{{` is a body, not a broken template, and
/// refusing it would make the failure mode wider than the mistake.
pub fn render(template: &str, context: &Context<'_>) -> Option<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;

    // **One pass, and a substituted value is never re-scanned.** Two sequential
    // `replace` calls would expand a `{{body}}` that arrived *inside the title*,
    // which turns a person's words into a template — the place an injection
    // would live if there were anything here worth injecting into. Emitting to
    // an output buffer makes that impossible rather than unlikely.
    while let Some(open) = rest.find("{{") {
        let (before, after_open) = rest.split_at(open);
        rendered.push_str(before);

        let Some(close) = after_open.find("}}") else {
            // An unclosed `{{` is text: a body containing braces is a body, and
            // refusing it would make the failure mode wider than the mistake.
            rendered.push_str(after_open);
            return Some(rendered);
        };

        let name = &after_open[2..close];

        match name {
            "title" => rendered.push_str(context.title),
            "body" => rendered.push_str(context.body),
            // Not a placeholder at all — `{{}}`, or braces around punctuation.
            // Copied through, for the reason the unclosed case gives.
            "" => rendered.push_str("{{}}"),
            other if other.contains(['{', '}']) => {
                rendered.push_str(&after_open[..close + 2]);
            }
            // A placeholder naming something the sender cannot resolve. The
            // caller sends the notification plain and logs (#257 AC5).
            _ => return None,
        }

        rest = &after_open[close + 2..];
    }

    rendered.push_str(rest);

    Some(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context<'static> {
        Context {
            title: "A task is waiting for you",
            body: "Approve the request on PR-2026-000001",
        }
    }

    #[test]
    fn the_two_placeholders_are_filled() {
        assert_eq!(
            render("{{title}}", &context()).expect("a subject"),
            "A task is waiting for you"
        );
        assert_eq!(
            render("{{body}}\n\nOpen Kelir.", &context()).expect("a body"),
            "Approve the request on PR-2026-000001\n\nOpen Kelir."
        );
    }

    /// **A template asking for something the sender does not have renders
    /// nothing**, so the caller can send the notification plain (#257 AC5).
    ///
    /// The alternative — sending it with `{{dueDate}}` still in the subject —
    /// is the version of this failure that reaches a person's mailbox.
    #[test]
    fn a_placeholder_nobody_can_fill_fails_the_render() {
        assert!(render("{{title}} — due {{dueDate}}", &context()).is_none());
        assert!(render("{{recipientName}}", &context()).is_none());
    }

    /// A body that happens to contain braces is a body.
    #[test]
    fn braces_that_are_not_a_placeholder_are_left_alone() {
        assert_eq!(
            render("a JSON body: {{}} and {{ {} }}", &context()).expect("a body"),
            "a JSON body: {{}} and {{ {} }}"
        );
        assert_eq!(
            render("an unclosed {{ is still text", &context()).expect("a body"),
            "an unclosed {{ is still text"
        );
    }

    /// **The values are substituted, not re-scanned.** A notification whose own
    /// title contains `{{body}}` must not become a template — it is a person's
    /// words, and the second pass is where an injection would live.
    #[test]
    fn a_value_that_looks_like_a_placeholder_is_not_expanded_again() {
        let quoted = Context {
            title: "{{body}}",
            body: "the actual body",
        };

        assert_eq!(
            render("{{title}}", &quoted).expect("a subject"),
            "{{body}}",
            "the title was re-expanded, which makes a notification a template"
        );
    }
}
