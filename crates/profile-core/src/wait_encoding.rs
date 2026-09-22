fn immediate(text: &str) -> Option<u16> {
    let text = text.split(';').next()?.split("//").next()?.trim();
    let (digits, radix) = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .map(|digits| (digits, 16))
        .unwrap_or((text, 10));
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u16::from_str_radix(digits, radix).ok()
}

/// Expands supported packed wait immediates into named counter operands.
pub fn expand(architecture: &str, text: &str) -> Option<String> {
    let (mnemonic, operand) = text.trim().split_once(char::is_whitespace)?;
    let bits = immediate(operand)?;
    if mnemonic == "s_waitcnt" {
        let (vm, exp, lgkm) = if architecture.starts_with("gfx11") {
            ((bits >> 10) & 63, bits & 7, (bits >> 4) & 63)
        } else if architecture.starts_with("gfx10") {
            (
                (bits & 15) | ((bits >> 14) << 4),
                (bits >> 4) & 7,
                (bits >> 8) & 63,
            )
        } else if architecture.starts_with("gfx9") {
            (
                (bits & 15) | ((bits >> 14) << 4),
                (bits >> 4) & 7,
                (bits >> 8) & 15,
            )
        } else {
            return None;
        };
        Some(format!(
            "s_waitcnt vmcnt({vm}) expcnt({exp}) lgkmcnt({lgkm})"
        ))
    } else if architecture.starts_with("gfx12") {
        let counter = match mnemonic {
            "s_wait_loadcnt_dscnt" => "loadcnt",
            "s_wait_storecnt_dscnt" => "storecnt",
            _ => return None,
        };
        Some(format!(
            "s_wait {counter}({}) dscnt({})",
            (bits >> 8) & 63,
            bits & 63
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::expand;
    #[test]
    fn separates_generation_layouts() {
        assert_eq!(
            expand("gfx942", "s_waitcnt 0x1234").unwrap(),
            "s_waitcnt vmcnt(4) expcnt(3) lgkmcnt(2)"
        );
        assert_eq!(
            expand("gfx1030", "s_waitcnt 0x1234").unwrap(),
            "s_waitcnt vmcnt(4) expcnt(3) lgkmcnt(18)"
        );
        assert_eq!(
            expand("gfx1100", "s_waitcnt 0x1234").unwrap(),
            "s_waitcnt vmcnt(4) expcnt(4) lgkmcnt(35)"
        );
        assert_eq!(
            expand("gfx1200", "s_wait_loadcnt_dscnt 0x1234").unwrap(),
            "s_wait loadcnt(18) dscnt(52)"
        );
        assert_eq!(
            expand("gfx1200", "s_wait_storecnt_dscnt 4660 ; decoded").unwrap(),
            "s_wait storecnt(18) dscnt(52)"
        );
    }
    #[test]
    fn rejects_wrong_architecture_overflow_and_partial_numeric_text() {
        for text in [
            "s_waitcnt -1",
            "s_waitcnt 65536",
            "s_waitcnt 0x10000",
            "s_waitcnt 0x",
            "s_waitcnt 0junk",
            "s_waitcnt 1 2",
        ] {
            assert!(expand("gfx1100", text).is_none(), "{text}");
        }
        assert!(expand("gfx1200", "s_waitcnt 0").is_none());
        assert!(expand("gfx1100", "s_wait_loadcnt_dscnt 0").is_none());
        assert!(expand("unknown", "s_waitcnt 0").is_none());
        assert!(expand("gfx1100", "s_waitcnt vmcnt(0)").is_none());
    }
}
