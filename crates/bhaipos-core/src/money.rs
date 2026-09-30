use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MoneyError {
    #[error("money overflow")]
    Overflow,
    #[error("quantity must be non-negative")]
    NegativeQuantity,
    #[error("invalid decimal amount")]
    InvalidDecimal,
    #[error("tax rate must be non-negative and zero for non-taxable categories")]
    InvalidTaxRate,
}

/// Authoritative BHD money value in fils (1 BHD = 1000 fils).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Self = Self(0);

    pub fn from_bhd_str(input: &str) -> Result<Self, MoneyError> {
        let s = input.trim();
        if s.is_empty() { return Err(MoneyError::InvalidDecimal); }
        let negative = s.starts_with('-');
        let unsigned = if negative { &s[1..] } else { s };
        let mut parts = unsigned.split('.');
        let whole = parts.next().ok_or(MoneyError::InvalidDecimal)?;
        let frac = parts.next().unwrap_or("");
        if parts.next().is_some() || whole.is_empty() || frac.len() > 3 || !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(MoneyError::InvalidDecimal);
        }
        let whole_i: i128 = whole.parse().map_err(|_| MoneyError::InvalidDecimal)?;
        let mut frac_s = frac.to_owned();
        while frac_s.len() < 3 { frac_s.push('0'); }
        let frac_i: i128 = if frac_s.is_empty() { 0 } else { frac_s.parse().map_err(|_| MoneyError::InvalidDecimal)? };
        let mut fils = whole_i.checked_mul(1000).and_then(|v| v.checked_add(frac_i)).ok_or(MoneyError::Overflow)?;
        if negative { fils = -fils; }
        i64::try_from(fils).map(Money).map_err(|_| MoneyError::Overflow)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0.checked_add(other.0).map(Self).ok_or(MoneyError::Overflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.0.checked_sub(other.0).map(Self).ok_or(MoneyError::Overflow)
    }

    pub fn checked_abs(self) -> Result<Self, MoneyError> {
        self.0.checked_abs().map(Self).ok_or(MoneyError::Overflow)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = (self.0 as i128).abs();
        write!(f, "{}{abs_whole}.{frac:03}", sign, abs_whole = abs / 1000, frac = abs % 1000)
    }
}

/// Quantity in thousandths of a sell unit. 1000 = 1.000 unit, 250 = 0.250 kg, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuantityMilli(pub i64);

impl QuantityMilli {
    pub const ONE: Self = Self(1000);
    pub fn new(v: i64) -> Result<Self, MoneyError> { if v < 0 { Err(MoneyError::NegativeQuantity) } else { Ok(Self(v)) } }
}

/// Computes unit price × decimal quantity using integer arithmetic and half-away-from-zero rounding.
pub fn price_times_quantity(unit_price: Money, quantity: QuantityMilli) -> Result<Money, MoneyError> {
    if quantity.0 < 0 { return Err(MoneyError::NegativeQuantity); }
    let numerator = (unit_price.0 as i128).checked_mul(quantity.0 as i128).ok_or(MoneyError::Overflow)?;
    let rounded = div_round_half_away(numerator, 1000);
    i64::try_from(rounded).map(Money).map_err(|_| MoneyError::Overflow)
}

pub fn apply_basis_points(amount: Money, bps: i32) -> Result<Money, MoneyError> {
    let numerator = (amount.0 as i128).checked_mul(bps as i128).ok_or(MoneyError::Overflow)?;
    let rounded = div_round_half_away(numerator, 10_000);
    i64::try_from(rounded).map(Money).map_err(|_| MoneyError::Overflow)
}

pub fn div_round_half_away(numerator: i128, denominator: i128) -> i128 {
    assert!(denominator > 0);
    let q = numerator / denominator;
    let r = numerator % denominator;
    if r == 0 { return q; }
    let twice = r.abs() * 2;
    if twice >= denominator { q + numerator.signum() } else { q }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parse_and_format_bhd() {
        for (s,v) in [("1",1000),("1.2",1200),("1.025",1025),("0.001",1),("-2.500",-2500)] {
            let m=Money::from_bhd_str(s).unwrap(); assert_eq!(m.0,v); assert_eq!(Money::from_bhd_str(&m.to_string()).unwrap(),m);
        }
    }
    #[test] fn weighted_quantity_is_integer_safe() {
        assert_eq!(price_times_quantity(Money(1250), QuantityMilli(250)).unwrap(), Money(313));
    }
    #[test] fn checked_operations_reject_i64_overflow() {
        assert_eq!(Money(i64::MAX).checked_add(Money(1)),Err(MoneyError::Overflow));
        assert_eq!(Money(i64::MIN).checked_sub(Money(1)),Err(MoneyError::Overflow));
        assert_eq!(Money(i64::MIN).checked_abs(),Err(MoneyError::Overflow));
    }
}
