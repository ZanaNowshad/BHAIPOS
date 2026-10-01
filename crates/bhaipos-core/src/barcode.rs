use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarcodeSymbology {
    Ean13,
    Ean8,
    UpcA,
    UpcE,
    Code128,
    Plu,
    Internal,
    Weighted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightedBarcode {
    pub raw: String,
    pub item_code: String,
    /// Embedded payload in minor units or grams according to configured scale mode.
    pub embedded_value: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BarcodeError {
    #[error("invalid EAN-13")]
    InvalidEan13,
    #[error("not a configured weighted barcode")]
    NotWeighted,
}

pub fn ean13_checksum_valid(code: &str) -> bool {
    if code.len() != 13 || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let d: Vec<u32> = code.bytes().map(|b| (b - b'0') as u32).collect();
    let sum: u32 = d[..12]
        .iter()
        .enumerate()
        .map(|(i, v)| if i % 2 == 0 { *v } else { *v * 3 })
        .sum();
    ((10 - (sum % 10)) % 10) == d[12]
}

/// Common supermarket layout: PP IIIII VVVVV C where PP=20..29, IIIII=item, VVVVV=weight/price payload.
pub fn parse_weighted_ean13(code: &str) -> Result<WeightedBarcode, BarcodeError> {
    if !ean13_checksum_valid(code) {
        return Err(BarcodeError::InvalidEan13);
    }
    let prefix: u8 = code[0..2].parse().map_err(|_| BarcodeError::InvalidEan13)?;
    if !(20..=29).contains(&prefix) {
        return Err(BarcodeError::NotWeighted);
    }
    Ok(WeightedBarcode {
        raw: code.to_owned(),
        item_code: code[2..7].to_owned(),
        embedded_value: code[7..12].parse().unwrap(),
    })
}

pub fn classify_barcode(code: &str) -> BarcodeSymbology {
    if code.chars().all(|c| c.is_ascii_digit()) {
        match code.len() {
            13 if ean13_checksum_valid(code) => {
                if matches!(
                    code.get(0..2).and_then(|p| p.parse::<u8>().ok()),
                    Some(20..=29)
                ) {
                    BarcodeSymbology::Weighted
                } else {
                    BarcodeSymbology::Ean13
                }
            }
            8 => BarcodeSymbology::Ean8,
            12 => BarcodeSymbology::UpcA,
            4..=6 => BarcodeSymbology::Plu,
            _ => BarcodeSymbology::Unknown,
        }
    } else {
        BarcodeSymbology::Code128
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_known_ean() {
        assert!(ean13_checksum_valid("4006381333931"));
        assert_eq!(classify_barcode("4006381333931"), BarcodeSymbology::Ean13);
    }
}
