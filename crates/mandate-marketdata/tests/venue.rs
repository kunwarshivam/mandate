//! Each feed's venue hours (E2-4 brief interpretation 6): SIP and IEX open and close by the
//! exchange calendar and their own checked-in hours, IEX after an early close is unclassified,
//! crypto never closes, and anything outside the recorded dates is unclassified.

use mandate_marketdata::model::Feed;
use mandate_marketdata::venue::{Venue, VenueError, VenueHours, VenueState};
use mandate_time::{Date, ExchangeCalendar, NewYorkTime, TimeError, UtcNanos};
use proptest::prelude::*;

use VenueState::{Closed, Open, Unclassified};

fn time(s: &str) -> UtcNanos {
    UtcNanos::parse(&format!("{s}:00.000000000Z")).unwrap()
}

fn day(s: &str) -> Date {
    Date::parse(s).unwrap()
}

fn ny(s: &str) -> NewYorkTime {
    NewYorkTime::parse(s).unwrap()
}

fn state(venue: &Venue, at: &str) -> VenueState {
    venue.state_at(time(at)).unwrap()
}

fn sip() -> Venue {
    Venue::of(Feed::Sip).unwrap()
}

fn iex() -> Venue {
    Venue::of(Feed::Iex).unwrap()
}

#[test]
fn sip_is_open_from_04_00_to_20_00_et_and_closed_overnight() {
    let sip = sip();
    let edt = [
        ("2026-09-24T07:59", Closed),
        ("2026-09-24T08:00", Open),
        ("2026-09-24T13:30", Open),
        ("2026-09-24T23:59", Open),
        ("2026-09-25T00:00", Closed),
        ("2026-09-25T04:00", Closed),
        ("2026-09-25T07:59", Closed),
        ("2026-09-25T08:00", Open),
    ];
    for (at, expected) in edt {
        assert_eq!(state(&sip, at), expected, "{at}");
    }
    let est = [
        ("2026-03-06T08:59", Closed),
        ("2026-03-06T09:00", Open),
        ("2026-03-07T00:59", Open),
        ("2026-03-07T01:00", Closed),
        ("2026-03-09T07:59", Closed),
        ("2026-03-09T08:00", Open),
    ];
    for (at, expected) in est {
        assert_eq!(state(&sip, at), expected, "{at}");
    }
}

#[test]
fn iex_is_open_from_08_00_to_17_00_et() {
    let iex = iex();
    for (at, expected) in [
        ("2026-09-24T08:00", Closed),
        ("2026-09-24T11:59", Closed),
        ("2026-09-24T12:00", Open),
        ("2026-09-24T20:59", Open),
        ("2026-09-24T21:00", Closed),
        ("2026-09-24T23:59", Closed),
        ("2026-09-25T02:00", Closed),
    ] {
        assert_eq!(state(&iex, at), expected, "{at}");
    }
}

#[test]
fn weekends_and_closures_are_closed_for_both_feeds() {
    for venue in [sip(), iex()] {
        for at in [
            "2026-09-26T15:00",
            "2026-09-27T15:00",
            "2026-11-26T15:00",
            "2026-11-26T10:00",
            "2026-11-26T20:00",
            "2026-09-07T14:00",
        ] {
            assert_eq!(state(&venue, at), Closed, "{at}");
        }
    }
}

#[test]
fn on_an_early_close_sip_closes_at_17_00_and_iex_is_unclassified_from_13_00_to_20_00() {
    let sip = sip();
    for (at, expected) in [
        ("2026-11-27T09:00", Open),
        ("2026-11-27T17:59", Open),
        ("2026-11-27T18:00", Open),
        ("2026-11-27T21:59", Open),
        ("2026-11-27T22:00", Closed),
        ("2026-11-28T00:59", Closed),
    ] {
        assert_eq!(state(&sip, at), expected, "sip {at}");
    }
    let iex = iex();
    for (at, expected) in [
        ("2026-11-27T12:59", Closed),
        ("2026-11-27T13:00", Open),
        ("2026-11-27T17:59", Open),
        ("2026-11-27T18:00", Unclassified),
        ("2026-11-27T22:00", Unclassified),
        ("2026-11-28T00:59", Unclassified),
        ("2026-11-28T01:00", Closed),
    ] {
        assert_eq!(state(&iex, at), expected, "iex {at}");
    }
}

#[test]
fn crypto_is_always_open() {
    let crypto = Venue::of(Feed::CryptoUs).unwrap();
    for at in [
        "2026-12-25T15:00",
        "2026-09-26T03:00",
        "2017-06-01T00:00",
        "2031-01-01T12:00",
    ] {
        assert_eq!(state(&crypto, at), Open, "{at}");
        assert_eq!(crypto.day_state(time(at).date()).unwrap(), Open, "{at}");
    }
    assert_eq!(state(&Venue::continuous(), "2026-11-26T15:00"), Open);
}

#[test]
fn minutes_outside_the_recorded_dates_are_unclassified() {
    let (sip, iex) = (sip(), iex());
    assert_eq!(state(&sip, "2026-12-04T15:00"), Open);
    assert_eq!(state(&sip, "2026-12-06T02:00"), Closed);
    assert_eq!(state(&sip, "2026-12-06T05:00"), Unclassified);
    assert_eq!(state(&sip, "2026-12-07T15:00"), Unclassified);
    assert_eq!(state(&iex, "2026-12-07T15:00"), Open);
    for venue in [&sip, &iex] {
        assert_eq!(state(venue, "2017-12-29T15:00"), Unclassified);
        assert_eq!(state(venue, "2018-01-01T04:59"), Unclassified);
        assert_eq!(state(venue, "2018-01-02T15:00"), Open);
    }
    assert_eq!(state(&iex, "2028-12-29T15:00"), Open);
    assert_eq!(state(&iex, "2029-01-01T05:00"), Unclassified);
    assert_eq!(state(&iex, "2029-01-02T15:00"), Unclassified);
}

#[test]
fn a_day_is_open_when_it_trades_closed_when_it_does_not_and_unclassified_outside_the_record() {
    let (sip, iex) = (sip(), iex());
    for (date, expected) in [
        ("2026-09-24", Open),
        ("2026-11-27", Open),
        ("2026-11-26", Closed),
        ("2026-09-26", Closed),
        ("2017-12-29", Unclassified),
        ("2026-12-07", Unclassified),
    ] {
        assert_eq!(sip.day_state(day(date)).unwrap(), expected, "sip {date}");
    }
    for (date, expected) in [
        ("2026-12-07", Open),
        ("2026-12-25", Closed),
        ("2028-12-29", Open),
        ("2029-01-02", Unclassified),
    ] {
        assert_eq!(iex.day_state(day(date)).unwrap(), expected, "iex {date}");
    }
}

fn venue(calendar: &str, hours: &str) -> Venue {
    Venue::exchange(
        ExchangeCalendar::parse(calendar).unwrap(),
        VenueHours::parse(hours).unwrap(),
    )
}

const SHORT_CALENDAR: &str = "valid 2026-09-21 2026-09-25\nhours 07:00 09:30 16:00 18:00\nearly_close 2026-09-25 12:00 14:00 Test\n";

#[test]
fn a_venue_is_open_only_while_both_the_calendar_and_its_own_hours_are() {
    let wide = venue(
        SHORT_CALENDAR,
        "valid 2026-01-01 2026-12-31\nhours 04:00 09:30 16:00 20:00\n",
    );
    for (at, expected) in [
        ("2026-09-24T10:59", Closed),
        ("2026-09-24T11:00", Open),
        ("2026-09-24T21:59", Open),
        ("2026-09-24T22:00", Closed),
        ("2026-09-25T17:59", Open),
        ("2026-09-25T18:00", Closed),
    ] {
        assert_eq!(state(&wide, at), expected, "wide {at}");
    }
    let narrow = venue(
        SHORT_CALENDAR,
        "valid 2026-01-01 2026-12-31\nhours 08:00 09:30 16:00 17:00\nunpublished_after_early_close 20:00\n",
    );
    for (at, expected) in [
        ("2026-09-24T11:59", Closed),
        ("2026-09-24T12:00", Open),
        ("2026-09-24T20:59", Open),
        ("2026-09-24T21:00", Closed),
        ("2026-09-25T15:59", Open),
        ("2026-09-25T16:00", Unclassified),
        ("2026-09-25T23:59", Unclassified),
        ("2026-09-26T00:00", Closed),
    ] {
        assert_eq!(state(&narrow, at), expected, "narrow {at}");
    }
    assert_eq!(state(&narrow, "2026-09-28T15:00"), Unclassified);
    assert_eq!(state(&narrow, "2026-09-20T15:00"), Unclassified);
}

#[test]
fn states_answer_instants_in_any_order() {
    let sip = sip();
    let mut states = sip.states();
    for (at, expected) in [
        ("2026-09-24T13:00", Open),
        ("2026-09-26T13:00", Closed),
        ("2026-09-24T03:00", Closed),
        ("2026-09-23T23:00", Open),
        ("2026-09-24T13:01", Open),
    ] {
        assert_eq!(states.at(time(at)).unwrap(), expected, "{at}");
    }
}

#[test]
fn the_checked_in_hours_are_the_published_ones() {
    let sip = VenueHours::sip().unwrap();
    assert_eq!(
        (sip.valid_from(), sip.valid_to()),
        (day("2018-01-01"), day("2026-12-05"))
    );
    assert_eq!(
        [
            sip.open(),
            sip.regular_open(),
            sip.regular_close(),
            sip.close()
        ],
        [ny("04:00"), ny("09:30"), ny("16:00"), ny("20:00")]
    );
    assert_eq!(sip.unpublished_after_early_close(), None);
    let iex = VenueHours::iex().unwrap();
    assert_eq!(
        (iex.valid_from(), iex.valid_to()),
        (day("2018-01-01"), day("2028-12-31"))
    );
    assert_eq!(
        [
            iex.open(),
            iex.regular_open(),
            iex.regular_close(),
            iex.close()
        ],
        [ny("08:00"), ny("09:30"), ny("16:00"), ny("17:00")]
    );
    assert_eq!(iex.unpublished_after_early_close(), Some(ny("20:00")));
}

#[test]
fn venue_hours_skip_comments_and_blank_lines() {
    let hours = VenueHours::parse(
        "# a comment\n\nvalid 2026-01-01 2026-01-31\n# another\nhours 04:00 09:30 16:00 20:00\n\n",
    )
    .unwrap();
    assert_eq!(hours.valid_to(), day("2026-01-31"));
    assert_eq!(hours.close(), ny("20:00"));
}

fn parse_err(text: &str) -> VenueError {
    VenueHours::parse(text).unwrap_err()
}

const VALID: &str = "valid 2026-01-01 2026-12-31\n";
const HOURS: &str = "hours 08:00 09:30 16:00 17:00\n";

#[test]
fn malformed_venue_hours_are_errors_on_their_line() {
    for (text, line) in [
        ("hours 08:00 09:30 16:00 17:00\n".to_owned(), 1),
        (format!("{VALID}{VALID}"), 2),
        (format!("{VALID}{HOURS}{HOURS}"), 3),
        (format!("{VALID}unpublished_after_early_close 20:00\n"), 2),
        (
            format!(
                "{VALID}{HOURS}unpublished_after_early_close 20:00\nunpublished_after_early_close 20:00\n"
            ),
            4,
        ),
        (format!("{VALID}hours 08:00 09:30 16:00\n"), 2),
        (format!("{VALID}hours 08:00 09:30 16:00 17:00 18:00\n"), 2),
        ("valid 2026-01-01\n".to_owned(), 1),
        (format!("{VALID}{HOURS}closed 2026-01-02 Test\n"), 3),
        (format!("{VALID}{HOURS}unpublished_after_early_close\n"), 3),
        (" valid 2026-01-01 2026-12-31\n".to_owned(), 1),
    ] {
        let err = parse_err(&text);
        assert!(
            matches!(err, VenueError::Syntax { line: l } if l == line),
            "{text:?}: {err:?}"
        );
    }
}

#[test]
fn bad_dates_and_times_are_value_errors_with_their_cause() {
    for (text, line, source) in [
        (
            "valid 2026-02-30 2026-12-31\n".to_owned(),
            1,
            TimeError::InvalidDate,
        ),
        (
            "valid 2026-01-01 26-12-31\n".to_owned(),
            1,
            TimeError::Syntax,
        ),
        (
            format!("{VALID}hours 08:00 09:30 16:00 24:00\n"),
            2,
            TimeError::InvalidDate,
        ),
        (
            format!("{VALID}hours 8:00 09:30 16:00 17:00\n"),
            2,
            TimeError::Syntax,
        ),
        (
            format!("{VALID}{HOURS}unpublished_after_early_close 20:60\n"),
            3,
            TimeError::InvalidDate,
        ),
    ] {
        let err = parse_err(&text);
        assert!(
            matches!(err, VenueError::Value { line: l, source: s } if l == line && s == source),
            "{text:?}: {err:?}"
        );
    }
}

#[test]
fn ranges_and_hours_must_increase() {
    for (text, line) in [
        ("valid 2026-12-31 2026-01-01\n".to_owned(), 1),
        (format!("{VALID}hours 09:30 09:30 16:00 17:00\n"), 2),
        (format!("{VALID}hours 08:00 16:00 09:30 17:00\n"), 2),
        (format!("{VALID}hours 08:00 09:30 17:00 17:00\n"), 2),
        (format!("{VALID}hours 08:00 09:30 16:00 15:00\n"), 2),
        (
            format!("{VALID}{HOURS}unpublished_after_early_close 16:00\n"),
            3,
        ),
        (
            format!("{VALID}{HOURS}unpublished_after_early_close 12:00\n"),
            3,
        ),
    ] {
        let err = parse_err(&text);
        assert!(
            matches!(err, VenueError::Order { line: l } if l == line),
            "{text:?}: {err:?}"
        );
    }
    let one_day = VenueHours::parse("valid 2026-01-01 2026-01-01\nhours 08:00 09:30 16:00 17:00\nunpublished_after_early_close 16:01\n").unwrap();
    assert_eq!(one_day.valid_from(), one_day.valid_to());
}

#[test]
fn venue_hours_need_both_the_range_and_the_hours() {
    for text in ["", "# only a comment\n", VALID] {
        assert!(
            matches!(parse_err(text), VenueError::Incomplete),
            "{text:?}"
        );
    }
}

#[test]
fn venue_errors_have_stable_codes() {
    assert_eq!(parse_err("x\n").code(), "syntax");
    assert_eq!(parse_err("valid x 2026-01-01\n").code(), "value");
    assert_eq!(parse_err("valid 2026-01-02 2026-01-01\n").code(), "order");
    assert_eq!(parse_err("").code(), "incomplete");
    let calendar = VenueError::Calendar(ExchangeCalendar::parse("").unwrap_err());
    assert_eq!(calendar.code(), "calendar");
}

const DST_STARTS: &str = "2026-03-08T07:00";
const DST_ENDS: &str = "2026-11-01T06:00";

const CLOSED_2026: [&str; 10] = [
    "2026-01-01",
    "2026-01-19",
    "2026-02-16",
    "2026-04-03",
    "2026-05-25",
    "2026-06-19",
    "2026-07-03",
    "2026-09-07",
    "2026-11-26",
    "2026-12-25",
];

const EARLY_CLOSE_2026: [&str; 2] = ["2026-11-27", "2026-12-24"];

/// The New York date and minute of the day of `at` in 2026, by the published US daylight-saving
/// rule rather than the time-zone database.
fn new_york_2026(at: UtcNanos) -> (Date, i64) {
    let daylight = time(DST_STARTS) <= at && at < time(DST_ENDS);
    let offset = if daylight { 4 * 3_600 } else { 5 * 3_600 };
    let local = at.secs() - offset;
    let date = UtcNanos::from_parts(local, 0).unwrap().date();
    (date, local.rem_euclid(86_400) / 60)
}

fn expected(feed: Feed, at: UtcNanos) -> VenueState {
    let (date, minute) = new_york_2026(at);
    let listed = |dates: &[&str]| dates.iter().any(|d| day(d) == date);
    if feed == Feed::Sip && date > day("2026-12-05") {
        return Unclassified;
    }
    if date.is_weekend() || listed(&CLOSED_2026) {
        return Closed;
    }
    let early = listed(&EARLY_CLOSE_2026);
    let within = |from: i64, to: i64| from * 60 <= minute && minute < to * 60;
    match feed {
        Feed::Sip if within(4, if early { 17 } else { 20 }) => Open,
        Feed::Iex if within(8, if early { 13 } else { 17 }) => Open,
        Feed::Iex if early && within(13, 20) => Unclassified,
        _ => Closed,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn sip_and_iex_agree_with_the_published_2026_schedule_at_every_minute(
        minute in 0_i64..(364 * 1_440),
        iex_feed in any::<bool>(),
    ) {
        let feed = if iex_feed { Feed::Iex } else { Feed::Sip };
        let at = UtcNanos::from_parts(time("2026-01-01T12:00").secs() + minute * 60, 0).unwrap();
        prop_assert_eq!(Venue::of(feed).unwrap().state_at(at).unwrap(), expected(feed, at), "{}", at);
    }
}
