use crate::encoding::Encoding;
use crate::schema::{Discriminator, Field, FieldKind, RecordSeparator, Schema, Variant};
use eframe::egui;

#[derive(Default)]
pub struct SchemaEditorState {
    pub selected_field: Option<usize>,
    pub validation_error: Option<String>,
    /// Active variant index (only meaningful when the schema is multi-variant).
    pub active_variant: Option<usize>,
}

pub fn draw(
    ui: &mut egui::Ui,
    schema: &mut Schema,
    state: &mut SchemaEditorState,
    status: &mut String,
) {
    crate::theme::section_heading(ui, "スキーマエディタ");
    ui.separator();

    draw_schema_meta(ui, schema);
    ui.separator();

    draw_discriminator_section(ui, schema, state);

    if schema.discriminator.is_some() {
        ui.separator();
        draw_variant_picker(ui, schema, state);
    } else {
        state.active_variant = None;
    }

    ui.separator();

    let header_text = if let Some(idx) = state.active_variant {
        match schema.variants.get(idx) {
            Some(v) => format!(
                "フィールド一覧 — バリアント [{}] {}",
                v.key,
                if v.name.is_empty() { "(無名)" } else { v.name.as_str() }
            ),
            None => "フィールド一覧".into(),
        }
    } else {
        "フィールド一覧".into()
    };
    ui.label(header_text);

    let mut want_validate = false;
    let mut auto_align_end: Option<usize> = None;

    {
        let target: Option<&mut Vec<Field>> = match state.active_variant {
            Some(idx) => schema.variants.get_mut(idx).map(|v| &mut v.fields),
            None => {
                if schema.discriminator.is_some() {
                    None
                } else {
                    Some(&mut schema.fields)
                }
            }
        };

        if let Some(fields) = target {
            draw_field_list(ui, fields, &mut state.selected_field);

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("+ フィールド追加").clicked() {
                    let next_offset = fields.last().map(|f| f.offset + f.length).unwrap_or(0);
                    fields.push(Field {
                        name: format!("field_{}", fields.len() + 1),
                        offset: next_offset,
                        length: 1,
                        kind: FieldKind::Text { pad: 0x20 },
                        encoding: None,
                        description: String::new(),
                    });
                }
                if ui.button("検証").clicked() {
                    want_validate = true;
                }
                if ui.button("オフセット自動整列").clicked() {
                    let mut off = 0usize;
                    for f in fields.iter_mut() {
                        f.offset = off;
                        off += f.length;
                    }
                    auto_align_end = Some(off);
                }
            });
        } else {
            ui.weak("(バリアントを追加・選択するとフィールドを編集できます)");
        }
    }

    if let Some(end) = auto_align_end {
        schema.record_length = schema.record_length.max(end);
        *status = "オフセットを詰めて整列しました".into();
    }
    if want_validate {
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
    if let Some(err) = &state.validation_error {
        ui.colored_label(crate::theme::ERR, err);
    }
}

fn draw_schema_meta(ui: &mut egui::Ui, schema: &mut Schema) {
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
}

fn draw_discriminator_section(
    ui: &mut egui::Ui,
    schema: &mut Schema,
    state: &mut SchemaEditorState,
) {
    let was_multi = schema.discriminator.is_some();
    let mut want_multi = was_multi;
    ui.checkbox(
        &mut want_multi,
        "マルチバリアント (レコード種別ごとにレイアウト分岐)",
    );
    if want_multi != was_multi {
        if want_multi {
            schema.discriminator = Some(Discriminator {
                offset: 0,
                length: 1,
                encoding: Encoding::Ascii,
            });
            if schema.variants.is_empty() {
                schema.variants.push(Variant {
                    key: "1".into(),
                    name: "バリアント1".into(),
                    description: String::new(),
                    fields: Vec::new(),
                });
            }
            state.active_variant = Some(0);
            state.selected_field = None;
        } else {
            schema.discriminator = None;
            state.active_variant = None;
            state.selected_field = None;
        }
    }

    if let Some(disc) = schema.discriminator.as_mut() {
        ui.indent("disc", |ui| {
            ui.horizontal(|ui| {
                ui.label("判別オフセット:");
                let mut off = disc.offset as i64;
                if ui
                    .add(egui::DragValue::new(&mut off).range(0..=65535))
                    .changed()
                {
                    disc.offset = off.max(0) as usize;
                }
                ui.label("長さ:");
                let mut l = disc.length as i64;
                if ui
                    .add(egui::DragValue::new(&mut l).range(1..=16))
                    .changed()
                {
                    disc.length = l.max(1) as usize;
                }
                ui.label("エンコ:");
                egui::ComboBox::from_id_salt("disc_enc")
                    .selected_text(disc.encoding.to_string())
                    .show_ui(ui, |ui| {
                        for e in Encoding::all() {
                            ui.selectable_value(&mut disc.encoding, e, e.to_string());
                        }
                    });
            });
        });
    }
}

fn draw_variant_picker(
    ui: &mut egui::Ui,
    schema: &mut Schema,
    state: &mut SchemaEditorState,
) {
    let mut remove_variant: Option<usize> = None;

    ui.horizontal_wrapped(|ui| {
        ui.label("バリアント:");
        for (i, v) in schema.variants.iter().enumerate() {
            let label = format!(
                "[{}] {}",
                v.key,
                if v.name.is_empty() { "(無名)" } else { v.name.as_str() }
            );
            let selected = state.active_variant == Some(i);
            if ui.selectable_label(selected, label).clicked() {
                state.active_variant = Some(i);
                state.selected_field = None;
            }
        }
        if ui.small_button("+ 追加").clicked() {
            let next_idx = schema.variants.len();
            schema.variants.push(Variant {
                key: format!("{}", next_idx + 1),
                name: format!("バリアント{}", next_idx + 1),
                description: String::new(),
                fields: Vec::new(),
            });
            state.active_variant = Some(next_idx);
            state.selected_field = None;
        }
        if let Some(idx) = state.active_variant {
            if idx < schema.variants.len() && ui.small_button("− 削除").clicked() {
                remove_variant = Some(idx);
            }
        }
    });

    if let Some(idx) = remove_variant {
        schema.variants.remove(idx);
        state.active_variant = if schema.variants.is_empty() {
            None
        } else {
            Some(idx.min(schema.variants.len() - 1))
        };
        state.selected_field = None;
    }

    // Normalize: if multi-variant and active is unset / out of range, pick first.
    if let Some(idx) = state.active_variant {
        if idx >= schema.variants.len() {
            state.active_variant = if schema.variants.is_empty() {
                None
            } else {
                Some(0)
            };
        }
    } else if !schema.variants.is_empty() {
        state.active_variant = Some(0);
    }

    if let Some(idx) = state.active_variant {
        if let Some(v) = schema.variants.get_mut(idx) {
            ui.indent("variant_meta", |ui| {
                egui::Grid::new(("variant_meta_grid", idx))
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("キー:");
                        ui.text_edit_singleline(&mut v.key);
                        ui.end_row();
                        ui.label("名前:");
                        ui.text_edit_singleline(&mut v.name);
                        ui.end_row();
                        ui.label("説明:");
                        ui.text_edit_singleline(&mut v.description);
                        ui.end_row();
                    });
            });
        }
    }
}

fn draw_field_list(ui: &mut egui::Ui, fields: &mut Vec<Field>, selected: &mut Option<usize>) {
    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 100.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut remove_idx: Option<usize> = None;
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;

            for i in 0..fields.len() {
                let is_sel = *selected == Some(i);
                let header = format!(
                    "{:>2}: {} (オフセット={}, 長さ={})",
                    i + 1,
                    fields[i].name,
                    fields[i].offset,
                    fields[i].length
                );
                let resp = egui::CollapsingHeader::new(header)
                    .id_salt(("field_hdr", i))
                    .default_open(is_sel)
                    .show(ui, |ui| {
                        draw_field_editor(ui, &mut fields[i], i);
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
                    *selected = Some(i);
                }
            }

            if let Some(i) = remove_idx {
                fields.remove(i);
                *selected = None;
            }
            if let Some(i) = move_up {
                if i > 0 {
                    fields.swap(i, i - 1);
                }
            }
            if let Some(i) = move_down {
                if i + 1 < fields.len() {
                    fields.swap(i, i + 1);
                }
            }
        });
}

fn draw_field_editor(ui: &mut egui::Ui, field: &mut Field, idx: usize) {
    egui::Grid::new(("field_grid", idx))
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
                egui::ComboBox::from_id_salt(("kind_combo", idx))
                    .selected_text(kind_label_jp(cur))
                    .show_ui(ui, |ui| {
                        for t in ["text", "numeric", "decimal", "date", "bytes", "filler"] {
                            if ui.selectable_label(cur == t, kind_label_jp(t)).clicked() && cur != t {
                                field.kind = default_kind_for(t);
                            }
                        }
                    });
                draw_kind_params(ui, &mut field.kind, idx);
            });
            ui.end_row();

            ui.label("エンコーディング:");
            ui.horizontal(|ui| {
                let mut use_default = field.encoding.is_none();
                if ui.checkbox(&mut use_default, "(既定値を使用)").changed() {
                    field.encoding = if use_default { None } else { Some(Encoding::ShiftJis) };
                }
                if let Some(enc) = field.encoding.as_mut() {
                    egui::ComboBox::from_id_salt(("enc_combo", idx))
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

fn draw_kind_params(ui: &mut egui::Ui, kind: &mut FieldKind, idx: usize) {
    match kind {
        FieldKind::Text { pad } | FieldKind::Filler { pad } => {
            pad_byte_editor(ui, pad, idx);
        }
        FieldKind::Numeric { pad, signed } => {
            pad_byte_editor(ui, pad, idx);
            ui.checkbox(signed, "符号付き");
        }
        FieldKind::Decimal {
            pad,
            scale,
            signed,
        } => {
            pad_byte_editor(ui, pad, idx);
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

fn pad_byte_editor(ui: &mut egui::Ui, pad: &mut u8, idx: usize) {
    ui.label("詰め文字:");
    let mut s = format!("{:#04X}", pad);
    if ui
        .add(
            egui::TextEdit::singleline(&mut s)
                .id_salt(("pad_edit", idx))
                .desired_width(50.0),
        )
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
