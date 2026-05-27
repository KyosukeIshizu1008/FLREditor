use crate::record::{format_field_value, RecordBuffer};
use crate::schema::{validate_value, Field, FieldKind, Schema};
use crate::ui::filter::{FilterCondition, FilterOp, FilterState};
use eframe::egui;
use egui::{Align, RichText};
use egui_extras::{Column, TableBuilder};

/// Per-session state for the spreadsheet view. Lives in `FlrApp`.
#[derive(Default)]
pub struct SpreadsheetState {
    pub edit: Option<EditCell>,
    /// Tracks the last record we auto-scrolled to, so we only request another
    /// scroll when an external change (search jump, hex click, …) moves the
    /// current record. Without this guard the table would re-center every frame
    /// and fight the user's manual scrolling.
    pub last_scrolled_to: Option<usize>,

    /// Currently selected variant tab (only meaningful for multi-variant
    /// schemas). `None` until the first multi-variant draw, then defaulted to
    /// the first variant.
    pub active_variant: Option<String>,

    /// Cached record indices per variant key, plus a signature so we know when
    /// to invalidate (e.g. after the buffer is reloaded or the schema swapped).
    pub variant_cache: VariantIndexCache,
}

#[derive(Default)]
pub struct VariantIndexCache {
    /// (buffer_byte_len, schema_record_length, schema_name, variant_count) —
    /// any change forces a recompute.
    pub signature: (usize, usize, String, usize),
    pub by_key: std::collections::HashMap<String, Vec<u32>>,
    /// Records that didn't match any variant (their discriminator value was
    /// not one of the declared keys). Useful for surfacing schema mismatches.
    pub unmatched: Vec<u32>,
}

pub struct EditCell {
    pub record_idx: usize,
    pub field_idx: usize,
    pub draft: String,
    pub error: Option<String>,
    /// True only on the first frame after the edit started — used to give the
    /// freshly mounted TextEdit keyboard focus.
    pub needs_focus: bool,
}

pub fn draw(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    highlighted_field: &mut Option<usize>,
    state: &mut SpreadsheetState,
    filter: &mut FilterState,
    status: &mut String,
) {
    let n_records = buffer.record_count(schema);

    // ── 1. Variant bookkeeping ──────────────────────────────────────────────
    if schema.is_multi_variant() {
        refresh_variant_cache(&mut state.variant_cache, schema, buffer);
        if state.active_variant.is_none()
            || !schema
                .variants
                .iter()
                .any(|v| Some(&v.key) == state.active_variant.as_ref())
        {
            state.active_variant = schema.variants.first().map(|v| v.key.clone());
        }
    } else {
        state.active_variant = None;
    }

    // ── 2. Title row ────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        crate::theme::section_heading(ui, "スプレッドシート");
        ui.separator();
        ui.label(format!("全 {} レコード", n_records));
        ui.separator();
        ui.weak("セルをダブルクリックで編集 / Enter 確定 / Esc 取消");
    });

    // ── 3. Variant tabs ─────────────────────────────────────────────────────
    if schema.is_multi_variant() {
        draw_variant_tabs(ui, schema, &state.variant_cache, &mut state.active_variant);
    }

    // ── 4. Resolve visible field layout based on active variant ─────────────
    let visible_fields: Vec<Field> = if let Some(key) = &state.active_variant {
        schema
            .variants
            .iter()
            .find(|v| v.key == *key)
            .map(|v| v.fields.clone())
            .unwrap_or_default()
    } else {
        schema.fields.clone()
    };

    // ── 5. Filter bar (operates on the active variant's fields) ─────────────
    draw_filter_bar(ui, schema, &visible_fields, filter, buffer, status);
    ui.separator();

    if visible_fields.is_empty() {
        ui.label("(表示できるフィールドが定義されていません)");
        return;
    }
    if n_records == 0 {
        ui.label("(レコードがありません)");
        return;
    }

    // ── 6. Compute displayed record indices ────────────────────────────────
    // Start with the variant's records (or all records for single schemas),
    // then intersect with filter.matched if a filter is active.
    let variant_indices: Option<&[u32]> = state
        .active_variant
        .as_ref()
        .and_then(|k| state.variant_cache.by_key.get(k))
        .map(|v| v.as_slice());

    let displayed: Vec<u32> = match (variant_indices, filter.active) {
        (Some(vi), true) => vi
            .iter()
            .copied()
            .filter(|i| filter.matched.binary_search(i).is_ok())
            .collect(),
        (Some(vi), false) => vi.to_vec(),
        (None, true) => filter.matched.clone(),
        (None, false) => (0..n_records as u32).collect(),
    };

    if displayed.is_empty() {
        if variant_indices.map_or(false, |vi| vi.is_empty()) {
            ui.label("(このバリアントに該当するレコードがありません)");
        } else if filter.active {
            ui.label("(絞り込み条件にマッチするレコードはありません)");
        } else {
            ui.label("(表示できるレコードがありません)");
        }
        return;
    }

    // ── 7. Build the table ──────────────────────────────────────────────────
    let row_h = 22.0;
    let should_scroll = state.last_scrolled_to != Some(buffer.current_record);

    let mut tb = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(70.0).at_least(50.0).resizable(true)); // # column

    for f in &visible_fields {
        tb = tb.column(
            Column::initial(estimate_column_width(f))
                .at_least(60.0)
                .at_most(400.0)
                .resizable(true)
                .clip(true),
        );
    }

    if should_scroll {
        if let Ok(target_row) = displayed.binary_search(&(buffer.current_record as u32)) {
            tb = tb.scroll_to_row(target_row, Some(Align::Center));
        }
    }

    tb.header(row_h, |mut header| {
        header.col(|ui| {
            ui.strong("#");
        });
        for f in &visible_fields {
            header.col(|ui| {
                let resp = ui.add(
                    egui::Label::new(RichText::new(&f.name).strong()).truncate(),
                );
                resp.on_hover_text(format!(
                    "{}\nオフセット={}, 長さ={}, 型={}",
                    if f.description.is_empty() {
                        "(説明なし)".to_string()
                    } else {
                        f.description.clone()
                    },
                    f.offset,
                    f.length,
                    kind_tag(&f.kind)
                ));
            });
        }
    })
    .body(|body| {
        body.rows(row_h, displayed.len(), |mut row| {
            let rec_idx = displayed[row.index()] as usize;
            let is_current = rec_idx == buffer.current_record;

            // record-index cell
            row.col(|ui| {
                let mut text = RichText::new(format!("{}", rec_idx + 1)).monospace();
                if is_current {
                    text = text.strong().color(crate::theme::ACCENT);
                }
                let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                if resp.clicked() {
                    buffer.current_record = rec_idx;
                    *status = format!("レコード {} を選択しました", rec_idx + 1);
                }
            });

            // field cells
            for (fi, field) in visible_fields.iter().enumerate() {
                let editing = matches!(
                    &state.edit,
                    Some(es) if es.record_idx == rec_idx && es.field_idx == fi
                );
                row.col(|ui| {
                    if editing {
                        draw_edit_cell(ui, schema, buffer, state, field, rec_idx, status);
                    } else {
                        draw_display_cell(
                            ui,
                            schema,
                            buffer,
                            highlighted_field,
                            &mut state.edit,
                            field,
                            rec_idx,
                            fi,
                            is_current,
                        );
                    }
                });
            }
        });
    });

    if let Some(es) = &state.edit {
        if let Some(err) = &es.error {
            let field_name = visible_fields
                .get(es.field_idx)
                .map(|f| f.name.as_str())
                .unwrap_or("?");
            ui.colored_label(
                crate::theme::ERR,
                format!(
                    "レコード {} / '{}': {}",
                    es.record_idx + 1,
                    field_name,
                    err
                ),
            );
        }
    }

    state.last_scrolled_to = Some(buffer.current_record);
}

/// Render the variant tab strip. Each tab shows the variant's display name
/// and the count of records belonging to it. The currently active key is
/// updated on click.
fn draw_variant_tabs(
    ui: &mut egui::Ui,
    schema: &Schema,
    cache: &VariantIndexCache,
    active: &mut Option<String>,
) {
    ui.horizontal_wrapped(|ui| {
        for v in &schema.variants {
            let count = cache.by_key.get(&v.key).map(|v| v.len()).unwrap_or(0);
            let is_active = active.as_ref() == Some(&v.key);
            let label = if v.name.is_empty() {
                format!("[{}] ({} 件)", v.key, count)
            } else {
                format!("[{}] {} ({} 件)", v.key, v.name, count)
            };
            let text = if is_active {
                RichText::new(label).strong().color(crate::theme::ACCENT)
            } else {
                RichText::new(label).color(crate::theme::FG_MUTED)
            };
            if ui
                .selectable_label(is_active, text)
                .clicked()
            {
                *active = Some(v.key.clone());
            }
        }
        if !cache.unmatched.is_empty() {
            ui.separator();
            ui.colored_label(
                crate::theme::WARN,
                format!("⚠ 未分類: {} 件", cache.unmatched.len()),
            );
        }
    });
}

/// Recompute the per-variant record index cache when the buffer or schema has
/// changed. Cheap when the signature matches: a HashMap lookup.
fn refresh_variant_cache(
    cache: &mut VariantIndexCache,
    schema: &Schema,
    buffer: &RecordBuffer,
) {
    use rayon::prelude::*;
    let sig = (
        buffer.data.len(),
        schema.record_length,
        schema.name.clone(),
        schema.variants.len(),
    );
    if cache.signature == sig && !cache.by_key.is_empty() {
        return;
    }
    let stride = schema.stride();
    let n = buffer.record_count(schema);
    let data = buffer.data.as_slice();
    let rec_len = schema.record_length;

    // Compute (idx, Option<variant_key>) pairs in parallel, then bucket.
    let pairs: Vec<(u32, Option<String>)> = (0..n as u32)
        .into_par_iter()
        .map(|idx| {
            let rec_start = idx as usize * stride;
            if rec_start + rec_len > data.len() {
                return (idx, None);
            }
            let rec = &data[rec_start..rec_start + rec_len];
            (idx, schema.variant_for(rec).map(|v| v.key.clone()))
        })
        .collect();

    let mut by_key: std::collections::HashMap<String, Vec<u32>> =
        std::collections::HashMap::new();
    for v in &schema.variants {
        by_key.insert(v.key.clone(), Vec::new());
    }
    let mut unmatched = Vec::new();
    for (idx, key) in pairs {
        match key {
            Some(k) => by_key.entry(k).or_default().push(idx),
            None => unmatched.push(idx),
        }
    }

    cache.signature = sig;
    cache.by_key = by_key;
    cache.unmatched = unmatched;
}

fn draw_display_cell(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    highlighted_field: &mut Option<usize>,
    edit: &mut Option<EditCell>,
    field: &Field,
    rec_idx: usize,
    fi: usize,
    is_current: bool,
) {
    let bytes = buffer
        .field_bytes(schema, rec_idx, field)
        .unwrap_or(&[])
        .to_vec();
    let encoding = schema.field_encoding(field);
    let display = format_field_value(field, encoding, &bytes);

    let mut text = RichText::new(&display).monospace();
    if is_current {
        text = text.color(crate::theme::ACCENT);
    }
    if is_current && Some(fi) == *highlighted_field {
        text = text
            .strong()
            .background_color(crate::theme::ACCENT_SOFT);
    }

    let resp = ui.add(
        egui::Label::new(text)
            .sense(egui::Sense::click())
            .truncate(),
    );
    if resp.clicked() {
        buffer.current_record = rec_idx;
        buffer.cursor_in_record = field.offset;
        *highlighted_field = Some(fi);
    }
    if resp.double_clicked() {
        *edit = Some(EditCell {
            record_idx: rec_idx,
            field_idx: fi,
            draft: display,
            error: None,
            needs_focus: true,
        });
    }
}

fn draw_edit_cell(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    state: &mut SpreadsheetState,
    field: &Field,
    rec_idx: usize,
    status: &mut String,
) {
    let es = state.edit.as_mut().unwrap();
    let resp = ui.add(
        egui::TextEdit::singleline(&mut es.draft)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY),
    );
    if es.needs_focus {
        resp.request_focus();
        es.needs_focus = false;
    }

    let commit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

    if commit {
        match validate_value(field, &es.draft) {
            Err(e) => es.error = Some(format!("不正な値: {e:#}")),
            Ok(()) => match buffer.set_field_text(schema, rec_idx, field, &es.draft) {
                Ok(()) => {
                    *status = format!(
                        "レコード {} の '{}' を更新しました",
                        rec_idx + 1,
                        field.name
                    );
                    state.edit = None;
                }
                Err(e) => es.error = Some(format!("書き込み失敗: {e:#}")),
            },
        }
    }
    if cancel {
        state.edit = None;
    }
}

/// Pick a sensible initial column width from the field's byte length and the
/// length of its header name. Users can still drag to resize.
fn estimate_column_width(f: &Field) -> f32 {
    let header_px = (f.name.chars().count() as f32) * 9.0 + 20.0;
    let value_px = match &f.kind {
        FieldKind::Bytes => (f.length as f32) * 3.0 * 8.5,
        _ => (f.length as f32) * 9.5,
    };
    header_px.max(value_px).clamp(70.0, 240.0)
}

fn kind_tag(k: &FieldKind) -> &'static str {
    match k {
        FieldKind::Text { .. } => "text",
        FieldKind::Numeric { .. } => "numeric",
        FieldKind::Decimal { .. } => "decimal",
        FieldKind::Date { .. } => "date",
        FieldKind::Bytes => "bytes",
        FieldKind::Filler { .. } => "filler",
    }
}

fn draw_filter_bar(
    ui: &mut egui::Ui,
    schema: &Schema,
    visible_fields: &[Field],
    filter: &mut FilterState,
    buffer: &RecordBuffer,
    status: &mut String,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("絞り込み:").strong());

        if ui.button("+ 条件追加").clicked() && !visible_fields.is_empty() {
            filter.conditions.push(FilterCondition {
                field_name: visible_fields[0].name.clone(),
                op: FilterOp::Contains,
                value: String::new(),
            });
        }

        let has_conds = !filter.conditions.is_empty();
        if ui.add_enabled(has_conds, egui::Button::new("適用")).clicked() {
            filter.apply(schema, buffer);
            *status = format!(
                "絞り込み: {} / {} 件 ({} ms)",
                filter.matched.len(),
                buffer.record_count(schema),
                filter.last_eval_ms
            );
        }
        if ui
            .add_enabled(has_conds || filter.active, egui::Button::new("クリア"))
            .clicked()
        {
            filter.clear();
            *status = "絞り込みをクリアしました".into();
        }
    });

    let mut remove_idx: Option<usize> = None;
    for (i, c) in filter.conditions.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("  {} :", i + 1));

            egui::ComboBox::from_id_salt(("filter_field", i))
                .selected_text(c.field_name.as_str())
                .width(160.0)
                .show_ui(ui, |ui| {
                    for f in visible_fields {
                        let is_selected = c.field_name == f.name;
                        if ui.selectable_label(is_selected, &f.name).clicked() {
                            c.field_name = f.name.clone();
                        }
                    }
                });

            egui::ComboBox::from_id_salt(("filter_op", i))
                .selected_text(c.op.label())
                .width(140.0)
                .show_ui(ui, |ui| {
                    for op in FilterOp::all() {
                        ui.selectable_value(&mut c.op, *op, op.label());
                    }
                });

            ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(220.0));

            if ui.button("×").on_hover_text("条件を削除").clicked() {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx {
        filter.conditions.remove(i);
        // Removing a condition makes the cached result stale.
        filter.invalidate();
    }
}
