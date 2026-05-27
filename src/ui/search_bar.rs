use crate::encoding::Encoding;
use crate::record::{build_field_bytes, RecordBuffer};
use crate::schema::Schema;
use eframe::egui;
use rayon::prelude::*;

/// Above this haystack size, search switches to a rayon-parallel chunk scan.
/// Below it, the sequential `windows().position()` form is faster (no thread
/// pool overhead). 1 MiB is roughly where the crossover lands on M-series.
const PAR_SEARCH_THRESHOLD: usize = 1 << 20;
/// Minimum bytes per parallel chunk, to keep thread overhead amortized.
const MIN_CHUNK_BYTES: usize = 1 << 16;

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub replace: String,
    pub mode: SearchMode,
    pub encoding: Encoding,
    pub field_idx: Option<usize>,
    pub case_insensitive: bool,
    pub last_match_abs: Option<usize>,
    pub jump_to: String,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Text,
    HexBytes,
    FieldValue,
}

pub fn draw(
    ui: &mut egui::Ui,
    state: &mut SearchState,
    buffer: &mut RecordBuffer,
    schema: &Schema,
    status: &mut String,
    highlighted_field: &mut Option<usize>,
) {
    let mut trigger_find = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("検索:");
        let query_resp = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .desired_width(220.0)
                .hint_text(match state.mode {
                    SearchMode::Text => "テキスト…",
                    SearchMode::HexBytes => "16進バイト列 例: 30 31 32 / 303132",
                    SearchMode::FieldValue => "選択中フィールドの値",
                }),
        );
        // Enter while focused in the query box triggers a search. macOS IME
        // composition consumes the first Enter to commit the conversion, so
        // the second Enter actually fires this branch — which is the natural
        // flow for Japanese input.
        if query_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            trigger_find = true;
        }

        egui::ComboBox::from_id_salt("search_mode")
            .selected_text(match state.mode {
                SearchMode::Text => "テキスト",
                SearchMode::HexBytes => "16進",
                SearchMode::FieldValue => "フィールド",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.mode, SearchMode::Text, "テキスト");
                ui.selectable_value(&mut state.mode, SearchMode::HexBytes, "16進");
                ui.selectable_value(&mut state.mode, SearchMode::FieldValue, "フィールド");
            });

        if matches!(state.mode, SearchMode::Text) {
            egui::ComboBox::from_id_salt("search_enc")
                .selected_text(state.encoding.to_string())
                .show_ui(ui, |ui| {
                    for e in Encoding::all() {
                        ui.selectable_value(&mut state.encoding, e, e.to_string());
                    }
                });
            ui.checkbox(&mut state.case_insensitive, "大文字小文字を無視");
        }

        if matches!(state.mode, SearchMode::FieldValue) {
            egui::ComboBox::from_id_salt("search_field")
                .selected_text(match state.field_idx {
                    Some(i) => schema.fields.get(i).map(|f| f.name.as_str()).unwrap_or("?"),
                    None => "(フィールド選択)",
                })
                .show_ui(ui, |ui| {
                    for (i, f) in schema.fields.iter().enumerate() {
                        ui.selectable_value(&mut state.field_idx, Some(i), &f.name);
                    }
                });
        }

        if ui.button("次を検索").clicked() || trigger_find {
            // Build pattern up-front so we can include it in the status message
            // for diagnostic purposes (lets the user verify what bytes are
            // actually being searched — useful for catching IME / encoding
            // surprises).
            let pat_preview = build_pattern(state, schema)
                .map(|p| pattern_preview(&p))
                .unwrap_or_default();
            match find_next(state, buffer, schema) {
                Ok(FindOutcome::Hit { offset, wrapped }) => {
                    let stride = schema.stride();
                    let rec_idx = offset / stride;
                    let off_in_rec = offset % stride;
                    buffer.current_record = rec_idx;
                    if off_in_rec < schema.record_length {
                        buffer.cursor_in_record = off_in_rec;
                    }
                    *highlighted_field =
                        field_at(schema, off_in_rec.min(schema.record_length.saturating_sub(1)));
                    state.last_match_abs = Some(offset);
                    *status = format!(
                        "{}ヒット: レコード {} / オフセット {} (検索: {})",
                        if wrapped { "↻ 折り返し " } else { "" },
                        rec_idx + 1,
                        off_in_rec,
                        pat_preview,
                    );
                }
                Ok(FindOutcome::NoMatch) => {
                    *status = format!("見つかりませんでした (検索: {})", pat_preview);
                }
                Err(e) => *status = format!("検索失敗: {e}"),
            }
        }

        ui.separator();
        ui.label("置換:");
        ui.add(egui::TextEdit::singleline(&mut state.replace).desired_width(160.0));
        if ui.button("次を置換").clicked() {
            match replace_next(state, buffer, schema) {
                Ok(Some(abs_off)) => {
                    *status = format!("オフセット {abs_off} を置換しました");
                    state.last_match_abs = Some(abs_off);
                }
                Ok(None) => *status = "これ以上の一致はありません".into(),
                Err(e) => *status = format!("置換失敗: {e}"),
            }
        }
        if ui.button("すべて置換").clicked() {
            match replace_all(state, buffer, schema) {
                Ok(n) => *status = format!("{n} 件を置換しました"),
                Err(e) => *status = format!("置換失敗: {e}"),
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("レコードへジャンプ:");
        ui.add(
            egui::TextEdit::singleline(&mut state.jump_to)
                .desired_width(80.0)
                .hint_text("1から開始"),
        );
        if ui.button("移動").clicked() {
            match state.jump_to.trim().parse::<usize>() {
                Ok(n) if n > 0 && n <= buffer.record_count(schema) => {
                    buffer.current_record = n - 1;
                    *status = format!("レコード {n} にジャンプしました");
                }
                _ => *status = "不正なレコード番号".into(),
            }
        }

        ui.separator();
        ui.label("オフセットへジャンプ:");
        let mut off_str = format!("{:#06X}", buffer.cursor_in_record);
        if ui
            .add(egui::TextEdit::singleline(&mut off_str).desired_width(80.0))
            .lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            let parsed = parse_offset(&off_str);
            match parsed {
                Some(off) if off < schema.record_length => {
                    buffer.cursor_in_record = off;
                    *highlighted_field = field_at(schema, off);
                    *status = format!("カーソル → オフセット {off}");
                }
                _ => *status = "不正なオフセット".into(),
            }
        }
    });
}

fn parse_offset(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        usize::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = s.strip_prefix("#") {
        usize::from_str_radix(rest, 16).ok()
    } else {
        s.parse::<usize>().ok()
    }
}

fn field_at(schema: &Schema, offset: usize) -> Option<usize> {
    for (i, f) in schema.fields.iter().enumerate() {
        if offset >= f.offset && offset < f.offset + f.length {
            return Some(i);
        }
    }
    None
}

/// Result of a single forward search step.
pub enum FindOutcome {
    /// A match was found at the given absolute byte offset.
    /// `wrapped` is true if we had to scan from offset 0 to find it (i.e. the
    /// query went past the current position).
    Hit { offset: usize, wrapped: bool },
    NoMatch,
}

/// Find the next match after `state.last_match_abs` (or after the current
/// cursor). If we hit EOF without finding anything, wrap around to offset 0 so
/// the user can find matches that lie before the current record — common when
/// you've scrolled to record N and want to search for something everywhere.
fn find_next(
    state: &SearchState,
    buffer: &RecordBuffer,
    schema: &Schema,
) -> Result<FindOutcome, String> {
    let pattern = build_pattern(state, schema)?;
    if pattern.is_empty() {
        return Err("検索文字列が空です".into());
    }

    let primary_start = match state.last_match_abs {
        Some(p) => p + 1,
        None => buffer.record_start(schema, buffer.current_record) + buffer.cursor_in_record,
    };

    if let SearchMode::FieldValue = state.mode {
        let field_idx = state.field_idx.ok_or("先にフィールドを選択してください")?;
        let field = &schema.fields[field_idx];
        let stride = schema.stride();
        let rec_count = buffer.record_count(schema);
        let data = buffer.data.as_slice();
        // forward pass from primary_start
        let from_rec = primary_start.div_ceil(stride);
        for rec in from_rec..rec_count {
            let abs = rec * stride + field.offset;
            if abs + field.length > data.len() {
                break;
            }
            if data[abs..abs + field.length] == pattern[..] {
                return Ok(FindOutcome::Hit {
                    offset: abs,
                    wrapped: false,
                });
            }
        }
        // wrap-around: 0 .. from_rec
        for rec in 0..from_rec.min(rec_count) {
            let abs = rec * stride + field.offset;
            if abs + field.length > data.len() {
                break;
            }
            if data[abs..abs + field.length] == pattern[..] {
                return Ok(FindOutcome::Hit {
                    offset: abs,
                    wrapped: true,
                });
            }
        }
        return Ok(FindOutcome::NoMatch);
    }

    let data = buffer.data.as_slice();
    if let Some(off) = find_subslice(data, &pattern, primary_start, state.case_insensitive) {
        return Ok(FindOutcome::Hit {
            offset: off,
            wrapped: false,
        });
    }
    if primary_start > 0 {
        if let Some(off) = find_subslice(data, &pattern, 0, state.case_insensitive) {
            if off < primary_start {
                return Ok(FindOutcome::Hit {
                    offset: off,
                    wrapped: true,
                });
            }
        }
    }
    Ok(FindOutcome::NoMatch)
}

/// Render a short hex preview of a pattern, for diagnostic status messages.
fn pattern_preview(pat: &[u8]) -> String {
    let n = pat.len();
    let max_show = 16;
    let shown: Vec<String> = pat
        .iter()
        .take(max_show)
        .map(|b| format!("{:02X}", b))
        .collect();
    if n > max_show {
        format!("{} … ({} バイト)", shown.join(" "), n)
    } else {
        format!("{} ({} バイト)", shown.join(" "), n)
    }
}

fn replace_next(
    state: &mut SearchState,
    buffer: &mut RecordBuffer,
    schema: &Schema,
) -> Result<Option<usize>, String> {
    let pat = build_pattern(state, schema)?;
    let rep = build_replacement(state, schema, &pat)?;
    if pat.is_empty() {
        return Err("検索文字列が空です".into());
    }
    if rep.len() != pat.len() {
        return Err(format!(
            "置換後の長さ {} が検索文字列の長さ {} と一致しません (同長のみ可)",
            rep.len(),
            pat.len()
        ));
    }
    let start = match state.last_match_abs {
        Some(p) => p + 1,
        None => buffer.record_start(schema, buffer.current_record) + buffer.cursor_in_record,
    };
    let found = find_subslice(buffer.data.as_slice(), &pat, start, state.case_insensitive);
    if let Some(off) = found {
        let data = buffer.data.make_owned();
        data[off..off + pat.len()].copy_from_slice(&rep);
        buffer.modified = true;
        state.last_match_abs = Some(off);
        Ok(Some(off))
    } else {
        Ok(None)
    }
}

fn replace_all(
    state: &mut SearchState,
    buffer: &mut RecordBuffer,
    schema: &Schema,
) -> Result<usize, String> {
    let pat = build_pattern(state, schema)?;
    let rep = build_replacement(state, schema, &pat)?;
    if pat.is_empty() {
        return Err("検索文字列が空です".into());
    }
    if rep.len() != pat.len() {
        return Err(format!(
            "置換後の長さ {} が検索文字列の長さ {} と一致しません (同長のみ可)",
            rep.len(),
            pat.len()
        ));
    }
    // Find every match in a single parallel pass, then apply sequentially.
    // Equal-length replacements mean positions remain valid after applying.
    let positions = find_all_subslice(buffer.data.as_slice(), &pat, state.case_insensitive);
    if positions.is_empty() {
        return Ok(0);
    }
    let data = buffer.data.make_owned();
    for off in &positions {
        data[*off..*off + pat.len()].copy_from_slice(&rep);
    }
    buffer.modified = true;
    Ok(positions.len())
}

fn build_pattern(state: &SearchState, schema: &Schema) -> Result<Vec<u8>, String> {
    match state.mode {
        SearchMode::Text => state
            .encoding
            .encode_exact(&state.query)
            .ok_or("指定エンコーディングで表現できない文字が含まれています".to_string()),
        SearchMode::HexBytes => parse_hex_bytes(&state.query),
        SearchMode::FieldValue => {
            let idx = state.field_idx.ok_or("フィールドを選択してください".to_string())?;
            let field = &schema.fields[idx];
            let enc = schema.field_encoding(field);
            build_field_bytes(field, enc, &state.query).map_err(|e| format!("{e:#}"))
        }
    }
}

fn build_replacement(
    state: &SearchState,
    schema: &Schema,
    pattern: &[u8],
) -> Result<Vec<u8>, String> {
    match state.mode {
        SearchMode::Text => state
            .encoding
            .encode_fixed(&state.replace, pattern.len(), b' ')
            .ok_or("置換文字列を指定エンコーディングで表現できません".into()),
        SearchMode::HexBytes => parse_hex_bytes(&state.replace),
        SearchMode::FieldValue => {
            let idx = state.field_idx.ok_or("フィールドを選択してください".to_string())?;
            let field = &schema.fields[idx];
            let enc = schema.field_encoding(field);
            build_field_bytes(field, enc, &state.replace).map_err(|e| format!("{e:#}"))
        }
    }
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("16進文字列の桁数が奇数です".into());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for chunk in cleaned.as_bytes().chunks(2) {
        let s = std::str::from_utf8(chunk).map_err(|_| "16進にASCII以外が含まれています".to_string())?;
        out.push(u8::from_str_radix(s, 16).map_err(|_| format!("不正な16進ペア '{}'", s))?);
    }
    Ok(out)
}


/// Compare a window against the needle, optionally case-insensitively.
/// Inlined for the hot search loop.
#[inline]
fn window_matches(window: &[u8], needle: &[u8], case_insensitive: bool) -> bool {
    if case_insensitive {
        window.len() == needle.len()
            && window
                .iter()
                .zip(needle)
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    } else {
        window == needle
    }
}

/// Find the earliest match of `needle` in `haystack[start..]`.
///
/// For haystacks ≥ `PAR_SEARCH_THRESHOLD` this splits the range into
/// `rayon::current_num_threads()` chunks (each overlapping the next by
/// `needle.len()-1` bytes to catch matches that straddle a chunk boundary),
/// scans them in parallel, and returns the global minimum. Below the
/// threshold it falls back to a single-threaded `windows().position()` which
/// is faster than paying thread-pool overhead.
fn find_subslice(
    haystack: &[u8],
    needle: &[u8],
    start: usize,
    case_insensitive: bool,
) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() || haystack.len() - start < needle.len() {
        return None;
    }
    let tail_len = haystack.len() - start;
    if tail_len < PAR_SEARCH_THRESHOLD {
        return find_subslice_seq(haystack, needle, start, case_insensitive);
    }

    let n_len = needle.len();
    let overlap = n_len - 1;
    let num_threads = rayon::current_num_threads().max(1);
    let chunk_size = tail_len.div_ceil(num_threads).max(MIN_CHUNK_BYTES);

    // (absolute body start, absolute body end). Search slice extends `overlap`
    // bytes past `body_end` so cross-chunk matches still get found, but matches
    // are only emitted when their starting position falls inside `[body_start, body_end)`.
    let chunks: Vec<(usize, usize)> = (0..tail_len)
        .step_by(chunk_size)
        .map(|off| {
            let body_start = start + off;
            let body_end = (body_start + chunk_size).min(haystack.len());
            (body_start, body_end)
        })
        .collect();

    chunks
        .into_par_iter()
        .filter_map(|(body_start, body_end)| {
            let search_end = (body_end + overlap).min(haystack.len());
            let slice = &haystack[body_start..search_end];
            slice
                .windows(n_len)
                .enumerate()
                .find_map(|(i, w)| {
                    let abs_pos = body_start + i;
                    if abs_pos >= body_end {
                        return None;
                    }
                    if window_matches(w, needle, case_insensitive) {
                        Some(abs_pos)
                    } else {
                        None
                    }
                })
        })
        .min()
}

fn find_subslice_seq(
    haystack: &[u8],
    needle: &[u8],
    start: usize,
    case_insensitive: bool,
) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|w| window_matches(w, needle, case_insensitive))
        .map(|p| p + start)
}

/// Find every match of `needle` in `haystack`. Returns positions in ascending
/// order. Uses the same chunk-with-overlap scheme as `find_subslice`.
fn find_all_subslice(haystack: &[u8], needle: &[u8], case_insensitive: bool) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    if haystack.len() < PAR_SEARCH_THRESHOLD {
        return haystack
            .windows(needle.len())
            .enumerate()
            .filter_map(|(i, w)| {
                if window_matches(w, needle, case_insensitive) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
    }

    let n_len = needle.len();
    let overlap = n_len - 1;
    let num_threads = rayon::current_num_threads().max(1);
    let chunk_size = haystack.len().div_ceil(num_threads).max(MIN_CHUNK_BYTES);

    let chunks: Vec<(usize, usize)> = (0..haystack.len())
        .step_by(chunk_size)
        .map(|off| (off, (off + chunk_size).min(haystack.len())))
        .collect();

    let mut positions: Vec<usize> = chunks
        .into_par_iter()
        .map(|(body_start, body_end)| {
            let search_end = (body_end + overlap).min(haystack.len());
            let slice = &haystack[body_start..search_end];
            let mut local = Vec::new();
            for (i, w) in slice.windows(n_len).enumerate() {
                let abs_pos = body_start + i;
                if abs_pos >= body_end {
                    break;
                }
                if window_matches(w, needle, case_insensitive) {
                    local.push(abs_pos);
                }
            }
            local
        })
        .flatten()
        .collect();
    positions.sort_unstable();
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::Encoding;

    #[test]
    fn encode_exact_sjis_no_padding() {
        // "メディカル" in CP932 is exactly these 10 bytes, with no trailing
        // padding. The earlier `encode_fixed(.., len*3, 0)` form padded with
        // NULL bytes, which produced patterns that never matched real data.
        let pat = Encoding::ShiftJis.encode_exact("メディカル").unwrap();
        assert_eq!(
            pat,
            vec![0x83, 0x81, 0x83, 0x66, 0x83, 0x42, 0x83, 0x4A, 0x83, 0x8B]
        );
    }

    /// End-to-end search of "メディカル" against the generated varied data.
    /// Skipped silently if the sample isn't present.
    #[test]
    fn finds_japanese_in_varied_data() {
        let path = "samples/varied_1000.dat";
        let schema = crate::schema::Schema::sample_120();
        let Ok(buf) = crate::record::RecordBuffer::load_from_path(
            std::path::Path::new(path),
            &schema,
        ) else {
            eprintln!("skip: {path} not found");
            return;
        };
        let pat = Encoding::ShiftJis.encode_exact("メディカル").unwrap();
        let pos = find_subslice(buf.data.as_slice(), &pat, 0, false);
        assert!(pos.is_some(), "メディカル should be in varied_1000.dat");
        let stride = schema.stride();
        let p = pos.unwrap();
        eprintln!(
            "first match at byte {} → record {} offset {}",
            p,
            p / stride + 1,
            p % stride
        );
    }

    /// Confirm parallel and sequential produce identical first-match results
    /// over a synthetic haystack with matches at random positions including
    /// chunk boundaries.
    #[test]
    fn parallel_matches_sequential() {
        let mut haystack = vec![0u8; 5 * 1024 * 1024]; // 5 MiB > PAR_SEARCH_THRESHOLD
        let needle = b"NEEDLE-HERE-42";
        // Non-overlapping positions spread through the haystack (matches are
        // 14 bytes; we keep each at least that far from the next). Some sit
        // adjacent to typical chunk boundaries (64 KiB and 1 MiB) so the
        // chunk-boundary handling actually gets exercised.
        let planted: &[usize] = &[
            0,
            1_000,
            65_500,
            65_540,
            100_000,
            1_048_500,
            1_048_580,
            4_000_000,
            haystack.len() - needle.len(),
        ];
        for &pos in planted {
            haystack[pos..pos + needle.len()].copy_from_slice(needle);
        }

        let seq = find_subslice_seq(&haystack, needle, 0, false);
        let par = find_subslice(&haystack, needle, 0, false);
        assert_eq!(seq, par, "seq and par disagree on first match");

        let seq2 = find_subslice_seq(&haystack, needle, 100_000, false);
        let par2 = find_subslice(&haystack, needle, 100_000, false);
        assert_eq!(seq2, par2);

        let all = find_all_subslice(&haystack, needle, false);
        assert!(all.windows(2).all(|w| w[0] < w[1]), "positions not sorted");
        assert_eq!(all, planted, "find_all missed or invented a match");
    }

    /// Quick perf print over a 120 MB file (1M records × 120 bytes).
    /// Skipped silently if the sample isn't present. Run with:
    ///   cargo test bench_mmap_rayon --release -- --nocapture --ignored
    #[test]
    #[ignore]
    fn bench_mmap_rayon() {
        use memmap2::Mmap;
        use std::time::Instant;

        let path = "samples/big_1M.dat";
        let Ok(file) = std::fs::File::open(path) else {
            eprintln!("skip: {path} not found");
            return;
        };

        let t = Instant::now();
        let mmap = unsafe { Mmap::map(&file) }.unwrap();
        let t_open = t.elapsed();
        let len = mmap.len();

        // Two scenarios:
        //  (a) early-hit: needle appears in every record → first match very near
        //      start. Sequential wins trivially.
        //  (b) miss: needle that never appears → full scan of the whole file.
        //      This is the case where rayon should shine.
        let early = b"REF20260131-0001    ";
        let missing = b"__THIS_NEEDLE_DOES_NOT_EXIST_ANYWHERE__";

        let t = Instant::now();
        let p_seq_a = find_subslice_seq(&mmap, early, 0, false);
        let t_seq_a = t.elapsed();
        let t = Instant::now();
        let p_par_a = find_subslice(&mmap, early, 0, false);
        let t_par_a = t.elapsed();

        let t = Instant::now();
        let p_seq_b = find_subslice_seq(&mmap, missing, 0, false);
        let t_seq_b = t.elapsed();
        let t = Instant::now();
        let p_par_b = find_subslice(&mmap, missing, 0, false);
        let t_par_b = t.elapsed();

        let t = Instant::now();
        let all = find_all_subslice(&mmap, early, false);
        let t_par_all = t.elapsed();

        eprintln!("--- mmap + rayon bench ---");
        eprintln!("file: {} ({} MiB), threads: {}", path, len / (1 << 20), rayon::current_num_threads());
        eprintln!("mmap open:              {:?}", t_open);
        eprintln!("[early-hit] seq:        {:?}  -> {:?}", t_seq_a, p_seq_a);
        eprintln!("[early-hit] par:        {:?}  -> {:?}", t_par_a, p_par_a);
        eprintln!("[full-scan miss] seq:   {:?}  -> {:?}", t_seq_b, p_seq_b);
        eprintln!("[full-scan miss] par:   {:?}  -> {:?}", t_par_b, p_par_b);
        eprintln!(
            "find_all_subslice (early): {:?}  ({} matches)",
            t_par_all,
            all.len()
        );
        assert_eq!(p_seq_a, p_par_a);
        assert_eq!(p_seq_b, p_par_b);
    }
}
