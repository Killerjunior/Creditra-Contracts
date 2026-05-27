// SPDX-License-Identifier: MIT

//! Pure integer arithmetic helpers used across the credit contract.
//!
//! All functions in this module operate on fixed-point integers and never
//! allocate. Rounding behaviour is documented per function; the default is
//! **truncation toward zero** (Rust's native integer division) unless stated
//! otherwise.

#![warn(missing_docs)]

/// Multiply `value` by `numerator` then divide by `denominator`, using an
/// intermediate `i128` accumulator to avoid overflow on typical inputs.
///
/// # Rounding
/// Truncates toward zero (floor for positive results). No rounding-up variant
/// is provided; callers that need ceiling arithmetic should add
/// `denominator - 1` to `value * numerator` before calling.
///
/// # Parameters
/// - `value`:       The base amount to scale.
/// - `numerator`:   Scaling numerator (e.g. an interest rate).
/// - `denominator`: Scaling denominator (e.g. 10_000 for basis-point math).
///
/// # Returns
/// `(value * numerator) / denominator`, truncated toward zero.
///
/// # Panics
/// - If `denominator` is zero (division by zero).
/// - If the intermediate product `value * numerator` overflows `i128`
///   (unlikely in practice; `i128` supports values up to ~1.7 × 10³⁸).
///
/// # Example
/// ```
/// // 1_000 * 300 / 10_000 = 30  (3% of 1_000)
/// assert_eq!(mul_div(1_000, 300, 10_000), 30);
/// ```
pub fn mul_div(value: i128, numerator: i128, denominator: i128) -> i128 {
    assert!(denominator != 0, "mul_div: denominator must not be zero");
    value
        .checked_mul(numerator)
        .expect("mul_div: intermediate product overflowed i128")
        / denominator
}

/// Apply a basis-point rate to an amount.
///
/// Basis points (bps) express rates as integer hundredths of a percent:
/// 1 bps = 0.01%, 100 bps = 1%, 10_000 bps = 100%.
///
/// This is a thin wrapper around [`mul_div`] with `denominator = 10_000`.
///
/// # Rounding
/// Truncates toward zero. For example, `apply_bps(1, 1)` returns `0`
/// because `1 * 1 / 10_000 = 0` after truncation.
///
/// # Parameters
/// - `amount`: The principal amount to apply the rate to.
/// - `rate_bps`: The rate in basis points (0 ..= 10_000 for 0%–100%;
///   values above 10_000 are accepted but represent rates over 100%).
///
/// # Returns
/// `amount * rate_bps / 10_000`, truncated toward zero.
///
/// # Panics
/// Panics only if the intermediate product `amount * rate_bps` overflows
/// `i128`, which requires both operands to be astronomically large.
///
/// # Examples
/// ```
/// // 3% of 1_000 = 30
/// assert_eq!(apply_bps(1_000, 300), 30);
///
/// // 0.5% of 200 = 1  (1.0 truncated to 1)
/// assert_eq!(apply_bps(200, 50), 1);
///
/// // 0.01% of 50 = 0  (0.005 truncated to 0)
/// assert_eq!(apply_bps(50, 1), 0);
///
/// // 100% of 500 = 500
/// assert_eq!(apply_bps(500, 10_000), 500);
/// ```
pub fn apply_bps(amount: i128, rate_bps: u32) -> i128 {
    mul_div(amount, rate_bps as i128, 10_000)
}

/// Pro-rate an annual interest charge to a sub-year elapsed period.
///
/// Converts an annual basis-point rate into the interest due for `elapsed`
/// seconds, assuming a 365-day (31_536_000-second) year.
///
/// Formula:
/// ```text
/// interest = principal * rate_bps * elapsed
///            ────────────────────────────────
///                  10_000 * 31_536_000
/// ```
///
/// Both multiplications are performed in `i128` to preserve precision before
/// the final division; the combined denominator is `315_360_000_000`.
///
/// # Rounding
/// Truncates toward zero. Partial-second or sub-unit amounts are lost.
/// For a principal of 1_000_000 at 500 bps (5%) over 1 hour (3_600 s):
/// ```text
/// 1_000_000 * 500 * 3_600 / 315_360_000_000
///   = 1_800_000_000_000 / 315_360_000_000
///   ≈ 5  (5 units of interest, truncated)
/// ```
///
/// # Parameters
/// - `principal`:   Outstanding balance to accrue interest on.
/// - `rate_bps`:    Annual interest rate in basis points (e.g. 500 = 5%).
/// - `elapsed_secs`: Seconds elapsed since last accrual. Passing `0` always
///   returns `0`.
///
/// # Returns
/// The pro-rated interest amount for the elapsed period, truncated toward zero.
///
/// # Panics
/// - If any intermediate multiplication overflows `i128`. In practice this
///   requires `principal * rate_bps` to exceed ~1.7 × 10³⁸, which is far
///   beyond realistic credit limits.
///
/// # Examples
/// ```
/// // 5% annual on 1_000_000 for 1 day (86_400 s)
/// // = 1_000_000 * 500 * 86_400 / 315_360_000_000
/// // = 43_200_000_000_000 / 315_360_000_000 = 137 (truncated)
/// assert_eq!(prorate_interest(1_000_000, 500, 86_400), 137);
///
/// // Zero elapsed → always 0
/// assert_eq!(prorate_interest(1_000_000, 500, 0), 0);
///
/// // Zero principal → always 0
/// assert_eq!(prorate_interest(0, 500, 86_400), 0);
///
/// // 10% annual on 100_000 for 1 year (31_536_000 s) = 10_000 exactly
/// assert_eq!(prorate_interest(100_000, 1_000, 31_536_000), 10_000);
/// ```
pub fn prorate_interest(principal: i128, rate_bps: u32, elapsed_secs: u64) -> i128 {
    const SECONDS_PER_YEAR: i128 = 31_536_000;
    const BPS_DENOMINATOR: i128 = 10_000;

    if elapsed_secs == 0 || principal == 0 {
        return 0;
    }

    let numerator = principal
        .checked_mul(rate_bps as i128)
        .expect("prorate_interest: principal * rate_bps overflowed i128")
        .checked_mul(elapsed_secs as i128)
        .expect("prorate_interest: product with elapsed_secs overflowed i128");

    let denominator = BPS_DENOMINATOR
        .checked_mul(SECONDS_PER_YEAR)
        .expect("prorate_interest: denominator overflowed i128");

    numerator / denominator
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── mul_div ──────────────────────────────────────────────────────────────

    #[test]
    fn mul_div_basic() {
        assert_eq!(mul_div(1_000, 300, 10_000), 30);
    }

    #[test]
    fn mul_div_truncates_toward_zero() {
        // 7 * 1 / 3 = 2.33… → 2
        assert_eq!(mul_div(7, 1, 3), 2);
    }

    #[test]
    fn mul_div_identity_denominator() {
        assert_eq!(mul_div(42, 1, 1), 42);
    }

    #[test]
    #[should_panic(expected = "denominator must not be zero")]
    fn mul_div_zero_denominator_panics() {
        mul_div(1, 1, 0);
    }

    // ── apply_bps ────────────────────────────────────────────────────────────

    #[test]
    fn apply_bps_three_percent() {
        assert_eq!(apply_bps(1_000, 300), 30);
    }

    #[test]
    fn apply_bps_half_percent_truncates() {
        assert_eq!(apply_bps(200, 50), 1);
    }

    #[test]
    fn apply_bps_sub_unit_truncates_to_zero() {
        assert_eq!(apply_bps(50, 1), 0);
    }

    #[test]
    fn apply_bps_full_rate() {
        assert_eq!(apply_bps(500, 10_000), 500);
    }

    #[test]
    fn apply_bps_zero_rate() {
        assert_eq!(apply_bps(1_000_000, 0), 0);
    }

    // ── prorate_interest ─────────────────────────────────────────────────────

    #[test]
    fn prorate_interest_one_day() {
        // 5% annual on 1_000_000 for 1 day
        assert_eq!(prorate_interest(1_000_000, 500, 86_400), 137);
    }

    #[test]
    fn prorate_interest_zero_elapsed() {
        assert_eq!(prorate_interest(1_000_000, 500, 0), 0);
    }

    #[test]
    fn prorate_interest_zero_principal() {
        assert_eq!(prorate_interest(0, 500, 86_400), 0);
    }

    #[test]
    fn prorate_interest_full_year() {
        // 10% on 100_000 for exactly 1 year = 10_000
        assert_eq!(prorate_interest(100_000, 1_000, 31_536_000), 10_000);
    }

    #[test]
    fn prorate_interest_one_hour() {
        // 5% on 1_000_000 for 3_600 s ≈ 5
        assert_eq!(prorate_interest(1_000_000, 500, 3_600), 5);
    }
}
