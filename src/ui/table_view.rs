use crate::app::TableEditState;
use crate::record::{build_field_bytes, format_field_value, RecordBuffer};
use crate::schema::{validate_value, Schema};
use eframe::egui;
use egui::RichText;
use egui_extras::{Column, TableBuilder};

pub fn draw(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    highlighted_field: &mut Option<usize>,
    edit_state: &mut Option<TableEditState>,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        crate::theme::section_heading(ui, "フィールド分解");
        ui.separator();
        ui.label(format!(
            "レコード {} / {}",
            if buffer.record_count(schema) == 0 {
                0
            } else {
                buffer.current_record + 1
            },
            buffer.record_count(schema)
        ));
    });
    ui.separator();

    let record_idx = buffer.current_record;
    let Some(record_bytes) = buffer.record_slice(schema, record_idx) else {
        ui.label("(現在のレコードがありません)");
        return;
    };

    // For variant-aware schemas, announce which variant this record belongs to.
    if let Some(variant) = schema.variant_for(record_bytes) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "バリアント: [{}] {}",
                    variant.key, variant.name
                ))
                .color(crate::theme::ACCENT)
                .strong(),
            );
            if !variant.description.is_empty() {
                ui.weak(&variant.description);
            }
        });
    } else if schema.is_multi_variant() {
        ui.colored_label(
            crate::theme::WARN,
            "⚠ このレコードはどのバリアントにも一致しません (ディスクリミネータ値を確認してください)",
        );
    }

    // Snapshot the fields list now (immutable borrow ends before we touch buffer mutably).
    let fields_snapshot: Vec<crate::schema::Field> =
        schema.fields_for(record_bytes).to_vec();

    // Internal vscroll is off because the outer ScrollArea in app.rs is
    // responsible — that keeps the bottom panel freely resizable down to
    // the resize handle, with overflow scrolling at the panel level.
    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .vscroll(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(40.0))   // #
        .column(Column::auto().at_least(150.0))  // name
        .column(Column::auto().at_least(60.0))   // offset
        .column(Column::auto().at_least(60.0))   // length
        .column(Column::auto().at_least(80.0))   // kind
        .column(Column::auto().at_least(110.0))  // encoding
        .column(Column::remainder().at_least(180.0)) // value (editable)
        .column(Column::auto().at_least(180.0))  // raw hex
        .header(20.0, |mut header| {
            header.col(|ui| { ui.strong("#"); });
            header.col(|ui| { ui.strong("フィールド名"); });
            header.col(|ui| { ui.strong("オフセット"); });
            header.col(|ui| { ui.strong("長さ"); });
            header.col(|ui| { ui.strong("型"); });
            header.col(|ui| { ui.strong("エンコーディング"); });
            header.col(|ui| { ui.strong("値"); });
            header.col(|ui| { ui.strong("生バイト (16進)"); });
        })
        .body(|mut body| {
            // Collect the data we need per-field before iterating (the closures
            // borrow `buffer` mutably for the edit case).
            let rows: Vec<RowData> = fields_snapshot
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let bytes = buffer
                        .field_bytes(schema, record_idx, f)
                        .unwrap_or(&[])
                        .to_vec();
                    let encoding = schema.field_encoding(f);
                    let display = format_field_value(f, encoding, &bytes);
                    let hex: String = bytes
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    RowData {
                        idx: i,
                        name: f.name.clone(),
                        offset: f.offset,
                        length: f.length,
                        kind_label: kind_label(&f.kind),
                        encoding_label: encoding.to_string(),
                        display,
                        hex,
                    }
                })
                .collect();

            for row in rows {
                body.row(22.0, |mut tr| {
                    let is_hl = *highlighted_field == Some(row.idx);
                    tr.col(|ui| {
                        let label =
                            egui::Label::new(format!("{}", row.idx + 1)).sense(egui::Sense::click());
                        if ui.add(label).clicked() {
                            *highlighted_field = Some(row.idx);
                            buffer.cursor_in_record = row.offset;
                        }
                    });
                    tr.col(|ui| {
                        let mut text = RichText::new(&row.name).monospace();
                        if is_hl {
                            text = text.strong().color(crate::theme::ACCENT);
                        }
                        if ui
                            .add(egui::Label::new(text).sense(egui::Sense::click()))
                            .clicked()
                        {
                            *highlighted_field = Some(row.idx);
                            buffer.cursor_in_record = row.offset;
                        }
                    });
                    tr.col(|ui| {
                        ui.monospace(format!("{}", row.offset));
                    });
                    tr.col(|ui| {
                        ui.monospace(format!("{}", row.length));
                    });
                    tr.col(|ui| {
                        ui.label(row.kind_label);
                    });
                    tr.col(|ui| {
                        ui.label(row.encoding_label);
                    });
                    tr.col(|ui| {
                        let editing = matches!(edit_state, Some(s) if s.field_idx == row.idx);
                        if editing {
                            let st = edit_state.as_mut().unwrap();
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut st.draft)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace),
                            );
                            let commit = resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            if let Some(err) = &st.error {
                                ui.colored_label(crate::theme::ERR, err);
                            }
                            if commit {
                                let field = &fields_snapshot[row.idx];
                                let validation = validate_value(field, &st.draft);
                                if let Err(e) = validation {
                                    st.error = Some(format!("不正な値: {e:#}"));
                                } else {
                                    let enc = schema.field_encoding(field);
                                    match build_field_bytes(field, enc, &st.draft) {
                                        Ok(_) => {
                                            match buffer.set_field_text(
                                                schema, record_idx, field, &st.draft,
                                            ) {
                                                Ok(()) => {
                                                    *status = format!(
                                                        "フィールド '{}' を更新しました",
                                                        field.name
                                                    );
                                                    *edit_state = None;
                                                }
                                                Err(e) => {
                                                    st.error =
                                                        Some(format!("書き込み失敗: {e:#}"));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            st.error = Some(format!("エンコード失敗: {e:#}"));
                                        }
                                    }
                                }
                            }
                            if cancel {
                                *edit_state = None;
                            }
                        } else {
                            let label = egui::Label::new(
                                RichText::new(&row.display).monospace(),
                            )
                            .sense(egui::Sense::click());
                            let resp = ui.add(label);
                            if resp.double_clicked() || resp.clicked_by(egui::PointerButton::Primary)
                                && resp.double_clicked()
                            {
                                // Will be handled below for double-click
                            }
                            if resp.clicked() {
                                *highlighted_field = Some(row.idx);
                                buffer.cursor_in_record = row.offset;
                            }
                            if resp.double_clicked() {
                                *edit_state = Some(TableEditState {
                                    field_idx: row.idx,
                                    draft: row.display.clone(),
                                    error: None,
                                });
                            }
                        }
                    });
                    tr.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&row.hex).monospace().color(egui::Color32::GRAY),
                            )
                            .truncate(),
                        );
                    });
                });
            }
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.weak("ヒント: 値セルをダブルクリックで編集。Enterで確定、Escで取消。");
    });
}

struct RowData {
    idx: usize,
    name: String,
    offset: usize,
    length: usize,
    kind_label: &'static str,
    encoding_label: String,
    display: String,
    hex: String,
}

fn kind_label(kind: &crate::schema::FieldKind) -> &'static str {
    use crate::schema::FieldKind::*;
    match kind {
        Text { .. } => "text",
        Numeric { .. } => "numeric",
        Decimal { .. } => "decimal",
        Date { .. } => "date",
        Bytes => "bytes",
        Filler { .. } => "filler",
    }
}
