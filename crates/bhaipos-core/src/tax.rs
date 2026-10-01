use crate::money::{apply_basis_points, div_round_half_away, Money, MoneyError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaxCategory {
    StandardRated,
    ZeroRated,
    Exempt,
    OutOfScope,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxRule {
    pub category: TaxCategory,
    pub rate_bps: i32,
    pub inclusive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxBreakdown {
    pub net: Money,
    pub tax: Money,
    pub gross: Money,
}

impl TaxRule {
    pub fn validate(&self) -> Result<(), MoneyError> {
        let non_taxable = matches!(
            self.category,
            TaxCategory::ZeroRated | TaxCategory::Exempt | TaxCategory::OutOfScope
        );
        if self.rate_bps < 0 || (non_taxable && self.rate_bps != 0) {
            return Err(MoneyError::InvalidTaxRate);
        }
        Ok(())
    }

    pub fn calculate(&self, basis: Money) -> Result<TaxBreakdown, MoneyError> {
        self.validate()?;
        if self.rate_bps == 0
            || matches!(
                self.category,
                TaxCategory::ZeroRated | TaxCategory::Exempt | TaxCategory::OutOfScope
            )
        {
            return Ok(TaxBreakdown {
                net: basis,
                tax: Money::ZERO,
                gross: basis,
            });
        }
        if self.inclusive {
            let numerator = (basis.0 as i128) * (self.rate_bps as i128);
            let denom = 10_000_i128 + self.rate_bps as i128;
            let tax_i = div_round_half_away(numerator, denom);
            let tax = Money(i64::try_from(tax_i).map_err(|_| MoneyError::Overflow)?);
            Ok(TaxBreakdown {
                net: basis.checked_sub(tax)?,
                tax,
                gross: basis,
            })
        } else {
            let tax = apply_basis_points(basis, self.rate_bps)?;
            Ok(TaxBreakdown {
                net: basis,
                tax,
                gross: basis.checked_add(tax)?,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bahrain_inclusive_ten_percent_is_exact() {
        let r = TaxRule {
            category: TaxCategory::StandardRated,
            rate_bps: 1000,
            inclusive: true,
        };
        let b = r.calculate(Money(1100)).unwrap();
        assert_eq!(
            b,
            TaxBreakdown {
                net: Money(1000),
                tax: Money(100),
                gross: Money(1100)
            }
        );
    }
    #[test]
    fn invalid_tax_rates_fail_without_panicking() {
        let negative = TaxRule {
            category: TaxCategory::StandardRated,
            rate_bps: -10_000,
            inclusive: true,
        };
        assert_eq!(
            negative.calculate(Money(1000)),
            Err(MoneyError::InvalidTaxRate)
        );
        let exempt_with_rate = TaxRule {
            category: TaxCategory::Exempt,
            rate_bps: 1000,
            inclusive: false,
        };
        assert_eq!(
            exempt_with_rate.calculate(Money(1000)),
            Err(MoneyError::InvalidTaxRate)
        );
    }
}
