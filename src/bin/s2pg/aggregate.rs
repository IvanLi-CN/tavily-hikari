//! Per-column aggregate fingerprints used by `verify`.
//!
//! Integer and numeric values are accumulated with exact semantics (`i128` / fixed-point);
//! only `double precision` columns use a relative tolerance, and the report marks those
//! columns explicitly. Text values contribute an order-independent additive sha256; bytea
//! columns are compared by byte length only, as required by the migration spec.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::hash::AdditiveDigest;
use crate::types::{ColumnKind, SqlValue};

pub const FLOAT_RELATIVE_TOLERANCE: f64 = 1e-12;

#[derive(Debug, Clone)]
pub enum AggregateState {
    Integer {
        count: u64,
        nulls: u64,
        sum: i128,
        min: Option<i64>,
        max: Option<i64>,
    },
    Double {
        count: u64,
        nulls: u64,
        sum: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
    Text {
        count: u64,
        nulls: u64,
        min_length: Option<u64>,
        max_length: Option<u64>,
        digest: AdditiveDigest,
    },
    Bytes {
        count: u64,
        nulls: u64,
        min_length: Option<u64>,
        max_length: Option<u64>,
        total_length: u128,
    },
    Numeric {
        count: u64,
        nulls: u64,
        sum: Decimal,
        min: Option<Decimal>,
        max: Option<Decimal>,
    },
}

impl AggregateState {
    pub fn new(kind: ColumnKind) -> Self {
        match kind {
            ColumnKind::Integer => Self::Integer {
                count: 0,
                nulls: 0,
                sum: 0,
                min: None,
                max: None,
            },
            ColumnKind::Double => Self::Double {
                count: 0,
                nulls: 0,
                sum: 0.0,
                min: None,
                max: None,
            },
            ColumnKind::Text => Self::Text {
                count: 0,
                nulls: 0,
                min_length: None,
                max_length: None,
                digest: AdditiveDigest::new(),
            },
            ColumnKind::Bytes => Self::Bytes {
                count: 0,
                nulls: 0,
                min_length: None,
                max_length: None,
                total_length: 0,
            },
            ColumnKind::Numeric => Self::Numeric {
                count: 0,
                nulls: 0,
                sum: Decimal::zero(),
                min: None,
                max: None,
            },
        }
    }

    pub fn kind(&self) -> ColumnKind {
        match self {
            Self::Integer { .. } => ColumnKind::Integer,
            Self::Double { .. } => ColumnKind::Double,
            Self::Text { .. } => ColumnKind::Text,
            Self::Bytes { .. } => ColumnKind::Bytes,
            Self::Numeric { .. } => ColumnKind::Numeric,
        }
    }

    /// Folds one value into the aggregate. A storage class that contradicts the column kind
    /// is reported instead of being coerced.
    pub fn observe(&mut self, column: &str, value: &SqlValue) -> Result<(), String> {
        if value.is_null() {
            match self {
                Self::Integer { nulls, .. }
                | Self::Double { nulls, .. }
                | Self::Text { nulls, .. }
                | Self::Bytes { nulls, .. }
                | Self::Numeric { nulls, .. } => *nulls += 1,
            }
            return Ok(());
        }
        match (self, value) {
            (
                Self::Integer {
                    count,
                    sum,
                    min,
                    max,
                    ..
                },
                SqlValue::Integer(raw),
            ) => {
                *count += 1;
                *sum += i128::from(*raw);
                *min = Some(min.map_or(*raw, |current| current.min(*raw)));
                *max = Some(max.map_or(*raw, |current| current.max(*raw)));
            }
            (
                Self::Double {
                    count,
                    sum,
                    min,
                    max,
                    ..
                },
                SqlValue::Real(raw),
            ) => {
                *count += 1;
                *sum += raw;
                *min = Some(min.map_or(*raw, |current| current.min(*raw)));
                *max = Some(max.map_or(*raw, |current| current.max(*raw)));
            }
            (
                Self::Double {
                    count,
                    sum,
                    min,
                    max,
                    ..
                },
                SqlValue::Integer(raw),
            ) => {
                // Integers only land in double columns when they fit the mantissa (load
                // guarantees it); verify still accepts them as exact widened values.
                let widened = *raw as f64;
                *count += 1;
                *sum += widened;
                *min = Some(min.map_or(widened, |current| current.min(widened)));
                *max = Some(max.map_or(widened, |current| current.max(widened)));
            }
            (
                Self::Text {
                    count,
                    min_length,
                    max_length,
                    digest,
                    ..
                },
                SqlValue::Text(raw),
            ) => {
                let length = raw.chars().count() as u64;
                *count += 1;
                *min_length = Some(min_length.map_or(length, |current| current.min(length)));
                *max_length = Some(max_length.map_or(length, |current| current.max(length)));
                digest.add_value(raw.as_bytes());
            }
            (
                Self::Bytes {
                    count,
                    min_length,
                    max_length,
                    total_length,
                    ..
                },
                SqlValue::Bytes(raw),
            ) => {
                let length = raw.len() as u64;
                *count += 1;
                *min_length = Some(min_length.map_or(length, |current| current.min(length)));
                *max_length = Some(max_length.map_or(length, |current| current.max(length)));
                *total_length += u128::from(length);
            }
            (
                Self::Numeric {
                    count,
                    sum,
                    min,
                    max,
                    ..
                },
                value,
            ) => {
                let decimal = numeric_decimal(value)?;
                *count += 1;
                *sum = sum.checked_add(decimal)?;
                let keep_min = match *min {
                    Some(current) => current.compare(decimal)? == std::cmp::Ordering::Less,
                    None => false,
                };
                if !keep_min {
                    *min = Some(decimal);
                }
                let keep_max = match *max {
                    Some(current) => current.compare(decimal)? == std::cmp::Ordering::Greater,
                    None => false,
                };
                if !keep_max {
                    *max = Some(decimal);
                }
            }
            (state, value) => {
                return Err(format!(
                    "column {column} maps to {} but the value is {}",
                    state.kind().pg_type_name(),
                    value.runtime_kind()
                ));
            }
        }
        Ok(())
    }

    pub fn view(&self) -> AggregateView {
        match self {
            Self::Integer {
                count,
                nulls,
                sum,
                min,
                max,
            } => AggregateView {
                count: *count,
                nulls: *nulls,
                sum: Some(sum.to_string()),
                min: min.map(|value| value.to_string()),
                max: max.map(|value| value.to_string()),
                min_length: None,
                max_length: None,
                total_length: None,
                value_digest: None,
            },
            Self::Double {
                count,
                nulls,
                sum,
                min,
                max,
            } => AggregateView {
                count: *count,
                nulls: *nulls,
                sum: Some(crate::types::format_float(*sum)),
                min: min.map(crate::types::format_float),
                max: max.map(crate::types::format_float),
                min_length: None,
                max_length: None,
                total_length: None,
                value_digest: None,
            },
            Self::Text {
                count,
                nulls,
                min_length,
                max_length,
                digest,
            } => AggregateView {
                count: *count,
                nulls: *nulls,
                sum: None,
                min: None,
                max: None,
                min_length: *min_length,
                max_length: *max_length,
                total_length: None,
                value_digest: Some(digest.hex()),
            },
            Self::Bytes {
                count,
                nulls,
                min_length,
                max_length,
                total_length,
            } => AggregateView {
                count: *count,
                nulls: *nulls,
                sum: None,
                min: None,
                max: None,
                min_length: *min_length,
                max_length: *max_length,
                total_length: Some(total_length.to_string()),
                value_digest: None,
            },
            Self::Numeric {
                count,
                nulls,
                sum,
                min,
                max,
            } => AggregateView {
                count: *count,
                nulls: *nulls,
                sum: Some(sum.render()),
                min: min.map(|value| value.render()),
                max: max.map(|value| value.render()),
                min_length: None,
                max_length: None,
                total_length: None,
                value_digest: None,
            },
        }
    }

    /// Compares two aggregates of the same column. `double precision` uses a relative
    /// tolerance; every other kind is compared exactly.
    pub fn compare(&self, other: &Self) -> ColumnComparison {
        if self.kind() != other.kind() {
            return ColumnComparison::failed(format!(
                "aggregate kinds differ: {} vs {}",
                self.kind().as_str(),
                other.kind().as_str()
            ));
        }
        let tolerance = (self.kind() == ColumnKind::Double).then_some(FLOAT_RELATIVE_TOLERANCE);
        let detail = match (self, other) {
            (
                Self::Integer {
                    count: left_count,
                    nulls: left_nulls,
                    sum: left_sum,
                    min: left_min,
                    max: left_max,
                },
                Self::Integer {
                    count: right_count,
                    nulls: right_nulls,
                    sum: right_sum,
                    min: right_min,
                    max: right_max,
                },
            ) => first_difference([
                eq_field("count", left_count, right_count),
                eq_field("nulls", left_nulls, right_nulls),
                eq_field("sum", left_sum, right_sum),
                eq_field("min", left_min, right_min),
                eq_field("max", left_max, right_max),
            ]),
            (
                Self::Double {
                    count: left_count,
                    nulls: left_nulls,
                    sum: left_sum,
                    min: left_min,
                    max: left_max,
                },
                Self::Double {
                    count: right_count,
                    nulls: right_nulls,
                    sum: right_sum,
                    min: right_min,
                    max: right_max,
                },
            ) => first_difference([
                eq_field("count", left_count, right_count),
                eq_field("nulls", left_nulls, right_nulls),
                approx_field("sum", *left_sum, *right_sum),
                approx_option("min", left_min, right_min),
                approx_option("max", left_max, right_max),
            ]),
            (
                Self::Text {
                    count: left_count,
                    nulls: left_nulls,
                    min_length: left_min_length,
                    max_length: left_max_length,
                    digest: left_digest,
                },
                Self::Text {
                    count: right_count,
                    nulls: right_nulls,
                    min_length: right_min_length,
                    max_length: right_max_length,
                    digest: right_digest,
                },
            ) => first_difference([
                eq_field("count", left_count, right_count),
                eq_field("nulls", left_nulls, right_nulls),
                eq_field("minLength", left_min_length, right_min_length),
                eq_field("maxLength", left_max_length, right_max_length),
                eq_field("valueDigest", &left_digest.hex(), &right_digest.hex()),
            ]),
            (
                Self::Bytes {
                    count: left_count,
                    nulls: left_nulls,
                    min_length: left_min_length,
                    max_length: left_max_length,
                    total_length: left_total,
                },
                Self::Bytes {
                    count: right_count,
                    nulls: right_nulls,
                    min_length: right_min_length,
                    max_length: right_max_length,
                    total_length: right_total,
                },
            ) => first_difference([
                eq_field("count", left_count, right_count),
                eq_field("nulls", left_nulls, right_nulls),
                eq_field("minLength", left_min_length, right_min_length),
                eq_field("maxLength", left_max_length, right_max_length),
                eq_field("totalLength", left_total, right_total),
            ]),
            (
                Self::Numeric {
                    count: left_count,
                    nulls: left_nulls,
                    sum: left_sum,
                    min: left_min,
                    max: left_max,
                },
                Self::Numeric {
                    count: right_count,
                    nulls: right_nulls,
                    sum: right_sum,
                    min: right_min,
                    max: right_max,
                },
            ) => first_difference([
                eq_field("count", left_count, right_count),
                eq_field("nulls", left_nulls, right_nulls),
                eq_field("sum", &left_sum.render(), &right_sum.render()),
                eq_field(
                    "min",
                    &left_min.map(|value| value.render()),
                    &right_min.map(|value| value.render()),
                ),
                eq_field(
                    "max",
                    &left_max.map(|value| value.render()),
                    &right_max.map(|value| value.render()),
                ),
            ]),
            _ => Some("aggregate kinds differ".to_string()),
        };
        match detail {
            Some(detail) => ColumnComparison {
                ok: false,
                detail: Some(detail),
                tolerance,
            },
            None => ColumnComparison {
                ok: true,
                detail: None,
                tolerance,
            },
        }
    }
}

fn numeric_decimal(value: &SqlValue) -> Result<Decimal, String> {
    match value {
        SqlValue::Integer(raw) => Ok(Decimal::from_i64(*raw)),
        SqlValue::Real(raw) => Decimal::from_f64(*raw),
        SqlValue::Text(raw) => Decimal::parse(raw),
        other => Err(format!(
            "numeric column received storage class {}",
            other.runtime_kind()
        )),
    }
}

fn eq_field<T: PartialEq + std::fmt::Debug>(name: &str, left: &T, right: &T) -> Option<String> {
    (left != right).then(|| format!("{name}: {left:?} != {right:?}"))
}

fn approx_field(name: &str, left: f64, right: f64) -> Option<String> {
    (!approx_eq(left, right)).then(|| {
        format!(
            "{name}: {} != {} (relative tolerance {FLOAT_RELATIVE_TOLERANCE:e})",
            crate::types::format_float(left),
            crate::types::format_float(right)
        )
    })
}

fn approx_option(name: &str, left: &Option<f64>, right: &Option<f64>) -> Option<String> {
    match (left, right) {
        (None, None) => None,
        (Some(left), Some(right)) => approx_field(name, *left, *right),
        _ => Some(format!("{name}: {left:?} != {right:?}")),
    }
}

pub fn approx_eq(left: f64, right: f64) -> bool {
    if left == right {
        return true;
    }
    if left.is_nan() || right.is_nan() {
        return false;
    }
    let difference = (left - right).abs();
    let scale = left.abs().max(right.abs());
    difference <= FLOAT_RELATIVE_TOLERANCE * scale
}

fn first_difference<const N: usize>(checks: [Option<String>; N]) -> Option<String> {
    checks.into_iter().flatten().next()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateView {
    pub count: u64,
    pub nulls: u64,
    pub sum: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub total_length: Option<String>,
    pub value_digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ColumnComparison {
    pub ok: bool,
    pub detail: Option<String>,
    pub tolerance: Option<f64>,
}

impl ColumnComparison {
    pub fn failed(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: Some(detail.into()),
            tolerance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AggregateState, FLOAT_RELATIVE_TOLERANCE, approx_eq};
    use crate::types::{ColumnKind, SqlValue};

    fn double_state(values: &[f64]) -> AggregateState {
        let mut state = AggregateState::new(ColumnKind::Double);
        for value in values {
            state.observe("weight", &SqlValue::Real(*value)).unwrap();
        }
        state
    }

    #[test]
    fn double_sums_compare_with_relative_tolerance() {
        let left = double_state(&[0.1, 0.2, 1.0 / 3.0]);
        let right = double_state(&[0.1, 0.2, 1.0 / 3.0 + 1e-15]);
        let comparison = left.compare(&right);
        assert!(comparison.ok, "{:?}", comparison.detail);
        assert_eq!(comparison.tolerance, Some(FLOAT_RELATIVE_TOLERANCE));

        let drifted = double_state(&[0.1, 0.2, 0.5]);
        assert!(!left.compare(&drifted).ok);
    }

    #[test]
    fn integer_aggregates_are_exact() {
        let mut left = AggregateState::new(ColumnKind::Integer);
        let mut right = AggregateState::new(ColumnKind::Integer);
        for value in [1i64, 2] {
            left.observe("id", &SqlValue::Integer(value)).unwrap();
            right.observe("id", &SqlValue::Integer(value)).unwrap();
        }
        assert!(left.compare(&right).ok);

        let mut shifted = AggregateState::new(ColumnKind::Integer);
        for value in [1i64, 3] {
            shifted.observe("id", &SqlValue::Integer(value)).unwrap();
        }
        assert!(!left.compare(&shifted).ok);
    }

    #[test]
    fn text_digest_sees_value_changes() {
        let mut left = AggregateState::new(ColumnKind::Text);
        let mut right = AggregateState::new(ColumnKind::Text);
        for value in ["alpha", "beta"] {
            left.observe("path", &SqlValue::Text(value.to_string()))
                .unwrap();
        }
        right
            .observe("path", &SqlValue::Text("alpha".to_string()))
            .unwrap();
        right
            .observe("path", &SqlValue::Text("gamma".to_string()))
            .unwrap();
        assert!(!left.compare(&right).ok);
    }

    #[test]
    fn rejects_wrong_storage_class() {
        let mut state = AggregateState::new(ColumnKind::Integer);
        assert!(
            state
                .observe("quota", &SqlValue::Text("12".to_string()))
                .is_err()
        );
    }

    #[test]
    fn approx_handles_boundaries() {
        assert!(approx_eq(0.0, 0.0));
        assert!(!approx_eq(1.0, 1.0 + 1e-9));
        assert!(approx_eq(f64::INFINITY, f64::INFINITY));
        assert!(!approx_eq(f64::NAN, f64::NAN));
    }

    #[test]
    fn numeric_aggregates_stay_exact() {
        let mut left = AggregateState::new(ColumnKind::Numeric);
        let mut right = AggregateState::new(ColumnKind::Numeric);
        for value in ["0.1", "0.2"] {
            left.observe("amount", &SqlValue::Text(value.to_string()))
                .unwrap();
            right
                .observe("amount", &SqlValue::Text(value.to_string()))
                .unwrap();
        }
        assert!(left.compare(&right).ok);

        let mut shifted = right;
        shifted
            .observe("amount", &SqlValue::Text("0.0001".to_string()))
            .unwrap();
        assert!(!left.compare(&shifted).ok);
    }
}
