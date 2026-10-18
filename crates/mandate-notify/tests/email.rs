//! E8-11 (notifications spec §4.4; DEC-700 item 4, DEC-820 item 4; DEC-2508): the founder's mail.
//! Its subject is the notice's fixed text, and its body the text, the sentence, the link and the
//! founder's footer, with no greeting. Its HTML fetches nothing, and its wire form has exactly one
//! `To`, the bound address, and no `Cc`, `Bcc`, `Reply-To` or `List-Unsubscribe`. The address
//! parses only as one plain address, so a second recipient, a `Cc` or a `Bcc` cannot be smuggled
//! in, and the brief is not mailed. Every capture passes NT-1's canary scan.

mod common;

use common::{ORIGIN, Recording, answer};
use mandate_notify::canary::scan;
use mandate_notify::email::{
    FOUNDER_FOOTER, FounderAddress, FounderMail, OPEN_SENTENCE, SENDER, founder_mail,
};
use mandate_notify::{NoticeId, Notification, NotifyError, Origin, TextKey, link};

const FOUNDER: &str = "founder@mail.example";

fn origin() -> Origin {
    answer("origin", Origin::parse(ORIGIN))
}

fn notification(seed: u64, text: TextKey) -> Notification {
    let notice = answer("notice", NoticeId::mint(&mut Recording::seeded(seed)));
    Notification { notice, text }
}

/// The keys a mail is rendered for: every one but the brief's.
const MAILED: [TextKey; 3] = [
    TextKey::ApprovalNeeded,
    TextKey::AttentionNeeded,
    TextKey::AccountChanged,
];

fn mail(seed: u64, text: TextKey) -> (Notification, FounderMail) {
    let notification = notification(seed, text);
    let mail = answer("mail", founder_mail(&origin(), &notification));
    (notification, mail)
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// DEC-820 item 4: one plain address and nothing else. A list, a display name, a comment, a quote,
/// a space, a line break that would start a `Cc` or `Bcc` header, a missing or doubled `@`, and
/// anything outside printable ASCII are each refused.
#[test]
#[ignore = "pending E8-11"]
fn the_founder_address_is_exactly_one_plain_address() {
    for good in [FOUNDER, "f.o+under@sub.mail.example", "a@b"] {
        answer(good, FounderAddress::parse(good));
    }
    for bad in [
        "",
        "founder",
        "@mail.example",
        "founder@",
        "founder@@mail.example",
        "a@mail.example@b.example",
        "founder@mail.example, other@mail.example",
        "founder@mail.example;other@mail.example",
        "founder@mail.example other@mail.example",
        "Founder <founder@mail.example>",
        "<founder@mail.example>",
        "founder@mail.example (me)",
        "\"founder\"@mail.example",
        "founder@mail.example\r\nCc: other@mail.example",
        "founder@mail.example\r\nBcc: other@mail.example",
        "founder@mail.example\nBcc: other@mail.example",
        "founder@mail.example\r",
        " founder@mail.example",
        "founder@mail.example\t",
        "fóunder@mail.example",
        "founder@mail.example\u{0}",
    ] {
        match FounderAddress::parse(bad) {
            Err(NotifyError::NotAnAddress) => {}
            Err(e) => panic!("{bad:?} is refused not_an_address, not: {e:?}"),
            Ok(_) => panic!("{bad:?} is refused"),
        }
    }
}

/// Spec §4.4: the subject is the fixed text; the plain-text body is the text, the sentence, the
/// link and the founder's footer, in that order and with no greeting; the link is exactly
/// `<origin>/n/<notice id>` (NT-4) and appears once.
#[test]
#[ignore = "pending E8-11"]
fn the_mail_is_the_text_the_sentence_the_link_and_the_founders_footer() {
    for (seed, key) in MAILED.into_iter().enumerate() {
        let (notification, mail) = mail(seed as u64, key);
        let text = answer("text", key.text());
        let link = answer("link", link(&origin(), &notification.notice));
        assert_eq!(mail.subject, text, "{key:?}");
        assert_eq!(
            mail.text,
            format!("{text}\n\n{OPEN_SENTENCE}\n{link}\n\n{FOUNDER_FOOTER}\n"),
            "{key:?}"
        );
        assert_eq!(count(&mail.text, "https://"), 1, "one link: {key:?}");
        assert_eq!(
            count(&mail.html, &link),
            2,
            "the href and its text: {key:?}"
        );
        for part in [text, OPEN_SENTENCE, FOUNDER_FOOTER] {
            assert_eq!(
                count(&mail.html, part),
                1,
                "{part:?} in the HTML of {key:?}"
            );
        }
    }
}

/// Spec §4.4: the HTML part fetches nothing and styles nothing from elsewhere: no image, source,
/// stylesheet, script, frame, form, `style` or `url(`, and its only URL is the notice's link.
#[test]
#[ignore = "pending E8-11"]
fn the_html_fetches_nothing() {
    for (seed, key) in MAILED.into_iter().enumerate() {
        let (notification, mail) = mail(seed as u64, key);
        let html = mail.html.to_ascii_lowercase();
        for banned in [
            "<img",
            "src=",
            "<link",
            "<script",
            "<style",
            "style=",
            "url(",
            "<iframe",
            "<object",
            "<embed",
            "<form",
            "srcset",
            "background",
            "@import",
        ] {
            assert!(!html.contains(banned), "{banned} in {key:?}'s HTML");
        }
        let link = answer("link", link(&origin(), &notification.notice));
        assert_eq!(count(&html, "https://"), count(&html, &link), "{key:?}");
        assert_eq!(count(&html, "http://"), 0, "{key:?}");
    }
}

/// DEC-2508: the brief is not mailed until its one-click unsubscribe endpoint and counsel's
/// wording exist.
#[test]
#[ignore = "pending E8-11"]
fn the_brief_is_not_mailed() {
    match founder_mail(&origin(), &notification(7, TextKey::BriefReady)) {
        Err(NotifyError::MailNotPermitted) => {}
        Err(e) => panic!("the brief is refused mail_not_permitted, not: {e:?}"),
        Ok(mail) => panic!("the brief is not mailed, not: {mail:?}"),
    }
}

/// The header names of `wire`, each with its value, up to the blank line that ends them.
fn headers(wire: &str) -> Vec<(String, String)> {
    let (head, _) = wire
        .split_once("\r\n\r\n")
        .expect("headers end in a blank line");
    head.split("\r\n")
        .map(|line| {
            let (name, value) = line.split_once(": ").unwrap_or_else(|| panic!("{line:?}"));
            (name.to_ascii_lowercase(), value.to_owned())
        })
        .collect()
}

/// DEC-820 item 4, rung 1: the wire form names the founder's address once, as its one `To`, from
/// [`SENDER`]; it has no `Cc`, `Bcc`, `Reply-To`, `List-Unsubscribe` or other recipient header,
/// every line ends in CRLF, and it carries both parts.
#[test]
#[ignore = "pending E8-11"]
fn the_wire_form_has_one_recipient_and_nothing_else_addressed() {
    let to = answer("address", FounderAddress::parse(FOUNDER));
    for (seed, key) in MAILED.into_iter().enumerate() {
        let (_, mail) = mail(seed as u64, key);
        let wire = answer("wire", mail.wire(&to));
        let wire = String::from_utf8(wire).expect("the wire form is text");
        let headers = headers(&wire);
        let names: Vec<&str> = headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["from", "to", "subject", "mime-version", "content-type"],
            "{key:?}"
        );
        assert_eq!(headers[0].1, SENDER);
        assert_eq!(headers[1].1, FOUNDER);
        assert_eq!(headers[2].1, mail.subject);
        assert!(
            headers[4].1.starts_with("multipart/alternative;"),
            "{key:?}"
        );
        assert_eq!(
            count(&wire, FOUNDER),
            1,
            "the address appears once: {key:?}"
        );
        assert_eq!(
            count(&wire, "@"),
            2,
            "the sender and the founder only: {key:?}"
        );
        assert_eq!(
            count(&wire, "\n"),
            count(&wire, "\r\n"),
            "CRLF line ends: {key:?}"
        );
        assert!(
            wire.contains("Content-Type: text/plain; charset=utf-8"),
            "{key:?}"
        );
        assert!(
            wire.contains("Content-Type: text/html; charset=utf-8"),
            "{key:?}"
        );
        assert!(wire.contains(&mail.text.replace('\n', "\r\n")), "{key:?}");
    }
}

/// NT-1: nothing a captured mail holds is trading data. The wire form of every mailed key, to an
/// address that is itself a canary-free plain address, holds no canary.
#[test]
#[ignore = "pending E8-11"]
fn every_mail_passes_the_canary_scan() {
    let to = answer("address", FounderAddress::parse(FOUNDER));
    for (seed, key) in MAILED.into_iter().enumerate() {
        let (_, mail) = mail(seed as u64 + 100, key);
        let wire = answer("wire", mail.wire(&to));
        assert_eq!(answer("scan", scan(&wire)), Vec::<&str>::new(), "{key:?}");
    }
}

/// The HTML's tags and text, in order: each `<...>` as a tag, and each run of text between tags,
/// trimmed, when it is not empty.
fn html_parts(html: &str) -> (Vec<String>, Vec<String>) {
    let (mut tags, mut texts) = (Vec::new(), Vec::new());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        let text = rest[..open].trim();
        if !text.is_empty() {
            texts.push(text.to_owned());
        }
        let close = rest[open..].find('>').expect("every tag closes") + open;
        tags.push(rest[open + 1..close].to_owned());
        rest = &rest[close + 1..];
    }
    assert!(
        rest.trim().is_empty(),
        "nothing follows the last tag: {rest:?}"
    );
    (tags, texts)
}

/// Spec §4.4, against an allowlist rather than a list of what to refuse: the HTML's tags are
/// exactly the doctype, `html`, `body`, `p`, `br` and one `a` whose only attribute is the notice's
/// link, and its text is exactly the four parts, in order.
#[test]
#[ignore = "pending E8-11"]
fn the_html_is_only_allowlisted_tags_around_the_four_parts() {
    for (seed, key) in MAILED.into_iter().enumerate() {
        let (notification, mail) = mail(seed as u64, key);
        let link = answer("link", link(&origin(), &notification.notice));
        let (tags, texts) = html_parts(&mail.html);
        let anchor = format!("a href=\"{link}\"");
        let allowed = [
            "!doctype html",
            "html",
            "/html",
            "body",
            "/body",
            "p",
            "/p",
            "br",
            anchor.as_str(),
            "/a",
        ];
        for tag in &tags {
            assert!(allowed.contains(&tag.as_str()), "{tag:?} in {key:?}'s HTML");
        }
        assert_eq!(count(&tags.join(" "), "a href"), 1, "one link: {key:?}");
        let text = answer("text", key.text());
        assert_eq!(
            texts,
            [text, OPEN_SENTENCE, link.as_str(), FOUNDER_FOOTER],
            "{key:?}: the text, the sentence, the link (as the anchor's text) and the footer"
        );
    }
}

/// NT-1's oracle is shown to fail before it is trusted (AGENTS.md, "Independent oracles"): a
/// canary seeded into a real mail's wire form, in its subject and in its body, is found, and only
/// that one.
#[test]
#[ignore = "pending E8-11"]
fn the_canary_scan_catches_a_canary_seeded_into_a_mail() {
    let to = answer("address", FounderAddress::parse(FOUNDER));
    let (_, mail) = mail(42, TextKey::AttentionNeeded);
    let clean = String::from_utf8(answer("wire", mail.wire(&to))).expect("text");
    for (place, leaked) in [
        (
            "subject",
            clean.replacen("Subject: ", "Subject: ZQInstrument ", 1),
        ),
        ("body", clean.replacen(OPEN_SENTENCE, "zqprice 501.2", 1)),
    ] {
        assert_ne!(leaked, clean, "the seed lands in the {place}");
        let found = answer("scan", scan(leaked.as_bytes()));
        let expected = if place == "subject" {
            "zqinstrument"
        } else {
            "zqprice"
        };
        assert_eq!(
            found,
            [expected],
            "the {place}'s seeded canary, and only it"
        );
    }
}
