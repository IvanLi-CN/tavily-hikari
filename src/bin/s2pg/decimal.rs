//! Exact fixed-point decimals used to compare `numeric` columns and their aggregates.
//!
//! The migration tool never introduces floating point into numeric semantics: values are
//! parsed from decimal literals into an `i128` mantissa plus a decimal scale, so sums and
//! extremes stay exact and reproduce identically on the SQLite and PostgreSQL sides.

use std::cmp::Ordering;

/// Maximum number of significant digits kept in the mantissa. `i128` holds 38 decimal
/// digits; the conservative cap leaves headroom for sums of many values.
const MAX_DIGITS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    units: i128,
    scale: u32,
}

impl Decimal {
    pub fn zero() -> Self {
        Self {
            negative: false,
            units: 0,
            scale: 0,
        }
    }

    pub fn from_i64(value: i64) -> Self {
        Self {
            negative: value < 0,
            units: i128::from(value).abs(),
            scale: 0,
        }
    }

    /// Parses a decimal literal: optional sign, digits, optional fraction, optional exponent.
    pub fn parse(input: &str) -> Result<Self, String> {
        let text = input.trim();
        if text.is_empty() {
            return Err("empty decimal literal".to_string());
        }
        let (negative, unsigned) = match text.as_bytes()[0] {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
            Some(index) => {
                let exponent = unsigned[index + 1..]
                    .parse::<i32>()
                    .map_err(|_| format!("invalid decimal exponent in {input:?}"))?;
                (&unsigned[..index], exponent)
            }
            None => (unsigned, 0),
        };
        let (int_part, frac_part) = match mantissa.split_once('.') {
            Some((int_part, frac_part)) => (int_part, frac_part),
            None => (mantissa, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(format!("invalid decimal literal {input:?}"));
        }
        if !int_part.bytes().all(|byte| byte.is_ascii_digit())
            || !frac_part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!("invalid decimal literal {input:?}"));
        }
        let digits = format!("{int_part}{frac_part}");
        let mut units = digits
            .parse::<i128>()
            .map_err(|_| format!("decimal literal {input:?} exceeds the supported range"))?;
        let mut scale = frac_part.len() as i64 - i64::from(exponent);
        if scale < 0 {
            let zeros = u32::try_from(-scale).unwrap_or(u32::MAX);
            units = checked_pow10(units, zeros)
                .ok_or_else(|| format!("decimal literal {input:?} exceeds the supported range"))?;
            scale = 0;
        }
        let mut value = Self {
            negative,
            units,
            scale: u32::try_from(scale)
                .map_err(|_| format!("decimal literal {input:?} exceeds the supported range"))?,
        };
        value.normalize();
        Ok(value)
    }

    pub fn from_f64(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err(format!("non-finite value {value} is not a decimal"));
        }
        // `{:?}` is the shortest representation that round-trips the binary value, which keeps
        // SQLite REAL -> numeric conversion reproducible on both sides.
        Self::parse(&format!("{value:?}"))
    }

    pub fn checked_add(self, other: Self) -> Result<Self, String> {
        let scale = self.scale.max(other.scale);
        let left = self.scaled_units(scale)?;
        let right = other.scaled_units(scale)?;
        let sum = left
            .checked_add(right)
            .ok_or_else(|| "decimal sum overflowed the supported range".to_string())?;
        let mut value = Self {
            negative: sum < 0,
            units: sum.abs(),
            scale,
        };
        value.normalize();
        Ok(value)
    }

    pub fn compare(self, other: Self) -> Result<Ordering, String> {
        let scale = self.scale.max(other.scale);
        let left = self.scaled_units(scale)?;
        let right = other.scaled_units(scale)?;
        Ok(left.cmp(&right))
    }

    pub fn render(&self) -> String {
        let digits = self.units.to_string();
        let sign = if self.negative { "-" } else { "" };
        if self.scale == 0 {
            return format!("{sign}{digits}");
        }
        let scale = self.scale as usize;
        let padded = if digits.len() <= scale {
            format!("{}{}", "0".repeat(scale - digits.len() + 1), digits)
        } else {
            digits
        };
        let split = padded.len() - scale;
        format!("{sign}{}.{}", &padded[..split], &padded[split..])
    }

    fn scaled_units(self, target_scale: u32) -> Result<i128, String> {
        let extra = target_scale - self.scale;
        let magnitude = checked_pow10(self.units, extra)
            .ok_or_else(|| "decimal value exceeds the supported range".to_string())?;
        Ok(if self.negative { -magnitude } else { magnitude })
    }

    fn normalize(&mut self) {
        if self.units == 0 {
            self.negative = false;
            self.scale = 0;
            return;
        }
        while self.scale > 0 && self.units % 10 == 0 {
            self.units /= 10;
            self.scale -= 1;
        }
    }
}

fn checked_pow10(value: i128, exponent: u32) -> Option<i128> {
    if exponent > MAX_DIGITS {
        return None;
    }
    let mut result = value;
    for _ in 0..exponent {
        result = result.checked_mul(10)?;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::Decimal;
    use std::cmp::Ordering;

    #[test]
    fn parses_and_renders_canonical_decimals() {
        assert_eq!(Decimal::parse("0010.500").unwrap().render(), "10.5");
        assert_eq!(Decimal::parse("-0.000").unwrap().render(), "0");
        assert_eq!(Decimal::parse("1e3").unwrap().render(), "1000");
        assert_eq!(Decimal::parse("1.25e-2").unwrap().render(), "0.0125");
        assert_eq!(Decimal::parse("+7").unwrap().render(), "7");
        assert_eq!(Decimal::parse("0.0000001").unwrap().render(), "0.0000001");
    }

    #[test]
    fn rejects_invalid_literals() {
        for literal in ["", "abc", "1.2.3", "1e", "1,5", "  "] {
            assert!(
                Decimal::parse(literal).is_err(),
                "{literal:?} should be rejected"
            );
        }
    }

    #[test]
    fn adds_and_compares_across_scales() {
        let sum = Decimal::parse("1.5")
            .unwrap()
            .checked_add(Decimal::parse("2.25").unwrap())
            .unwrap();
        assert_eq!(sum.render(), "3.75");
        assert_eq!(
            Decimal::parse("1.50")
                .unwrap()
                .compare(Decimal::parse("1.5").unwrap())
                .unwrap(),
            Ordering::Equal
        );
        assert_eq!(
            Decimal::parse("-2")
                .unwrap()
                .compare(Decimal::parse("1").unwrap())
                .unwrap(),
            Ordering::Less
        );
    }

    #[test]
    fn converts_floats_through_their_shortest_representation() {
        assert_eq!(Decimal::from_f64(0.1).unwrap().render(), "0.1");
        assert_eq!(Decimal::from_f64(-2.5).unwrap().render(), "-2.5");
        assert!(Decimal::from_f64(f64::INFINITY).is_err());
    }
}
