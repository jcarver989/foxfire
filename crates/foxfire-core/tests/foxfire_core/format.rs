use agent_client_protocol::v2::schema::Cost;
use foxfire_core::format::{cost, token_count};

#[test]
fn token_counts_read_in_thousands_and_millions_to_a_tenth() {
    let cases = [
        (0, "0"),
        (850, "850"),
        (1_000, "1k"),
        (12_000, "12k"),
        (12_345, "12.3k"),
        (999_999, "999.9k"),
        (1_000_000, "1M"),
        (1_250_000, "1.2M"),
    ];

    for (count, expected) in cases {
        assert_eq!(token_count(count), expected, "{count}");
    }
}

#[test]
fn costs_read_in_their_currencys_notation_or_followed_by_their_unit() {
    let cases = [
        (0.0, "USD", "$0.00"),
        (0.004, "USD", "<$0.01"),
        (2.469, "USD", "$2.47"),
        (1_234.5, "USD", "$1234.50"),
        (2.5, "EUR", "€2.50"),
        (2.5, "GBP", "£2.50"),
        (2.5, "CHF", "2.50 CHF"),
        (370.0, "JPY", "370 JPY"),
        (247.0, "credits", "247 credits"),
        (0.004, "credits", "<0.01 credits"),
    ];

    for (amount, currency, expected) in cases {
        assert_eq!(cost(&Cost::new(amount, currency)), expected, "{amount} {currency}");
    }
}
