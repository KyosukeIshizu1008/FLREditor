use crate::encoding::Encoding;
use crate::schema::{Field, FieldKind, Schema};
use anyhow::{anyhow, Context, Result};
use memmap2::Mmap;
use std::path::{Path, PathBuf};

/// Where the buffer's bytes currently live.
///
/// Files are loaded as `Mmap` (read-only memory map → near-zero cost open even
/// for multi-GB inputs). The first mutation copies the entire byte range into
/// an owned `Vec<u8>` (copy-on-write); subsequent edits stay in `Owned`.
pub enum DataSource {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl DataSource {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            DataSource::Mmap(m) => m.as_ref(),
            DataSource::Owned(v) => v.as_slice(),
        }
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_mmap(&self) -> bool {
        matches!(self, DataSource::Mmap(_))
    }

    /// Promote the buffer to `Owned` so it can be mutated. If already `Owned`,
    /// this is a no-op. Otherwise it allocates and copies the entire current
    /// byte range — for a 12 GB mmap this is a several-second one-time cost.
    pub fn make_owned(&mut self) -> &mut Vec<u8> {
        if !matches!(self, DataSource::Owned(_)) {
            let bytes = self.as_slice().to_vec();
            *self = DataSource::Owned(bytes);
        }
        match self {
            DataSource::Owned(v) => v,
            DataSource::Mmap(_) => unreachable!(),
        }
    }
}

/// Buffer holding the entire file (mmap or owned) plus navigation/editing state.
pub struct RecordBuffer {
    pub data: DataSource,
    pub path: Option<PathBuf>,
    pub modified: bool,

    /// 0-based currently selected record.
    pub current_record: usize,
    /// 0-based byte offset of the cursor within the current record.
    pub cursor_in_record: usize,
}

impl RecordBuffer {
    pub fn new_empty(schema: &Schema, initial_records: usize) -> Self {
        let stride = schema.stride();
        let mut data = vec![0x20u8; stride * initial_records];
        if matches!(schema.record_separator, crate::schema::RecordSeparator::Lf) {
            for i in 0..initial_records {
                data[i * stride + schema.record_length] = b'\n';
            }
        } else if matches!(schema.record_separator, crate::schema::RecordSeparator::CrLf) {
            for i in 0..initial_records {
                data[i * stride + schema.record_length] = b'\r';
                data[i * stride + schema.record_length + 1] = b'\n';
            }
        }
        Self {
            data: DataSource::Owned(data),
            path: None,
            modified: false,
            current_record: 0,
            cursor_in_record: 0,
        }
    }

    pub fn load_from_path(path: &Path, schema: &Schema) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("ファイル読み込み失敗: {}", path.display()))?;
        let len = file
            .metadata()
            .with_context(|| "ファイルメタデータの取得に失敗しました")?
            .len() as usize;
        let stride = schema.stride();
        if stride == 0 {
            return Err(anyhow!("スキーマのストライド (レコード長+区切り) が 0 です"));
        }
        if len % stride != 0 {
            let remainder = len % stride;
            return Err(anyhow!(
                "ファイルサイズ {} バイトがレコード長 {} (ストライド {}) の倍数ではありません。\
                 端数 {} バイトが残ります。スキーマが間違っているか、ファイルが破損している可能性があります。",
                len,
                schema.record_length,
                stride,
                remainder
            ));
        }

        // SAFETY: mmap is unsafe because external processes (or even the kernel
        // truncating the file) could invalidate the mapping. We accept this as
        // a documented limitation — the editor expects exclusive ownership of
        // the file while it is open. Zero-length files cannot be mmap'd on some
        // platforms, so fall through to an empty owned buffer.
        let data = if len == 0 {
            DataSource::Owned(Vec::new())
        } else {
            let mmap = unsafe { Mmap::map(&file) }
                .with_context(|| format!("mmap失敗: {}", path.display()))?;
            DataSource::Mmap(mmap)
        };

        Ok(Self {
            data,
            path: Some(path.to_path_buf()),
            modified: false,
            current_record: 0,
            cursor_in_record: 0,
        })
    }

    pub fn save_to_path(&mut self, path: &Path, schema: &Schema) -> Result<()> {
        let stride = schema.stride();
        if stride == 0 {
            return Err(anyhow!("スキーマのストライド (レコード長+区切り) が 0 です"));
        }
        if self.data.len() % stride != 0 {
            let remainder = self.data.len() % stride;
            return Err(anyhow!(
                "バッファサイズ {} バイトがレコード長 {} (ストライド {}) の倍数ではありません \
                 (端数 {} バイト)。保存を中止しました。",
                self.data.len(),
                schema.record_length,
                stride,
                remainder
            ));
        }

        // If we are about to overwrite the same file we are mmap'd against,
        // the write would alias our own read-only mapping. Materialize first
        // so the mmap can be safely dropped before std::fs::write truncates.
        let same_path = self.path.as_deref() == Some(path);
        if same_path && self.data.is_mmap() {
            self.data.make_owned();
        }

        std::fs::write(path, self.data.as_slice())
            .with_context(|| format!("ファイル書き込み失敗: {}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.modified = false;
        Ok(())
    }

    pub fn record_count(&self, schema: &Schema) -> usize {
        let stride = schema.stride();
        if stride == 0 {
            0
        } else {
            self.data.len() / stride
        }
    }

    /// Byte offset of the start of record `idx`.
    pub fn record_start(&self, schema: &Schema, idx: usize) -> usize {
        idx * schema.stride()
    }

    pub fn record_slice<'a>(&'a self, schema: &Schema, idx: usize) -> Option<&'a [u8]> {
        let start = self.record_start(schema, idx);
        let end = start + schema.record_length;
        let data = self.data.as_slice();
        if end > data.len() {
            None
        } else {
            Some(&data[start..end])
        }
    }

    pub fn record_slice_mut<'a>(
        &'a mut self,
        schema: &Schema,
        idx: usize,
    ) -> Option<&'a mut [u8]> {
        let start = self.record_start(schema, idx);
        let end = start + schema.record_length;
        let data = self.data.make_owned();
        if end > data.len() {
            None
        } else {
            Some(&mut data[start..end])
        }
    }

    pub fn field_bytes<'a>(
        &'a self,
        schema: &Schema,
        record_idx: usize,
        field: &Field,
    ) -> Option<&'a [u8]> {
        let rec = self.record_slice(schema, record_idx)?;
        let end = field.offset + field.length;
        if end > rec.len() {
            None
        } else {
            Some(&rec[field.offset..end])
        }
    }

    /// Write a new value into a field, returning Ok on success.
    pub fn set_field_text(
        &mut self,
        schema: &Schema,
        record_idx: usize,
        field: &Field,
        value: &str,
    ) -> Result<()> {
        let encoding = schema.field_encoding(field);
        let bytes = build_field_bytes(field, encoding, value)?;
        let rec = self
            .record_slice_mut(schema, record_idx)
            .ok_or_else(|| anyhow!("レコード番号が範囲外です"))?;
        rec[field.offset..field.offset + field.length].copy_from_slice(&bytes);
        self.modified = true;
        Ok(())
    }

    pub fn set_byte(&mut self, abs_offset: usize, byte: u8) -> Result<()> {
        let data = self.data.make_owned();
        let b = data
            .get_mut(abs_offset)
            .ok_or_else(|| anyhow!("バイトオフセット {} は範囲外です", abs_offset))?;
        if *b != byte {
            *b = byte;
            self.modified = true;
        }
        Ok(())
    }

    /// Append a fresh blank record at the end and return its index.
    pub fn append_record(&mut self, schema: &Schema) -> usize {
        let pad = b' ';
        let new_idx = self.record_count(schema);
        let mut block = vec![pad; schema.record_length];
        for f in &schema.fields {
            let pad_byte = match &f.kind {
                FieldKind::Numeric { pad, .. }
                | FieldKind::Decimal { pad, .. }
                | FieldKind::Filler { pad }
                | FieldKind::Text { pad } => *pad,
                FieldKind::Bytes => 0x00,
                FieldKind::Date { .. } => 0x30,
            };
            for b in &mut block[f.offset..f.offset + f.length] {
                *b = pad_byte;
            }
        }
        let data = self.data.make_owned();
        data.extend_from_slice(&block);
        match schema.record_separator {
            crate::schema::RecordSeparator::Lf => data.push(b'\n'),
            crate::schema::RecordSeparator::CrLf => {
                data.push(b'\r');
                data.push(b'\n');
            }
            crate::schema::RecordSeparator::None => {}
        }
        self.modified = true;
        new_idx
    }

    pub fn delete_record(&mut self, schema: &Schema, idx: usize) -> Result<()> {
        let stride = schema.stride();
        let start = idx * stride;
        let end = start + stride;
        let data = self.data.make_owned();
        if end > data.len() {
            return Err(anyhow!("レコード番号が範囲外です"));
        }
        data.drain(start..end);
        self.modified = true;
        if self.current_record >= self.record_count(schema) && self.current_record > 0 {
            self.current_record -= 1;
        }
        Ok(())
    }
}

/// Format raw field bytes into a human-readable display string.
pub fn format_field_value(field: &Field, encoding: Encoding, bytes: &[u8]) -> String {
    match &field.kind {
        FieldKind::Text { pad } => {
            let decoded = encoding.decode(bytes);
            decoded.trim_end_matches(*pad as char).to_string()
        }
        FieldKind::Numeric { pad, signed } => {
            let decoded = encoding.decode(bytes);
            let trimmed = decoded.trim_start_matches(*pad as char);
            if *signed {
                trimmed.to_string()
            } else if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        }
        FieldKind::Decimal {
            pad,
            scale,
            signed: _,
        } => {
            let decoded = encoding.decode(bytes);
            let trimmed = decoded.trim_start_matches(*pad as char);
            let digits = if trimmed.is_empty() { "0" } else { trimmed };
            if *scale == 0 {
                digits.to_string()
            } else {
                let n = *scale as usize;
                if digits.len() <= n {
                    let frac = format!("{:0>width$}", digits, width = n);
                    format!("0.{}", frac)
                } else {
                    let split = digits.len() - n;
                    format!("{}.{}", &digits[..split], &digits[split..])
                }
            }
        }
        FieldKind::Date { format } => {
            let decoded = encoding.decode(bytes);
            if decoded.len() != format.len() {
                return decoded;
            }
            decoded
        }
        FieldKind::Bytes | FieldKind::Filler { .. } => {
            // For bytes/filler, show hex by default
            bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
        }
    }
}

/// Convert a display string back to raw bytes for storage in the field.
/// For numeric/decimal fields the value is padded with the field's pad byte.
pub fn build_field_bytes(field: &Field, encoding: Encoding, value: &str) -> Result<Vec<u8>> {
    match &field.kind {
        FieldKind::Text { pad } => encoding
            .encode_fixed(value, field.length, *pad)
            .ok_or_else(|| anyhow!("値が長すぎるか、指定エンコーディングで表現できません")),
        FieldKind::Filler { pad } => encoding
            .encode_fixed(value, field.length, *pad)
            .ok_or_else(|| anyhow!("値が長すぎるか、指定エンコーディングで表現できません")),
        FieldKind::Numeric { pad, signed } => {
            let v = value.trim();
            let (sign_char, body) = if *signed && (v.starts_with('+') || v.starts_with('-')) {
                (Some(v.as_bytes()[0]), &v[1..])
            } else {
                (None, v)
            };
            if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
                return Err(anyhow!("数値フィールドには数字が必要です"));
            }
            let avail = if sign_char.is_some() {
                field.length - 1
            } else {
                field.length
            };
            if body.len() > avail {
                return Err(anyhow!("値がフィールド長を超えています"));
            }
            let mut buf = vec![*pad; field.length];
            let start = field.length - body.len();
            buf[start..].copy_from_slice(body.as_bytes());
            if let Some(c) = sign_char {
                buf[0] = c;
            }
            Ok(buf)
        }
        FieldKind::Decimal {
            pad,
            scale,
            signed,
        } => {
            let v = value.trim();
            let (sign_char, body) = if *signed && (v.starts_with('+') || v.starts_with('-')) {
                (Some(v.as_bytes()[0]), &v[1..])
            } else {
                (None, v)
            };
            let (int_part, frac_part) = match body.split_once('.') {
                Some((a, b)) => (a, b),
                None => (body, ""),
            };
            if !int_part.chars().all(|c| c.is_ascii_digit())
                || !frac_part.chars().all(|c| c.is_ascii_digit())
            {
                return Err(anyhow!("不正な小数表現です"));
            }
            if frac_part.len() as u8 > *scale {
                return Err(anyhow!("小数桁が scale={} を超えています", scale));
            }
            // pad fractional part to `scale` with trailing zeros
            let mut frac_padded = frac_part.to_string();
            while (frac_padded.len() as u8) < *scale {
                frac_padded.push('0');
            }
            let digits = format!("{}{}", int_part, frac_padded);
            let digits = digits.trim_start_matches('0');
            let digits = if digits.is_empty() { "0" } else { digits };
            let avail = if sign_char.is_some() {
                field.length - 1
            } else {
                field.length
            };
            if digits.len() > avail {
                return Err(anyhow!("値がフィールド長を超えています"));
            }
            let mut buf = vec![*pad; field.length];
            let start = field.length - digits.len();
            buf[start..].copy_from_slice(digits.as_bytes());
            if let Some(c) = sign_char {
                buf[0] = c;
            }
            Ok(buf)
        }
        FieldKind::Date { format } => {
            crate::schema::validate_value(field, value)?;
            let _ = format;
            // ASCII bytes
            if value.len() != field.length {
                return Err(anyhow!(
                    "日付の長さが一致しません (期待={}, 入力={})",
                    field.length,
                    value.len()
                ));
            }
            Ok(value.as_bytes().to_vec())
        }
        FieldKind::Bytes => {
            let cleaned: String = value.chars().filter(|c| !c.is_whitespace()).collect();
            if cleaned.len() != field.length * 2 {
                return Err(anyhow!(
                    "16進文字 {} 個を期待しましたが {} 個でした",
                    field.length * 2,
                    cleaned.len()
                ));
            }
            let mut out = Vec::with_capacity(field.length);
            for chunk in cleaned.as_bytes().chunks(2) {
                let s = std::str::from_utf8(chunk).unwrap();
                let b = u8::from_str_radix(s, 16).map_err(|_| anyhow!("不正な16進"))?;
                out.push(b);
            }
            Ok(out)
        }
    }
}
