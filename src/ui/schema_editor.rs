use crate::encoding::Encoding;
use crate::schema::{Field, FieldKind, RecordSeparator, Schema};
use eframe::egui;

#[derive(Default)]
pub struct SchemaEditorState {
    pub selected_field: Option<usize>,
    pub validation_error: Option<String>,
}

pub fn draw(
    ui: &mut egui::Ui,
    schema: &mut Schema,
    state: &mut SchemaEditorState,
    status: &mut String,
) {
    crate::theme::section_heading(ui, "スキーマエディタ");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("スキーマ名:");
        ui.text_edit_singleline(&mut schema.name);
    });
    ui.horizontal(|ui| {
        ui.label("レコード長:");
        let mut len = schema.record_length as i64;
        if ui
            .add(egui::DragValue::new(&mut len).range(1..=65536).speed(1.0))
            .changed()
        {
            schema.record_length = len.max(1) as usize;
        }
        ui.label("バイト");
    });
    ui.horizontal(|ui| {
        ui.label("既定エンコーディング:");
        egui::ComboBox::from_id_salt("default_enc")
            .selected_text(schema.default_encoding.to_string())
            .show_ui(ui, |ui| {
                for e in Encoding::all() {
                    ui.selectable_value(&mut schema.default_encoding, e, e.to_string());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("レコード区切り:");
        egui::ComboBox::from_id_salt("sep")
            .selected_text(match schema.record_separator {
                RecordSeparator::None => "なし",
                RecordSeparator::Lf => "LF",
                RecordSeparator::CrLf => "CRLF",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut schema.record_separator, RecordSeparator::None, "なし");
                ui.selectable_value(&mut schema.record_separator, RecordSeparator::Lf, "LF");
                ui.selectable_value(&mut schema.record_separator, RecordSeparator::CrLf, "CRLF");
            });
    });

    ui.separator();
    ui.label("フィールド一覧");

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 80.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut remove_idx: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;
            let field_count = schema.fields.len();

            for i in 0..field_count {
                let is_sel = state.selected_field == Some(i);
                let header = format!(
                    "{:>2}: {} (オフセット={}, 長さ={})",
                    i + 1,
                    schema.fields[i].name,
                    schema.fields[i].offset,
                    schema.fields[i].length
                );
                let resp = egui::CollapsingHeader::new(header)
                    .id_salt(("field_hdr", i))
                    .default_open(is_sel)
                    .show(ui, |ui| {
                        draw_field_editor(ui, &mut schema.fields[i]);
                        ui.horizontal(|ui| {
                            if ui.button("↑ 上へ").clicked() {
                                move_up = Some(i);
                            }
                            if ui.button("↓ 下へ").clicked() {
                                move_down = Some(i);
                            }
                            if ui.button("削除").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    });
                if resp.header_response.clicked() {
                    state.selected_field = Some(i);
                }
            }

            if let Some(i) = remove_idx {
                schema.fields.remove(i);
                state.selected_field = None;
            }
            if let Some(i) = move_up {
                if i > 0 {
                    schema.fields.swap(i, i - 1);
                }
            }
            if let Some(i) = move_down {
                if i + 1 < schema.fields.len() {
                    schema.fields.swap(i, i + 1);
                }
            }
        });

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("+ フィールド追加").clicked() {
            let next_offset = schema
                .fields
                .last()
                .map(|f| f.offset + f.length)
                .unwrap_or(0);
            schema.fields.push(Field {
                name: format!("field_{}", schema.fields.len() + 1),
                offset: next_offset,
                length: 1,
                kind: FieldKind::Text { pad: 0x20 },
                encoding: None,
                description: String::new(),
            });
        }
        if ui.button("検証").clicked() {
            match schema.validate() {
                Ok(()) => {
                    state.validation_error = None;
                    *status = "スキーマは正常です".into();
                }
                Err(e) => {
                    state.validation_error = Some(format!("{e:#}"));
                    *status = format!("スキーマ不正: {e:#}");
                }
            }
        }
        if ui.button("オフセット自動整列").clicked() {
            let mut off = 0usize;
            for f in &mut schema.fields {
                f.offset = off;
                off += f.length;
            }
            schema.record_length = schema.record_length.max(off);
            *status = "オフセットを詰めて整列しました".into();
        }
    });
    if let Some(err) = &state.validation_error {
        ui.colored_label(crate::theme::ERR, err);
    }
}

fn draw_field_editor(ui: &mut egui::Ui, field: &mut Field) {
    egui::Grid::new(("field_grid", field.name.as_str()))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("フィールド名:");
            ui.text_edit_singleline(&mut field.name);
            ui.end_row();

            ui.label("オフセット:");
            let mut off = field.offset as i64;
            if ui.add(egui::DragValue::new(&mut off).range(0..=65535)).changed() {
                field.offset = off.max(0) as usize;
            }
            ui.end_row();

            ui.label("長さ:");
            let mut len = field.length as i64;
            if ui.add(egui::DragValue::new(&mut len).range(1..=4096)).changed() {
                field.length = len.max(1) as usize;
            }
            ui.end_row();

            ui.label("型:");
            ui.horizontal(|ui| {
                let cur = kind_tag(&field.kind);
                egui::ComboBox::from_id_salt(("kind_combo", field.name.as_str()))
                    .selected_text(kind_label_jp(cur))
                    .show_ui(ui, |ui| {
                        for t in ["text", "numeric", "decimal", "date", "bytes", "filler"] {
                            if ui.selectable_label(cur == t, kind_label_jp(t)).clicked() && cur != t {
                                field.kind = default_kind_for(t);
                            }
                        }
                    });
                draw_kind_params(ui, &mut field.kind);
            });
            ui.end_row();

            ui.label("エンコーディング:");
            ui.horizontal(|ui| {
                let mut use_default = field.encoding.is_none();
                if ui.checkbox(&mut use_default, "(既定値を使用)").changed() {
                    field.encoding = if use_default { None } else { Some(Encoding::ShiftJis) };
                }
                if let Some(enc) = field.encoding.as_mut() {
                    egui::ComboBox::from_id_salt(("enc_combo", field.name.as_str()))
                        .selected_text(enc.to_string())
                        .show_ui(ui, |ui| {
                            for e in Encoding::all() {
                                ui.selectable_value(enc, e, e.to_string());
                            }
                        });
                }
            });
            ui.end_row();

            ui.label("説明:");
            ui.text_edit_singleline(&mut field.description);
            ui.end_row();
        });
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

fn kind_label_jp(tag: &str) -> &'static str {
    match tag {
        "text" => "テキスト",
        "numeric" => "整数 (numeric)",
        "decimal" => "固定小数 (decimal)",
        "date" => "日付",
        "bytes" => "バイト列",
        "filler" => "予備領域 (filler)",
        _ => "(不明)",
    }
}

fn default_kind_for(tag: &str) -> FieldKind {
    match tag {
        "text" => FieldKind::Text { pad: 0x20 },
        "numeric" => FieldKind::Numeric {
            pad: 0x30,
            signed: false,
        },
        "decimal" => FieldKind::Decimal {
            pad: 0x30,
            scale: 0,
            signed: false,
        },
        "date" => FieldKind::Date {
            format: "YYYYMMDD".into(),
        },
        "bytes" => FieldKind::Bytes,
        "filler" => FieldKind::Filler { pad: 0x20 },
        _ => FieldKind::Text { pad: 0x20 },
    }
}

fn draw_kind_params(ui: &mut egui::Ui, kind: &mut FieldKind) {
    match kind {
        FieldKind::Text { pad } | FieldKind::Filler { pad } => {
            pad_byte_editor(ui, pad);
        }
        FieldKind::Numeric { pad, signed } => {
            pad_byte_editor(ui, pad);
            ui.checkbox(signed, "符号付き");
        }
        FieldKind::Decimal {
            pad,
            scale,
            signed,
        } => {
            pad_byte_editor(ui, pad);
            ui.label("小数桁:");
            let mut s = *scale as i64;
            if ui.add(egui::DragValue::new(&mut s).range(0..=18)).changed() {
                *scale = s.clamp(0, 18) as u8;
            }
            ui.checkbox(signed, "符号付き");
        }
        FieldKind::Date { format } => {
            ui.label("書式:");
            ui.text_edit_singleline(format);
        }
        FieldKind::Bytes => {
            ui.weak("(パラメータなし)");
        }
    }
}

fn pad_byte_editor(ui: &mut egui::Ui, pad: &mut u8) {
    ui.label("詰め文字:");
    let mut s = format!("{:#04X}", pad);
    if ui
        .add(egui::TextEdit::singleline(&mut s).desired_width(50.0))
        .changed()
    {
        if let Some(v) = parse_byte(&s) {
            *pad = v;
        }
    }
}

fn parse_byte(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = s.strip_prefix("#") {
        u8::from_str_radix(rest, 16).ok()
    } else {
        s.parse::<u8>().ok()
    }
}
