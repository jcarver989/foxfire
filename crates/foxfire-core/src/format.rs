use agent_client_protocol::v2::schema::Cost;

pub fn token_count(count: u64) -> String {
    let (unit, suffix) = match count {
        0..1_000 => return count.to_string(),
        1_000..1_000_000 => (1_000, "k"),
        _ => (1_000_000, "M"),
    };
    let tenths = count * 10 / unit;
    match tenths % 10 {
        0 => format!("{}{suffix}", tenths / 10),
        tenth => format!("{}.{tenth}{suffix}", tenths / 10),
    }
}

pub fn cost(cost: &Cost) -> String {
    let (symbol, unit) = match cost.currency.as_str() {
        "USD" => ("$", String::new()),
        "EUR" => ("€", String::new()),
        "GBP" => ("£", String::new()),
        unit => ("", format!(" {unit}")),
    };
    let (less, amount) = if cost.amount > 0.0 && cost.amount < SMALLEST { ("<", SMALLEST) } else { ("", cost.amount) };
    let decimals = if symbol.is_empty() && amount.fract() == 0.0 { 0 } else { 2 };
    format!("{less}{symbol}{amount:.decimals$}{unit}")
}

const SMALLEST: f64 = 0.01;
