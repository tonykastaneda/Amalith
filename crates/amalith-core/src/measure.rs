//! Free-text parsing and formatting for measurement fields, shared by
//! every numeric field in the shell (Transform X/Y/W/H, stroke weight,
//! offset distance, dash/gap, document width/height, …).
//!
//! Two things every such field needs, matching Illustrator's own numeric
//! fields: it always shows its own unit's initials (`12 px`, `45°`,
//! `50%`), and typed text is a small arithmetic expression — `10+5`,
//! `3*2` — where any individual number may carry its own unit suffix
//! (`5in`, `3px`, `10mm`) that gets converted into the field's unit
//! before being combined with the rest. So typing `5in` into a field
//! currently showing `px` commits `480` (5 × 96), and `1in + 3px`
//! commits `99`.
use crate::units::Unit;

/// What a field's plain (unsuffixed) numbers already mean, and what
/// suffix it displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A physical/document length in `Unit` — bare numbers are read (and
    /// re-displayed) in this unit; another unit's suffix converts into
    /// it.
    Length(Unit),
    /// A percentage — bare numbers only; a trailing `%` is accepted and
    /// ignored (purely cosmetic — the value is already a percent), any
    /// other suffix is a parse failure.
    Percent,
    /// An angle in degrees — bare numbers only; a trailing `°` or `deg`
    /// is accepted and ignored, any other suffix fails.
    Angle,
    /// A dimensionless count or ratio (sides, copies, miter limit) — no
    /// suffix of any kind is accepted, though arithmetic still is.
    Count,
}

/// Parses `input` as an arithmetic expression (`+ - * /`, parentheses),
/// resolving any per-literal unit suffix against `kind`, and returns the
/// result expressed in `kind`'s own unit — ready to drop straight into
/// the field it came from. `None` on anything that doesn't fully parse:
/// an empty field, stray trailing characters, an unrecognized or
/// inapplicable suffix, unbalanced parentheses, or division by zero.
pub fn parse_measurement(input: &str, kind: Kind) -> Option<f64> {
    let chars: Vec<char> = input.trim().chars().collect();
    let mut p = Parser { chars: &chars, pos: 0, kind };
    let v = p.expr()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return None;
    }
    Some(v)
}

/// Formats `value` (already expressed in `kind`'s own unit) back into a
/// plain, trimmed string carrying `kind`'s suffix — the display
/// counterpart to [`parse_measurement`]. Trailing zeros are trimmed, so
/// `12.0` reads as `12`, not `12.0000`.
pub fn format_measurement(value: f64, kind: Kind) -> String {
    let n = format_number(value);
    match kind {
        Kind::Length(unit) => format!("{n} {}", unit.abbr()),
        Kind::Percent => format!("{n}%"),
        Kind::Angle => format!("{n}\u{b0}"),
        Kind::Count => n,
    }
}

fn format_number(value: f64) -> String {
    let r = (value * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 5e-5 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    kind: Kind,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while matches!(self.chars.get(self.pos), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    v += self.term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    v *= self.factor()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let d = self.factor()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn factor(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                Some(-self.factor()?)
            }
            Some('+') => {
                self.pos += 1;
                self.factor()
            }
            Some('(') => {
                self.pos += 1;
                let v = self.expr()?;
                self.skip_ws();
                if self.peek() != Some(')') {
                    return None;
                }
                self.pos += 1;
                Some(v)
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if self.pos == start {
            return None;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let value: f64 = text.parse().ok()?;

        // A unit suffix, if present (optionally after some whitespace,
        // e.g. `5 in`), applies to *this* literal only, so an expression
        // can mix units — `1in + 3px` converts and sums both terms
        // rather than requiring the whole field to agree.
        let before_suffix = self.pos;
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
        let suffix_start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_alphabetic()) {
            self.pos += 1;
        }
        if self.pos == suffix_start && matches!(self.peek(), Some('"') | Some('\'') | Some('\u{b0}') | Some('%')) {
            self.pos += 1;
        }
        if self.pos == suffix_start {
            // No suffix after all — don't consume the whitespace; leave
            // it for the normal operator-skipping logic to see.
            self.pos = before_suffix;
            return Some(value);
        }
        let suffix: String = self.chars[suffix_start..self.pos].iter().collect();
        self.resolve(value, &suffix)
    }

    /// Converts one literal's `value`, typed with `suffix` (empty if
    /// none), into the parser's own `kind`.
    fn resolve(&self, value: f64, suffix: &str) -> Option<f64> {
        if suffix.is_empty() {
            return Some(value);
        }
        match self.kind {
            Kind::Length(target) => {
                let from = Unit::parse_abbr(suffix)?;
                Some(target.from_px(from.to_px(value)))
            }
            Kind::Percent => (suffix == "%").then_some(value),
            Kind::Angle => (suffix == "\u{b0}" || suffix.eq_ignore_ascii_case("deg")).then_some(value),
            Kind::Count => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_number_is_read_in_the_fields_own_unit() {
        assert_eq!(parse_measurement("12", Kind::Length(Unit::Px)), Some(12.0));
        assert_eq!(parse_measurement("12", Kind::Length(Unit::Pt)), Some(12.0));
    }

    #[test]
    fn a_typed_unit_converts_into_the_fields_own_unit() {
        // 5in typed into a px field becomes 480px.
        assert_eq!(parse_measurement("5in", Kind::Length(Unit::Px)), Some(480.0));
        // 72pt typed into an inch field becomes 1in.
        assert_eq!(parse_measurement("72pt", Kind::Length(Unit::In)), Some(1.0));
    }

    #[test]
    fn mixed_unit_arithmetic_converts_each_term_before_combining() {
        // 1in + 3px in a px field: 96 + 3 = 99.
        let v = parse_measurement("1in + 3px", Kind::Length(Unit::Px)).unwrap();
        assert!((v - 99.0).abs() < 1e-9, "{v}");
    }

    #[test]
    fn plain_arithmetic_works_with_no_unit_involved() {
        assert_eq!(parse_measurement("10+5", Kind::Length(Unit::Px)), Some(15.0));
        assert_eq!(parse_measurement("3*2", Kind::Count), Some(6.0));
        assert_eq!(parse_measurement("(2+3)*4", Kind::Length(Unit::Px)), Some(20.0));
        assert_eq!(parse_measurement("10/4", Kind::Length(Unit::Px)), Some(2.5));
    }

    #[test]
    fn negative_and_unary_signs_work() {
        assert_eq!(parse_measurement("-5", Kind::Length(Unit::Px)), Some(-5.0));
        assert_eq!(parse_measurement("-5in", Kind::Length(Unit::Px)), Some(-480.0));
    }

    #[test]
    fn percent_and_angle_suffixes_are_accepted_and_stripped() {
        assert_eq!(parse_measurement("50%", Kind::Percent), Some(50.0));
        assert_eq!(parse_measurement("45\u{b0}", Kind::Angle), Some(45.0));
        assert_eq!(parse_measurement("45deg", Kind::Angle), Some(45.0));
    }

    #[test]
    fn a_length_suffix_on_a_count_field_fails_to_parse() {
        assert_eq!(parse_measurement("5in", Kind::Count), None);
    }

    #[test]
    fn division_by_zero_and_garbage_fail_to_parse() {
        assert_eq!(parse_measurement("5/0", Kind::Length(Unit::Px)), None);
        assert_eq!(parse_measurement("abc", Kind::Length(Unit::Px)), None);
        assert_eq!(parse_measurement("5 apples", Kind::Length(Unit::Px)), None);
        assert_eq!(parse_measurement("", Kind::Length(Unit::Px)), None);
    }

    #[test]
    fn format_matches_each_kinds_display_convention() {
        assert_eq!(format_measurement(12.0, Kind::Length(Unit::Px)), "12 px");
        assert_eq!(format_measurement(50.0, Kind::Percent), "50%");
        assert_eq!(format_measurement(45.0, Kind::Angle), "45\u{b0}");
        assert_eq!(format_measurement(3.0, Kind::Count), "3");
        assert_eq!(format_measurement(133.0203, Kind::Length(Unit::Px)), "133.0203 px");
    }

    #[test]
    fn round_trips_through_parse_and_format() {
        for kind in [Kind::Length(Unit::Px), Kind::Percent, Kind::Angle, Kind::Count] {
            let s = format_measurement(42.5, kind);
            // Strip the suffix back off by parsing it — should recover
            // the original value in the field's own unit.
            assert_eq!(parse_measurement(&s, kind), Some(42.5), "{s}");
        }
    }
}
