use crate::record::RecordBuffer;
use crate::schema::Schema;
use eframe::egui;
use egui::{Color32, RichText, Sense, Stroke};

const BYTES_PER_ROW: usize = 16;

/// Draw the hex+ASCII dump of the currently selected record.
pub fn draw(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    highlighted_field: &mut Option<usize>,
    status: &mut String,
) {
    ui.horizontal(|ui| {
        crate::theme::section_heading(ui, "16進ダンプ");
        ui.separator();
        let count = buffer.record_count(schema);
        if count == 0 {
            ui.label("(レコードがありません)");
            return;
        }
        let mut cur = buffer.current_record;
        ui.label("レコード:");
        let cur_disp = cur + 1;
        let mut cur_i64 = cur_disp as i64;
        if ui
            .add(
                egui::DragValue::new(&mut cur_i64)
                    .range(1..=count as i64)
                    .speed(0.2),
            )
            .changed()
        {
            cur = (cur_i64 - 1).clamp(0, count as i64 - 1) as usize;
            buffer.current_record = cur;
        }
        ui.label(format!("/ {count}"));

        ui.separator();
        if ui.button("◀ 前").clicked() && buffer.current_record > 0 {
            buffer.current_record -= 1;
        }
        if ui.button("次 ▶").clicked() && buffer.current_record + 1 < count {
            buffer.current_record += 1;
        }

        ui.separator();
        ui.label(format!("カーソル位置: オフセット {:#06X}", buffer.cursor_in_record));
    });

    ui.separator();

    let record_idx = buffer.current_record;
    let Some(record) = buffer.record_slice(schema, record_idx).map(|s| s.to_vec()) else {
        ui.label("(範囲外のレコード — [レコード] メニューから追加してください)");
        return;
    };

    let row_count = (schema.record_length + BYTES_PER_ROW - 1) / BYTES_PER_ROW;

    // Resolve which variant (if any) this record belongs to, and snapshot its
    // field list so we can colour and label by the right layout.
    let fields_snapshot: Vec<crate::schema::Field> = schema.fields_for(&record).to_vec();
    if let Some(variant) = schema.variant_for(&record) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("バリアント: [{}] {}", variant.key, variant.name))
                    .color(crate::theme::ACCENT)
                    .strong(),
            );
            if !variant.description.is_empty() {
                ui.weak(&variant.description);
            }
        });
        ui.separator();
    } else if schema.is_multi_variant() {
        ui.colored_label(
            crate::theme::WARN,
            "⚠ どのバリアントにも一致しないレコードです",
        );
        ui.separator();
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Header row
            ui.horizontal(|ui| {
                ui.monospace(format!("{:>8}", "オフセット"));
                ui.add_space(8.0);
                for col in 0..BYTES_PER_ROW {
                    ui.monospace(format!("{:02X}", col));
                }
                ui.add_space(8.0);
                ui.monospace("ASCII");
            });
            ui.separator();

            for row in 0..row_count {
                let row_start = row * BYTES_PER_ROW;
                let row_end = (row_start + BYTES_PER_ROW).min(schema.record_length);

                ui.horizontal(|ui| {
                    ui.monospace(format!("{:08X}", row_start));
                    ui.add_space(8.0);

                    // Hex bytes
                    for col in 0..BYTES_PER_ROW {
                        let off = row_start + col;
                        if off >= row_end {
                            ui.monospace("  ");
                            continue;
                        }
                        let byte = record[off];
                        let field_idx = field_index_at(&fields_snapshot, off);
                        let highlighted = *highlighted_field == field_idx && field_idx.is_some();
                        let is_cursor = buffer.cursor_in_record == off;

                        let mut text = RichText::new(format!("{:02X}", byte)).monospace();
                        if let Some(fi) = field_idx {
                            let color = field_color(fi);
                            text = text.color(color);
                        }
                        if highlighted {
                            text = text.background_color(crate::theme::ACCENT_SOFT);
                        }
                        if is_cursor {
                            text = text
                                .strong()
                                .background_color(crate::theme::ACCENT_DIM);
                        }

                        let resp = ui.add(egui::Label::new(text).sense(Sense::click()));
                        if resp.clicked() {
                            buffer.cursor_in_record = off;
                            *highlighted_field = field_idx;
                            *status = match field_idx {
                                Some(fi) => format!(
                                    "オフセット {:#06X} → フィールド '{}'",
                                    off, fields_snapshot[fi].name
                                ),
                                None => format!("オフセット {:#06X} (フィールド未割当)", off),
                            };
                        }
                        if resp.hovered() {
                            if let Some(fi) = field_idx {
                                resp.on_hover_text(format!(
                                    "{}\nオフセット={}, 長さ={}",
                                    fields_snapshot[fi].name,
                                    fields_snapshot[fi].offset,
                                    fields_snapshot[fi].length
                                ));
                            }
                        }
                    }
                    ui.add_space(8.0);

                    // ASCII column
                    let mut ascii = String::new();
                    for off in row_start..row_end {
                        let b = record[off];
                        ascii.push(if (0x20..0x7f).contains(&b) {
                            b as char
                        } else {
                            '·'
                        });
                    }
                    ui.monospace(ascii);
                });
            }

            // Decoded representation (current encoding) below the dump.
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("デコード結果:").strong());
                let decoded = schema.default_encoding.decode(&record);
                ui.monospace(decoded);
            });

            // Inline byte-editor at cursor.
            ui.add_space(8.0);
            ui.separator();
            byte_editor(ui, schema, buffer, status);
        });

    // Draw a thin border around the central panel for visual separation
    let _ = Stroke::new(1.0, Color32::DARK_GRAY);
}

fn byte_editor(
    ui: &mut egui::Ui,
    schema: &Schema,
    buffer: &mut RecordBuffer,
    status: &mut String,
) {
    let off = buffer.cursor_in_record;
    if off >= schema.record_length {
        return;
    }
    let abs_off = buffer.record_start(schema, buffer.current_record) + off;
    let cur_byte = buffer.data.as_slice().get(abs_off).copied().unwrap_or(0);

    ui.horizontal(|ui| {
        ui.label(format!("バイト編集 @ オフセット {:#06X}:", off));

        ui.label("16進:");
        let mut hex_str = format!("{:02X}", cur_byte);
        let hex_resp = ui.add(
            egui::TextEdit::singleline(&mut hex_str)
                .desired_width(40.0)
                .font(egui::TextStyle::Monospace),
        );
        if hex_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            match u8::from_str_radix(hex_str.trim(), 16) {
                Ok(b) => match buffer.set_byte(abs_off, b) {
                    Ok(()) => {
                        *status = format!("オフセット {:#06X} に {:#04X} を書き込みました", off, b)
                    }
                    Err(e) => *status = format!("書き込み失敗: {e:#}"),
                },
                Err(_) => *status = "不正な16進バイト".into(),
            }
        }

        ui.label("10進:");
        let mut dec_str = format!("{}", cur_byte);
        let dec_resp = ui.add(egui::TextEdit::singleline(&mut dec_str).desired_width(40.0));
        if dec_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            match dec_str.trim().parse::<u8>() {
                Ok(b) => {
                    let _ = buffer.set_byte(abs_off, b);
                    *status = format!("オフセット {:#06X} に {} を書き込みました", off, b);
                }
                Err(_) => *status = "不正な10進バイト".into(),
            }
        }

        ui.label("文字:");
        let printable_char = if (0x20..0x7f).contains(&cur_byte) {
            (cur_byte as char).to_string()
        } else {
            "·".to_string()
        };
        let mut ch_str = printable_char;
        let ch_resp = ui.add(egui::TextEdit::singleline(&mut ch_str).desired_width(30.0));
        if ch_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(c) = ch_str.chars().next() {
                if (c as u32) < 0x80 {
                    let _ = buffer.set_byte(abs_off, c as u8);
                    *status = format!("オフセット {:#06X} に '{}' を書き込みました", off, c);
                } else {
                    *status = "ここではASCII文字のみ入力できます".into();
                }
            }
        }
    });
}

fn field_index_at(fields: &[crate::schema::Field], offset: usize) -> Option<usize> {
    for (i, f) in fields.iter().enumerate() {
        if offset >= f.offset && offset < f.offset + f.length {
            return Some(i);
        }
    }
    None
}

/// Stable color per field index for visual segmentation in the hex dump.
/// Chosen as a desaturated rotation that stays legible against the dark
/// theme without clashing with the teal accent.
fn field_color(idx: usize) -> Color32 {
    const PALETTE: &[Color32] = &[
        Color32::from_rgb(0xE8, 0xB7, 0x8A), // peach
        Color32::from_rgb(0x9C, 0xC8, 0xE2), // sky
        Color32::from_rgb(0xB4, 0xD4, 0x96), // sage
        Color32::from_rgb(0xE5, 0xB1, 0xC9), // dusty pink
        Color32::from_rgb(0xE3, 0xCC, 0x82), // mustard
        Color32::from_rgb(0xAE, 0xB6, 0xE0), // lavender
        Color32::from_rgb(0xDF, 0xA5, 0xA5), // brick
        Color32::from_rgb(0xA2, 0xD4, 0xBE), // mint
        Color32::from_rgb(0xC9, 0xB0, 0xDC), // lilac
        Color32::from_rgb(0xE3, 0xC4, 0x94), // sand
    ];
    PALETTE[idx % PALETTE.len()]
}
