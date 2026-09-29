//! Invoice arithmetic. Every amount is integer USD cents; quantities are
//! integer hundredths (150 = 1.50 hours). Nothing here touches floating point,
//! so a document's printed quantity x rate always equals its printed amount.

use chrono::{Datelike, Duration, NaiveDate};

const MS_PER_HOUR: i64 = 3_600_000;

/// The largest rate (USD 10,000,000.00) and quantity (10,000,000.00) a line
/// accepts; keeps every product and sum far inside `i64`.
pub const MAX_RATE_CENTS: i64 = 1_000_000_000;
pub const MAX_QUANTITY_HUNDREDTHS: i64 = 1_000_000_000;
/// The largest single line amount (USD 999,999,999.99), so paper columns hold.
pub const MAX_AMOUNT_CENTS: i64 = 99_999_999_999;

/// A tracked duration as billable hundredths of an hour, rounded to nearest
/// (36 s granularity) with a floor of one hundredth so no entry bills zero.
pub fn hours_hundredths(duration_ms: i64) -> i64 {
    let rounded = (duration_ms * 100 + MS_PER_HOUR / 2) / MS_PER_HOUR;
    rounded.max(1)
}

/// `quantity x rate`, rounded half-up to whole cents at the line level.
pub fn line_amount_cents(quantity_hundredths: i64, rate_cents: i64) -> Result<i64, String> {
    if !(1..=MAX_QUANTITY_HUNDREDTHS).contains(&quantity_hundredths) {
        return Err("Quantity must be greater than zero".into());
    }
    if !(0..=MAX_RATE_CENTS).contains(&rate_cents) {
        return Err("Rate must be between $0.00 and $10,000,000.00".into());
    }
    let amount = (quantity_hundredths * rate_cents + 50) / 100;
    if amount > MAX_AMOUNT_CENTS {
        return Err("A line amount can be at most $999,999,999.99".into());
    }
    Ok(amount)
}

/// `$1,234.56`.
pub fn format_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    let digits = (cents / 100).to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{sign}${grouped}.{:02}", cents % 100)
}

/// `1.50` for hours-like quantities, `3` for whole ones.
pub fn format_quantity(hundredths: i64) -> String {
    let whole = hundredths / 100;
    let fraction = hundredths % 100;
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:02}")
    }
}

pub fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "Dates must look like 2026-09-29".to_string())
}

/// `September 29, 2026`.
pub fn format_date(date: NaiveDate) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!(
        "{} {}, {}",
        MONTHS[date.month0() as usize],
        date.day(),
        date.year()
    )
}

/// The due date for a payment term, as `YYYY-MM-DD`.
pub fn due_date(issue_date: &str, terms_days: i64) -> Result<String, String> {
    let issue = parse_date(issue_date)?;
    let due = issue
        .checked_add_signed(Duration::days(terms_days))
        .ok_or("Due date is out of range")?;
    Ok(due.format("%Y-%m-%d").to_string())
}

/// `Net 30`, or `Due on receipt` for zero days.
pub fn terms_label(terms_days: i64) -> String {
    if terms_days == 0 {
        "Due on receipt".into()
    } else {
        format!("Net {terms_days}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_has_exact_two_decimals_and_grouping() {
        assert_eq!(format_money(0), "$0.00");
        assert_eq!(format_money(1), "$0.01");
        assert_eq!(format_money(123_456), "$1,234.56");
        assert_eq!(format_money(100_000_000), "$1,000,000.00");
        assert_eq!(format_money(-500), "-$5.00");
    }

    #[test]
    fn line_amounts_round_half_up_at_the_line() {
        assert_eq!(line_amount_cents(150, 10_000).unwrap(), 15_000);
        // 0.33 h x $199.95 = $65.9835 -> $65.98
        assert_eq!(line_amount_cents(33, 19_995).unwrap(), 6_598);
        // 0.01 h x $0.50 = 0.005 -> half-up to one cent
        assert_eq!(line_amount_cents(1, 50).unwrap(), 1);
        assert_eq!(line_amount_cents(100, 0).unwrap(), 0);
        assert!(line_amount_cents(0, 100).is_err());
        assert!(line_amount_cents(100, -1).is_err());
        assert!(line_amount_cents(MAX_QUANTITY_HUNDREDTHS + 1, 100).is_err());
        assert!(line_amount_cents(100, MAX_RATE_CENTS + 1).is_err());
        assert!(line_amount_cents(MAX_QUANTITY_HUNDREDTHS, MAX_RATE_CENTS).is_err());
    }

    #[test]
    fn tracked_time_bills_in_hundredths_of_an_hour() {
        assert_eq!(hours_hundredths(3_600_000), 100);
        assert_eq!(hours_hundredths(5_400_000), 150);
        assert_eq!(hours_hundredths(1_000_000), 28); // 16m40s = 0.2777h
        assert_eq!(hours_hundredths(1), 1);
    }

    #[test]
    fn quantities_and_dates_format_for_paper() {
        assert_eq!(format_quantity(150), "1.50");
        assert_eq!(format_quantity(100), "1");
        assert_eq!(format_quantity(5), "0.05");
        assert_eq!(
            format_date(parse_date("2026-09-29").unwrap()),
            "September 29, 2026"
        );
        assert_eq!(due_date("2026-09-29", 30).unwrap(), "2026-10-29");
        assert_eq!(due_date("2026-12-15", 30).unwrap(), "2027-01-14");
        assert!(parse_date("29/09/2026").is_err());
        assert_eq!(terms_label(30), "Net 30");
        assert_eq!(terms_label(0), "Due on receipt");
    }
}
