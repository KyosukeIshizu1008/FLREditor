use crate::encoding::Encoding;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// Text field, right-padded with `pad` (default 0x20 space).
    Text {
        #[serde(default = "default_space")]
        pad: u8,
    },
    /// Numeric digits as text. Left-padded with `pad` (default 0x30 '0').
    /// If `signed`, allows a leading '+' or '-' on the very first byte.
    Numeric {
        #[serde(default = "default_zero")]
        pad: u8,
        #[serde(default)]
        signed: bool,
    },
    /// Fixed-point decimal as text (e.g. raw "0000012345" with scale 2 -> 123.45).
    Decimal {
        #[serde(default = "default_zero")]
        pad: u8,
        #[serde(default)]
        scale: u8,
        #[serde(default)]
        signed: bool,
    },
    /// Date as YYYYMMDD (8 bytes) or YYYY-MM-DD etc — validated lexically only.
    Date {
        /// Format string using Y, M, D placeholders. Other chars are literals.
        /// Example: "YYYYMMDD" (length 8), "YYYY-MM-DD" (length 10).
        #[serde(default = "default_date_fmt")]
        format: String,
    },
    /// Opaque bytes; shown as hex.
    Bytes,
    /// Filler / reserved area. Treated as bytes but flagged in UI.
    Filler {
        #[serde(default = "default_space")]
        pad: u8,
    },
}

fn default_space() -> u8 {
    0x20
}
fn default_zero() -> u8 {
    0x30
}
fn default_date_fmt() -> String {
    "YYYYMMDD".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub offset: usize,
    pub length: usize,
    #[serde(flatten)]
    pub kind: FieldKind,
    /// Per-field encoding override. If None, the schema default is used.
    #[serde(default)]
    pub encoding: Option<Encoding>,
    #[serde(default)]
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Discriminator {
    pub offset: usize,
    pub length: usize,
    #[serde(default = "default_ascii_enc")]
    pub encoding: Encoding,
}

fn default_ascii_enc() -> Encoding {
    Encoding::Ascii
}

/// One layout of a multi-variant schema (e.g. a header record vs a data record
/// in 全銀協 format). The `key` is matched against the bytes read at the
/// schema's `discriminator` offset.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Variant {
    pub key: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub fields: Vec<Field>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Schema {
    pub name: String,
    pub record_length: usize,
    #[serde(default)]
    pub default_encoding: Encoding,
    /// Optional record separator. Most fixed-length files have none, but some use CRLF.
    #[serde(default)]
    pub record_separator: RecordSeparator,

    /// Default field layout, used when this schema has no variants OR when a
    /// record's discriminator doesn't match any declared variant.
    #[serde(default)]
    pub fields: Vec<Field>,

    /// When present, the schema is variant-aware: each record's layout is
    /// chosen by reading these bytes and matching `variants[*].key`.
    #[serde(default)]
    pub discriminator: Option<Discriminator>,

    #[serde(default)]
    pub variants: Vec<Variant>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordSeparator {
    #[default]
    None,
    Lf,
    CrLf,
}

impl RecordSeparator {
    pub fn len(self) -> usize {
        match self {
            RecordSeparator::None => 0,
            RecordSeparator::Lf => 1,
            RecordSeparator::CrLf => 2,
        }
    }
}

impl Schema {
    /// Returns the effective on-disk stride: record length + separator length.
    pub fn stride(&self) -> usize {
        self.record_length + self.record_separator.len()
    }

    pub fn field_encoding(&self, field: &Field) -> Encoding {
        field.encoding.unwrap_or(self.default_encoding)
    }

    /// True when this schema dispatches on a discriminator field.
    pub fn is_multi_variant(&self) -> bool {
        self.discriminator.is_some() && !self.variants.is_empty()
    }

    /// Look up the variant a record belongs to. Returns None for single-layout
    /// schemas or when the discriminator bytes don't match any variant key.
    pub fn variant_for<'a>(&'a self, record_bytes: &[u8]) -> Option<&'a Variant> {
        let disc = self.discriminator.as_ref()?;
        let end = disc.offset.checked_add(disc.length)?;
        if end > record_bytes.len() {
            return None;
        }
        let key_bytes = &record_bytes[disc.offset..end];
        let decoded = disc.encoding.decode(key_bytes);
        let key_trim = decoded.trim();
        self.variants
            .iter()
            .find(|v| v.key.trim() == key_trim)
    }

    /// Pick the field layout for a record. Falls back to the default
    /// `self.fields` if no variant matches.
    pub fn fields_for<'a>(&'a self, record_bytes: &[u8]) -> &'a [Field] {
        if let Some(v) = self.variant_for(record_bytes) {
            return &v.fields;
        }
        if !self.fields.is_empty() {
            return &self.fields;
        }
        // No discriminator match AND no default — fall back to the first
        // variant so the UI shows *something* rather than blank.
        self.variants
            .first()
            .map(|v| v.fields.as_slice())
            .unwrap_or(&[])
    }

    /// All distinct field layouts in this schema, for places that need to
    /// enumerate them (schema editor, search "field" combo, etc.).
    /// Returns (label, fields) tuples. For single schemas the label is empty.
    #[allow(dead_code)]
    pub fn all_layouts(&self) -> Vec<(String, &[Field])> {
        if self.is_multi_variant() {
            self.variants
                .iter()
                .map(|v| (format!("[{}] {}", v.key, v.name), v.fields.as_slice()))
                .collect()
        } else {
            vec![(String::new(), self.fields.as_slice())]
        }
    }

    /// Validate the schema: bounds, field ordering, overlaps. For multi-variant
    /// schemas, every variant's field list is validated independently.
    pub fn validate(&self) -> Result<()> {
        if self.record_length == 0 {
            return Err(anyhow!("レコード長は1以上である必要があります"));
        }

        if let Some(disc) = &self.discriminator {
            if disc.length == 0 {
                return Err(anyhow!("ディスクリミネータの長さが 0 です"));
            }
            let end = disc
                .offset
                .checked_add(disc.length)
                .ok_or_else(|| anyhow!("ディスクリミネータのオフセット+長さがオーバーフロー"))?;
            if end > self.record_length {
                return Err(anyhow!(
                    "ディスクリミネータ (オフセット={}, 長さ={}) がレコード長 {} を超えています",
                    disc.offset,
                    disc.length,
                    self.record_length
                ));
            }
            if self.variants.is_empty() {
                return Err(anyhow!(
                    "ディスクリミネータが定義されていますが、バリアントが1つもありません"
                ));
            }
            // Detect duplicate variant keys.
            let mut seen = std::collections::HashSet::new();
            for v in &self.variants {
                if !seen.insert(v.key.trim().to_string()) {
                    return Err(anyhow!("重複するバリアントキー: '{}'", v.key));
                }
                self.validate_fields(&v.fields, &format!("バリアント '{}'", v.key))?;
            }
        }

        if !self.fields.is_empty() {
            self.validate_fields(&self.fields, "既定レイアウト")?;
        }
        Ok(())
    }

    fn validate_fields(&self, fields: &[Field], context: &str) -> Result<()> {
        let mut last_end = 0usize;
        for (i, f) in fields.iter().enumerate() {
            if f.length == 0 {
                return Err(anyhow!(
                    "{}: フィールド '{}' の長さが 0 です",
                    context,
                    f.name
                ));
            }
            let end = f.offset.checked_add(f.length).ok_or_else(|| {
                anyhow!(
                    "{}: フィールド '{}' のオフセット+長さがオーバーフロー",
                    context,
                    f.name
                )
            })?;
            if end > self.record_length {
                return Err(anyhow!(
                    "{}: フィールド '{}' (オフセット={}, 長さ={}) がレコード長 {} を超えています",
                    context,
                    f.name,
                    f.offset,
                    f.length,
                    self.record_length
                ));
            }
            if i > 0 && f.offset < last_end {
                return Err(anyhow!(
                    "{}: フィールド '{}' (オフセット {}) が直前のフィールド (終端 {}) と重複しています",
                    context,
                    f.name,
                    f.offset,
                    last_end
                ));
            }
            last_end = end;
        }
        Ok(())
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read schema file {}", path.display()))?;
        let text = std::str::from_utf8(&bytes).context("schema file is not valid UTF-8")?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let schema: Schema = match ext.as_str() {
            "json" => serde_json::from_str(text).context("invalid JSON schema")?,
            _ => toml::from_str(text).context("invalid TOML schema")?,
        };
        schema.validate()?;
        Ok(schema)
    }

    /// Parse a schema directly from an in-memory TOML string.
    pub fn from_toml_str(text: &str) -> Result<Self> {
        let schema: Schema = toml::from_str(text).context("invalid TOML schema")?;
        schema.validate()?;
        Ok(schema)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let serialized = if ext == "json" {
            serde_json::to_string_pretty(self)?
        } else {
            toml::to_string_pretty(self)?
        };
        std::fs::write(path, serialized)
            .with_context(|| format!("failed to write schema to {}", path.display()))?;
        Ok(())
    }

    /// Sample 120-byte schema, used when launching without a schema file.
    pub fn sample_120() -> Self {
        Self {
            name: "sample_120byte_remittance".into(),
            record_length: 120,
            default_encoding: Encoding::ShiftJis,
            record_separator: RecordSeparator::None,
            discriminator: None,
            variants: Vec::new(),
            fields: vec![
                Field {
                    name: "record_type".into(),
                    offset: 0,
                    length: 1,
                    kind: FieldKind::Text { pad: 0x20 },
                    encoding: Some(Encoding::Ascii),
                    description: "レコード区分 (1=ヘッダ, 2=データ, 8=トレーラ, 9=エンド)".into(),
                },
                Field {
                    name: "bank_code".into(),
                    offset: 1,
                    length: 4,
                    kind: FieldKind::Numeric {
                        pad: 0x30,
                        signed: false,
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "金融機関コード".into(),
                },
                Field {
                    name: "branch_code".into(),
                    offset: 5,
                    length: 3,
                    kind: FieldKind::Numeric {
                        pad: 0x30,
                        signed: false,
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "支店コード".into(),
                },
                Field {
                    name: "account_type".into(),
                    offset: 8,
                    length: 1,
                    kind: FieldKind::Numeric {
                        pad: 0x30,
                        signed: false,
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "預金種目 (1=普通, 2=当座, 9=その他)".into(),
                },
                Field {
                    name: "account_number".into(),
                    offset: 9,
                    length: 7,
                    kind: FieldKind::Numeric {
                        pad: 0x30,
                        signed: false,
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "口座番号".into(),
                },
                Field {
                    name: "account_holder".into(),
                    offset: 16,
                    length: 30,
                    kind: FieldKind::Text { pad: 0x20 },
                    encoding: None, // uses default (Shift_JIS)
                    description: "口座名義人カナ/漢字".into(),
                },
                Field {
                    name: "transfer_date".into(),
                    offset: 46,
                    length: 8,
                    kind: FieldKind::Date {
                        format: "YYYYMMDD".into(),
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "振込指定日".into(),
                },
                Field {
                    name: "amount".into(),
                    offset: 54,
                    length: 10,
                    kind: FieldKind::Decimal {
                        pad: 0x30,
                        scale: 0,
                        signed: false,
                    },
                    encoding: Some(Encoding::Ascii),
                    description: "振込金額 (円)".into(),
                },
                Field {
                    name: "ref_no".into(),
                    offset: 64,
                    length: 20,
                    kind: FieldKind::Text { pad: 0x20 },
                    encoding: Some(Encoding::Ascii),
                    description: "顧客参照番号".into(),
                },
                Field {
                    name: "ezk_code".into(),
                    offset: 84,
                    length: 1,
                    kind: FieldKind::Text { pad: 0x20 },
                    encoding: Some(Encoding::Ascii),
                    description: "EDI情報区分".into(),
                },
                Field {
                    name: "note".into(),
                    offset: 85,
                    length: 20,
                    kind: FieldKind::Text { pad: 0x20 },
                    encoding: None,
                    description: "備考 / EDI情報".into(),
                },
                Field {
                    name: "filler".into(),
                    offset: 105,
                    length: 15,
                    kind: FieldKind::Filler { pad: 0x20 },
                    encoding: Some(Encoding::Ascii),
                    description: "予備領域".into(),
                },
            ],
        }
    }
}

/// Validate a candidate edited value against a field's kind.
pub fn validate_value(field: &Field, value: &str) -> Result<()> {
    match &field.kind {
        FieldKind::Text { .. } | FieldKind::Filler { .. } | FieldKind::Bytes => Ok(()),
        FieldKind::Numeric { signed, .. } => {
            let s = value.trim();
            let (sign_len, body) = if *signed && (s.starts_with('+') || s.starts_with('-')) {
                (1, &s[1..])
            } else {
                (0, s)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(anyhow!("数字のみ入力できます"));
            }
            if sign_len + body.len() > field.length {
                return Err(anyhow!(
                    "値 '{}' がフィールド長 {} を超えています",
                    s,
                    field.length
                ));
            }
            Ok(())
        }
        FieldKind::Decimal { scale, signed, .. } => {
            let s = value.trim();
            let body = if *signed && (s.starts_with('+') || s.starts_with('-')) {
                &s[1..]
            } else {
                s
            };
            let parts: Vec<&str> = body.split('.').collect();
            match parts.as_slice() {
                [int_part] if int_part.chars().all(|c| c.is_ascii_digit()) => Ok(()),
                [int_part, frac] => {
                    if !int_part.chars().all(|c| c.is_ascii_digit())
                        || !frac.chars().all(|c| c.is_ascii_digit())
                    {
                        return Err(anyhow!("不正な小数表現です"));
                    }
                    if frac.len() as u8 > *scale {
                        return Err(anyhow!("小数桁が scale={} を超えています", scale));
                    }
                    Ok(())
                }
                _ => Err(anyhow!("不正な小数表現です")),
            }
        }
        FieldKind::Date { format } => validate_date(value, format),
    }
}

fn validate_date(value: &str, format: &str) -> Result<()> {
    if value.len() != format.len() {
        return Err(anyhow!(
            "日付 '{}' が書式 '{}' と長さが一致しません",
            value,
            format
        ));
    }
    let (mut y, mut m, mut d) = (0i32, 0u32, 0u32);
    let (mut yc, mut mc, mut dc) = (0u32, 0u32, 0u32);
    for (vc, fc) in value.chars().zip(format.chars()) {
        match fc {
            'Y' => {
                if !vc.is_ascii_digit() {
                    return Err(anyhow!("日付に数字以外が含まれています"));
                }
                y = y * 10 + vc.to_digit(10).unwrap() as i32;
                yc += 1;
            }
            'M' => {
                if !vc.is_ascii_digit() {
                    return Err(anyhow!("日付に数字以外が含まれています"));
                }
                m = m * 10 + vc.to_digit(10).unwrap();
                mc += 1;
            }
            'D' => {
                if !vc.is_ascii_digit() {
                    return Err(anyhow!("日付に数字以外が含まれています"));
                }
                d = d * 10 + vc.to_digit(10).unwrap();
                dc += 1;
            }
            literal => {
                if vc != literal {
                    return Err(anyhow!("'{}' を期待しましたが '{}' が見つかりました", literal, vc));
                }
            }
        }
    }
    if yc == 0 || mc == 0 || dc == 0 {
        return Err(anyhow!("日付書式に Y/M/D が必要です"));
    }
    if !(1..=12).contains(&m) {
        return Err(anyhow!("月が範囲外です: {}", m));
    }
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => unreachable!(),
    };
    if !(1..=days_in_month).contains(&d) {
        return Err(anyhow!("日が範囲外です: {}", d));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_multi_variant_schema() {
        let path = Path::new("schemas/zengin_120_multi.toml");
        let schema = Schema::load_from_path(path).expect("schema should load");
        assert!(schema.is_multi_variant());
        assert_eq!(schema.variants.len(), 4);
        // Each variant has its own field layout
        let header = schema.variants.iter().find(|v| v.key == "1").unwrap();
        assert!(header.fields.iter().any(|f| f.name == "委託者名"));
        let data = schema.variants.iter().find(|v| v.key == "2").unwrap();
        assert!(data.fields.iter().any(|f| f.name == "振込金額"));
        let trailer = schema.variants.iter().find(|v| v.key == "8").unwrap();
        assert!(trailer.fields.iter().any(|f| f.name == "合計件数"));
    }

    #[test]
    fn variant_for_dispatches_by_discriminator() {
        let schema = Schema::load_from_path(Path::new("schemas/zengin_120_multi.toml"))
            .expect("schema should load");
        // Header record starts with '1'
        let mut header = vec![b' '; 120];
        header[0] = b'1';
        let v = schema.variant_for(&header).expect("should match header");
        assert_eq!(v.key, "1");
        assert_eq!(v.name, "ヘッダレコード");

        // Data record
        let mut data = vec![b' '; 120];
        data[0] = b'2';
        assert_eq!(schema.variant_for(&data).unwrap().key, "2");

        // End record
        let mut end = vec![b' '; 120];
        end[0] = b'9';
        assert_eq!(schema.variant_for(&end).unwrap().key, "9");

        // Unknown discriminator returns None
        let mut other = vec![b' '; 120];
        other[0] = b'X';
        assert!(schema.variant_for(&other).is_none());
    }
}

