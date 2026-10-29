#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Volt,
    Hertz,
    Ohm,
    Henry,
    Farad,
    Siemens,
    Ampere,
    Watt,
    Var,
    /// Dimensionless ratio/value; accepts no unit suffix.
    Unitless,
}

impl Unit {
    fn suffixes(self) -> &'static [&'static str] {
        match self {
            Self::Volt => &["V", "v"],
            Self::Hertz => &["Hz", "hz", "HZ"],
            Self::Ohm => &["Ohm", "ohm", "OHM", "Ω", "Ω"],
            Self::Henry => &["H", "h"],
            Self::Farad => &["F", "f"],
            Self::Siemens => &["S", "siemens"],
            Self::Ampere => &["A", "a"],
            Self::Watt => &["W", "w"],
            Self::Var => &["var", "VAR"],
            Self::Unitless => &[],
        }
    }
}

/// Parse an optional SI prefix and unit; a bare number is in `bare_scale` units.
pub fn parse(text: &str, unit: Unit, bare_scale: f64) -> Result<f64, String> {
    let s = text.trim();
    if s.is_empty() {
        return Err("enter a value".into());
    }
    // First try the complete bare number (including scientific notation).
    if let Ok(value) = s.parse::<f64>() {
        if value.is_finite() {
            return Ok(value * bare_scale);
        }
        return Err("value must be finite".into());
    }
    for suffix in unit.suffixes() {
        if let Some(body) = s.strip_suffix(suffix) {
            let body = body.trim_end();
            for (prefix, multiplier) in [
                ("", 1.0),
                ("p", 1e-12),
                ("n", 1e-9),
                ("u", 1e-6),
                ("µ", 1e-6),
                ("μ", 1e-6),
                ("m", 1e-3),
                ("k", 1e3),
                ("K", 1e3),
                ("M", 1e6),
                ("G", 1e9),
            ] {
                let number = if prefix.is_empty() {
                    body
                } else if let Some(n) = body.strip_suffix(prefix) {
                    n
                } else {
                    continue;
                };
                if let Ok(value) = number.trim().parse::<f64>() {
                    let result = value * multiplier;
                    if result.is_finite() {
                        return Ok(result);
                    }
                }
            }
        }
    }
    Err(format!("invalid value or unit: {s} (expected {:?})", unit))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_lab_units() {
        assert!((parse("5uH", Unit::Henry, 1.0).unwrap() - 5e-6).abs() < 1e-18);
        assert!((parse("10pF", Unit::Farad, 1.0).unwrap() - 1e-11).abs() < 1e-22);
        assert!((parse("1mOhm", Unit::Ohm, 1.0).unwrap() - 1e-3).abs() < 1e-15);
        assert_eq!(parse(" 0.1 µF", Unit::Farad, 1.0).unwrap(), 1e-7);
        assert_eq!(parse("2.4kΩ", Unit::Ohm, 1.0).unwrap(), 2400.0);
        assert_eq!(parse("1mS", Unit::Siemens, 1.0).unwrap(), 0.001);
        assert!((parse("5", Unit::Farad, 1e-6).unwrap() - 5e-6).abs() < 1e-18);
        assert!(parse("5uF", Unit::Henry, 1.0).is_err());
        assert!(parse("NaN", Unit::Volt, 1.0).is_err());
    }
}
