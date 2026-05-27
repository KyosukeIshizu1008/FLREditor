//! DB-like multi-condition filtering for the spreadsheet view.
//!
//! All conditions are AND'd together. Evaluation is parallelized across
//! records via rayon — the typical 100M-record case spends most of its time
//! decoding field bytes per record, which is embarrassingly parallel.

use crate::record::{format_field_value, RecordBuffer};
use crate::schema::{FieldKind, Schema};
use rayon::prelude::*;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterOp {
    Contains,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl FilterOp {
    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Contains => "含む",
            FilterOp::Equals => "= 等しい",
            FilterOp::NotEquals => "≠ 等しくない",
            FilterOp::StartsWith => "で始まる",
            FilterOp::EndsWith => "で終わる",
            FilterOp::Gt => "> より大きい",
            FilterOp::Gte => "≥ 以上",
            FilterOp::Lt => "< より小さい",
            FilterOp::Lte => "≤ 以下",
        }
    }

    pub fn all() -> &'static [FilterOp] {
        &[
            FilterOp::Contains,
            FilterOp::Equals,
            FilterOp::NotEquals,
            FilterOp::StartsWith,
            FilterOp::EndsWith,
            FilterOp::Gt,
            FilterOp::Gte,
            FilterOp::Lt,
            FilterOp::Lte,
        ]
    }
}

#[derive(Clone)]
pub struct FilterCondition {
    /// Field name (looked up per-record in `Schema::fields_for`). Storing the
    /// name (not an index) lets the same condition apply correctly across
    /// records of different variants in a multi-variant schema.
    pub field_name: String,
    pub op: FilterOp,
    pub value: String,
}

#[derive(Default)]
pub struct FilterState {
    pub conditions: Vec<FilterCondition>,
    /// Record indices that pass all conditions, sorted ascending.
    /// Populated by `apply`; cleared by `clear` or when buffer changes.
    pub matched: Vec<u32>,
    /// True when `matched` reflects the current conditions.
    pub active: bool,
    /// Elapsed time of the last `apply`, shown in the status bar.
    pub last_eval_ms: u128,
}

impl FilterState {
    pub fn clear(&mut self) {
        self.conditions.clear();
        self.matched.clear();
        self.active = false;
        self.last_eval_ms = 0;
    }

    /// Drop the cached result without touching conditions — call after the
    /// buffer changes (file open / record append / delete) so the next view
    /// recomputes on demand.
    pub fn invalidate(&mut self) {
        self.matched.clear();
        self.active = false;
    }

    pub fn apply(&mut self, schema: &Schema, buffer: &RecordBuffer) {
        if self.conditions.is_empty() {
            self.matched.clear();
            self.active = false;
            self.last_eval_ms = 0;
            return;
        }

        let t0 = Instant::now();
        let stride = schema.stride();
        let n = buffer.record_count(schema);
        let data = buffer.data.as_slice();
        let conditions = self.conditions.clone();
        let rec_len = schema.record_length;

        let matched: Vec<u32> = (0..n as u32)
            .into_par_iter()
            .filter(|&idx| {
                let rec_start = idx as usize * stride;
                if rec_start + rec_len > data.len() {
                    return false;
                }
                let rec = &data[rec_start..rec_start + rec_len];
                conditions.iter().all(|c| eval_condition(c, schema, rec))
            })
            .collect();

        self.matched = matched;
        self.active = true;
        self.last_eval_ms = t0.elapsed().as_millis();
    }

    /// Total rows to display in the spreadsheet, given the active filter.
    #[allow(dead_code)]
    pub fn displayed_count(&self, total: usize) -> usize {
        if self.active {
            self.matched.len()
        } else {
            total
        }
    }

    /// Map a 0-based display row to the underlying record index in the buffer.
    #[allow(dead_code)]
    pub fn row_to_record(&self, row: usize, total: usize) -> Option<usize> {
        if self.active {
            self.matched.get(row).map(|&i| i as usize)
        } else if row < total {
            Some(row)
        } else {
            None
        }
    }
}

fn eval_condition(c: &FilterCondition, schema: &Schema, rec: &[u8]) -> bool {
    // Look up the field by name in *this record's* variant layout.
    // Multi-variant schemas pick a different field list per record, so storing
    // a field index would be ambiguous; storing the name lets us evaluate
    // safely across variants (records that don't have a matching field simply
    // don't satisfy the condition).
    let Some(field) = schema.fields_for(rec).iter().find(|f| f.name == c.field_name) else {
        return false;
    };
    if field.offset + field.length > rec.len() {
        return false;
    }
    let bytes = &rec[field.offset..field.offset + field.length];
    let encoding = schema.field_encoding(field);
    let display = format_field_value(field, encoding, bytes);
    let display_trim = display.trim();
    let q = c.value.as_str();
    let q_trim = q.trim();

    let is_numeric = matches!(
        field.kind,
        FieldKind::Numeric { .. } | FieldKind::Decimal { .. }
    );

    match c.op {
        FilterOp::Contains => display.contains(q),
        FilterOp::Equals => display_trim == q_trim,
        FilterOp::NotEquals => display_trim != q_trim,
        FilterOp::StartsWith => display_trim.starts_with(q_trim),
        FilterOp::EndsWith => display_trim.ends_with(q_trim),
        FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
            if is_numeric {
                let (a, b) = match (display_trim.parse::<f64>(), q_trim.parse::<f64>()) {
                    (Ok(a), Ok(b)) => (a, b),
                    _ => return false,
                };
                match c.op {
                    FilterOp::Gt => a > b,
                    FilterOp::Gte => a >= b,
                    FilterOp::Lt => a < b,
                    FilterOp::Lte => a <= b,
                    _ => unreachable!(),
                }
            } else {
                // Lexicographic comparison — useful for dates in YYYYMMDD form
                // and for sortable identifiers.
                match c.op {
                    FilterOp::Gt => display_trim > q_trim,
                    FilterOp::Gte => display_trim >= q_trim,
                    FilterOp::Lt => display_trim < q_trim,
                    FilterOp::Lte => display_trim <= q_trim,
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_varied_data_by_substring() {
        let schema = crate::schema::Schema::sample_120();
        let Ok(buf) = crate::record::RecordBuffer::load_from_path(
            std::path::Path::new("samples/varied_1000.dat"),
            &schema,
        ) else {
            eprintln!("skip");
            return;
        };
        let mut filter = FilterState::default();
        // Find account holders containing "メディカル"
        let holder_idx = schema
            .fields
            .iter()
            .position(|f| f.name == "account_holder")
            .unwrap();
        filter.conditions.push(FilterCondition {
            field_name: "account_holder".to_string(),
            op: FilterOp::Contains,
            value: "メディカル".to_string(),
        });
        let _ = holder_idx;
        filter.apply(&schema, &buf);
        eprintln!(
            "filter hit {}/{} records in {} ms",
            filter.matched.len(),
            buf.record_count(&schema),
            filter.last_eval_ms
        );
        assert!(!filter.matched.is_empty(), "expected メディカル matches");
        assert!(filter.active);
    }

    /// Perf check for the 1M-record file. Run with:
    ///   cargo test --release bench_filter -- --nocapture --ignored
    #[test]
    #[ignore]
    fn bench_filter() {
        let schema = crate::schema::Schema::sample_120();
        let Ok(buf) = crate::record::RecordBuffer::load_from_path(
            std::path::Path::new("samples/varied_1M.dat"),
            &schema,
        ) else {
            eprintln!("skip");
            return;
        };
        eprintln!(
            "loaded {} records, threads: {}",
            buf.record_count(&schema),
            rayon::current_num_threads()
        );
        for label in ["amount > 5000000", "account_holder contains メディカル", "両方"] {
            let mut f = FilterState::default();
            match label {
                "amount > 5000000" => f.conditions.push(FilterCondition {
                    field_name: "amount".into(),
                    op: FilterOp::Gt,
                    value: "5000000".into(),
                }),
                "account_holder contains メディカル" => f.conditions.push(FilterCondition {
                    field_name: "account_holder".into(),
                    op: FilterOp::Contains,
                    value: "メディカル".into(),
                }),
                "両方" => {
                    f.conditions.push(FilterCondition {
                        field_name: "amount".into(),
                        op: FilterOp::Gt,
                        value: "5000000".into(),
                    });
                    f.conditions.push(FilterCondition {
                        field_name: "account_holder".into(),
                        op: FilterOp::Contains,
                        value: "メディカル".into(),
                    });
                }
                _ => unreachable!(),
            }
            f.apply(&schema, &buf);
            eprintln!(
                "  {label:40} -> {} 件 / {}ms",
                f.matched.len(),
                f.last_eval_ms
            );
        }
    }

    #[test]
    fn filter_numeric_compare() {
        let schema = crate::schema::Schema::sample_120();
        let Ok(buf) = crate::record::RecordBuffer::load_from_path(
            std::path::Path::new("samples/varied_1000.dat"),
            &schema,
        ) else {
            eprintln!("skip");
            return;
        };
        let mut filter = FilterState::default();
        filter.conditions.push(FilterCondition {
            field_name: "amount".into(),
            op: FilterOp::Gt,
            value: "5000000".to_string(),
        });
        filter.apply(&schema, &buf);
        eprintln!(
            "amount > 5,000,000: {} records",
            filter.matched.len()
        );
    }
}
