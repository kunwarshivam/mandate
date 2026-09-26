import json
from decimal import Decimal

import pytest

from research_spike.config import MAX_THESES
from research_spike.http import decode
from research_spike.propose import ThesisError, build_user_prompt, parse_response, parse_thesis

NEWS_IDS = {"48123456", "48123457"}


def thesis(**overrides) -> dict:
    base = {
        "instrument": "AAPL",
        "direction": "long",
        "conviction": 0.6,
        "confidence": 0.5,
        "horizon_days": 5,
        "thesis": "Services revenue grew 14% and the close rose three days in a row.",
        "evidence": ["48123456"],
        "invalidation": "A close below the 20-day low.",
    }
    return base | overrides


def response(*theses: dict) -> str:
    return json.dumps({"theses": list(theses)})


def test_valid_thesis_is_accepted_with_decimal_numbers(basket):
    accepted, rejected = parse_response(response(thesis(conviction=0.1234567)), basket, NEWS_IDS, set())
    assert rejected == []
    (parsed,) = accepted
    assert parsed.conviction == Decimal("0.1234567") and isinstance(parsed.conviction, Decimal)
    assert parsed.confidence == Decimal("0.5") and parsed.evidence == ("48123456",)


def test_no_thesis_is_a_valid_answer(basket):
    assert parse_response('{"theses": []}', basket, NEWS_IDS, set()) == ([], [])


@pytest.mark.parametrize(
    ("override", "reason"),
    [
        ({"instrument": "TSLA"}, "not in the basket"),
        ({"instrument": "aapl"}, "not in the basket"),
        ({"direction": "short"}, "direction must be 'long'"),
        ({"conviction": 1.5}, "outside"),
        ({"conviction": -0.4}, "not held"),
        ({"conviction": "high"}, "must be a number"),
        ({"conviction": True}, "must be a number"),
        ({"confidence": -0.1}, "outside"),
        ({"horizon_days": 0}, "horizon_days"),
        ({"horizon_days": 2.5}, "horizon_days"),
        ({"horizon_days": 31}, "horizon_days"),
        ({"thesis": "Investors should buy on the dip."}, "forbidden language"),
        ({"thesis": "Price target 300."}, "forbidden language"),
        ({"thesis": "The stock will rise next week."}, "forbidden language"),
        ({"thesis": ""}, "non-empty"),
        ({"evidence": ["1"]}, "unknown news ids"),
        ({"evidence": "48123456"}, "list of news ids"),
        ({"invalidation": None}, "non-empty"),
    ],
)
def test_invalid_theses_are_rejected(basket, override, reason):
    accepted, rejected = parse_response(response(thesis(**override)), basket, NEWS_IDS, set())
    assert accepted == [] and reason in rejected[0]["reason"]


def test_extra_or_missing_fields_are_rejected(basket):
    with pytest.raises(ThesisError, match="fields must be exactly"):
        parse_thesis(thesis(target_price=300), basket, NEWS_IDS, set())
    missing = thesis()
    del missing["invalidation"]
    with pytest.raises(ThesisError, match="fields must be exactly"):
        parse_thesis(missing, basket, NEWS_IDS, set())


def test_negative_conviction_is_allowed_only_for_held_instruments(basket):
    parsed = parse_thesis(
        decode(json.dumps(thesis(conviction=-0.5, evidence=[]))), basket, NEWS_IDS, {"AAPL"}
    )
    assert parsed.conviction == Decimal("-0.5")
    with pytest.raises(ThesisError, match="must be a number"):
        parse_thesis(thesis(conviction=-0.5), basket, NEWS_IDS, {"AAPL"})


def test_rejections_are_reported_with_reasons_and_do_not_block_others(basket):
    accepted, rejected = parse_response(
        response(thesis(instrument="TSLA"), thesis(instrument="SPY", evidence=[])), basket, NEWS_IDS, set()
    )
    assert [t.instrument for t in accepted] == ["SPY"]
    assert rejected[0]["reason"] == "TSLA is not in the basket" and rejected[0]["raw"]["instrument"] == "TSLA"


def test_second_thesis_for_the_same_instrument_is_rejected(basket):
    accepted, rejected = parse_response(response(thesis(), thesis(confidence=0.9)), basket, NEWS_IDS, set())
    assert len(accepted) == 1 and "second thesis for AAPL" in rejected[0]["reason"]


def test_more_than_the_maximum_is_rejected(basket):
    many = [thesis(instrument=s, evidence=[]) for s in ("AAPL", "SPY", "BTC/USD")] + [
        thesis(instrument="AAPL")
    ] * 2
    big = Basket = None  # noqa: F841
    accepted, rejected = parse_response(response(*many), basket, NEWS_IDS, set())
    assert len(accepted) == 3 and all("second thesis" in r["reason"] for r in rejected)
    assert MAX_THESES == 4


@pytest.mark.parametrize("text", ["not json", "[]", '{"theses": {}}', '{"ideas": []}'])
def test_malformed_envelopes_raise(basket, text):
    with pytest.raises(ThesisError):
        parse_response(text, basket, NEWS_IDS, set())


def test_prompt_lists_bars_news_and_holdings(basket):
    from datetime import UTC, datetime

    bars = {
        "AAPL": [
            {"t": "2026-09-23T04:00:00Z", "o": Decimal("231"), "h": 1, "l": 1, "c": Decimal("231.5"), "v": 5}
        ]
    }
    news = [
        {
            "id": 48123456,
            "headline": "Apple reports",
            "summary": "S",
            "source": "benzinga",
            "created_at": "t",
            "url": "u",
            "symbols": ["AAPL"],
        }
    ]
    text = build_user_prompt(basket, bars, news, {"AAPL": "4"}, datetime(2026, 9, 26, tzinfo=UTC))
    assert "2026-09-23 o=231 h=1 l=1 c=231.5 v=5" in text
    assert "[id 48123456]" in text and "Held: AAPL (qty 4)." in text
