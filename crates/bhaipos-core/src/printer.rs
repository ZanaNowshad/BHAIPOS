use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptEncoding { Ascii, Utf8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutMode { None, Partial, Full }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerPulsePolicy { Never, CashSale }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscPosProfile {
    pub paper_width_mm:u8,
    pub characters_per_line:u8,
    pub encoding:ReceiptEncoding,
    pub cut_mode:CutMode,
    pub drawer_pulse_policy:DrawerPulsePolicy,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReceiptEncodingError {
    #[error("unsupported receipt paper width")]
    PaperWidth,
    #[error("characters-per-line is invalid for the selected paper width")]
    LineWidth,
    #[error("receipt contains non-ASCII text; configure a verified UTF-8/Arabic-capable printer profile")]
    UnsupportedText,
}

pub fn render_esc_pos(snapshot:&str,profile:EscPosProfile,cash_sale:bool)->Result<Vec<u8>,ReceiptEncodingError>{
    let max_columns=match profile.paper_width_mm{58=>42,80=>64,_=>return Err(ReceiptEncodingError::PaperWidth)};
    if profile.characters_per_line<24||profile.characters_per_line>max_columns{return Err(ReceiptEncodingError::LineWidth);}
    if matches!(profile.encoding,ReceiptEncoding::Ascii)&&!snapshot.is_ascii(){return Err(ReceiptEncodingError::UnsupportedText);}
    let mut bytes=vec![0x1b,0x40];
    if cash_sale&&matches!(profile.drawer_pulse_policy,DrawerPulsePolicy::CashSale){bytes.extend_from_slice(&[0x1b,0x70,0x00,0x32,0xfa]);}
    bytes.extend_from_slice(snapshot.as_bytes());
    if !snapshot.ends_with('\n'){bytes.push(b'\n');}
    bytes.extend_from_slice(&[b'\n',b'\n',b'\n']);
    match profile.cut_mode{CutMode::None=>{},CutMode::Partial=>bytes.extend_from_slice(&[0x1d,0x56,0x01]),CutMode::Full=>bytes.extend_from_slice(&[0x1d,0x56,0x00])}
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile()->EscPosProfile{EscPosProfile{paper_width_mm:80,characters_per_line:48,encoding:ReceiptEncoding::Ascii,cut_mode:CutMode::Partial,drawer_pulse_policy:DrawerPulsePolicy::CashSale}}

    #[test]
    fn exact_snapshot_bytes_are_preserved_between_device_commands(){
        let snapshot="BHAIPOS\nReceipt: MAIN-20260929-000001\nMilk 1L  1.000 x 1.100 = 1.100\n";
        let rendered=render_esc_pos(snapshot,profile(),false).unwrap();
        assert!(rendered.windows(snapshot.len()).any(|window|window==snapshot.as_bytes()));
        assert!(!rendered.windows(5).any(|window|window==[0x1b,0x70,0,0x32,0xfa]));
        assert!(rendered.ends_with(&[0x1d,0x56,0x01]));
    }

    #[test]
    fn cash_drawer_pulse_is_policy_bound_and_arabic_requires_explicit_encoding(){
        let rendered=render_esc_pos("Cash receipt\n",profile(),true).unwrap();
        assert!(rendered.windows(5).any(|window|window==[0x1b,0x70,0,0x32,0xfa]));
        assert_eq!(render_esc_pos("إيصال\n",profile(),false),Err(ReceiptEncodingError::UnsupportedText));
        let utf8=EscPosProfile{encoding:ReceiptEncoding::Utf8,..profile()};
        assert!(render_esc_pos("إيصال\n",utf8,false).unwrap().windows("إيصال".len()).any(|window|window=="إيصال".as_bytes()));
    }
}
