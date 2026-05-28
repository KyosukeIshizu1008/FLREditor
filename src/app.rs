use crate::encoding::Encoding;
use crate::record::RecordBuffer;
use crate::schema::Schema;
use crate::ui::{filter, hex_view, schema_editor, search_bar, spreadsheet_view, table_view};
use eframe::egui;

/// Built-in sample schemas, embedded at compile time so they're available
/// no matter where the binary is launched from. (name, description, toml).
const BUILTIN_SCHEMAS: &[(&str, &str, &str)] = &[
    (
        "sample_120",
        "120バイト 振込フォーマット (単一)",
        include_str!("../schemas/sample_120.toml"),
    ),
    (
        "zengin_120_multi",
        "全銀協 120バイト (ヘッダ/データ/トレーラ/エンドの4種混在)",
        include_str!("../schemas/zengin_120_multi.toml"),
    ),
    (
        "product_80",
        "商品マスタ 80バイト (単一)",
        include_str!("../schemas/product_80.toml"),
    ),
    (
        "pos_100_multi",
        "POS取引 100バイト (H/D/T)",
        include_str!("../schemas/pos_100_multi.toml"),
    ),
    (
        "employee_120",
        "従業員 120バイト (全 kind 網羅)",
        include_str!("../schemas/employee_120.toml"),
    ),
];

/// Which large view occupies the central panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Hex dump + ASCII view of the currently selected record.
    Detail,
    /// All records as rows, all fields as columns.
    Spreadsheet,
}

/// Top-level application state.
pub struct FlrApp {
    pub schema: Schema,
    pub buffer: RecordBuffer,

    /// Pending status message shown briefly in the status bar.
    pub status: String,

    /// State for the schema editor side panel.
    pub schema_editor_open: bool,
    pub schema_editor: schema_editor::SchemaEditorState,

    /// State for the search bar.
    pub search: search_bar::SearchState,

    /// Field index currently highlighted (clicked in either pane).
    pub highlighted_field: Option<usize>,

    /// Inline editing state for the table view.
    pub table_edit: Option<TableEditState>,

    /// Which large view the central panel shows.
    pub view_mode: ViewMode,

    /// State for the spreadsheet view (edit cell, last scrolled row).
    pub spreadsheet: spreadsheet_view::SpreadsheetState,

    /// Active filter (multi-condition AND'd) applied to the spreadsheet.
    pub filter: filter::FilterState,

    /// Whether the bottom "フィールド分解" panel is shown.
    pub table_panel_open: bool,
}

pub struct TableEditState {
    pub field_idx: usize,
    pub draft: String,
    pub error: Option<String>,
}

impl FlrApp {
    pub fn new() -> Self {
        let schema = Schema::sample_120();
        let buffer = RecordBuffer::new_empty(&schema, 1);
        Self {
            schema,
            buffer,
            status: "準備完了。120バイト・サンプルスキーマを読み込みました。".into(),
            schema_editor_open: false,
            schema_editor: schema_editor::SchemaEditorState::default(),
            search: search_bar::SearchState::default(),
            highlighted_field: None,
            table_edit: None,
            view_mode: ViewMode::Detail,
            spreadsheet: spreadsheet_view::SpreadsheetState::default(),
            filter: filter::FilterState::default(),
            table_panel_open: true,
        }
    }

    fn open_data_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("固定長データファイルを開く")
            .pick_file()
        {
            match RecordBuffer::load_from_path(&path, &self.schema) {
                Ok(buf) => {
                    let count = buf.record_count(&self.schema);
                    self.buffer = buf;
                    self.filter.clear();
                    self.status =
                        format!("{} を読み込みました ({} レコード)", path.display(), count);
                }
                Err(e) => self.status = format!("読み込み失敗: {e:#}"),
            }
        }
    }

    fn save_data_file(&mut self, save_as: bool) {
        let path = if save_as || self.buffer.path.is_none() {
            rfd::FileDialog::new()
                .set_title("固定長データファイルを保存")
                .save_file()
        } else {
            self.buffer.path.clone()
        };
        if let Some(path) = path {
            match self.buffer.save_to_path(&path, &self.schema) {
                Ok(()) => self.status = format!("{} に保存しました", path.display()),
                Err(e) => self.status = format!("保存失敗: {e:#}"),
            }
        }
    }

    fn open_schema_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("スキーマ", &["toml", "json"])
            .set_title("スキーマファイルを開く")
            .pick_file()
        {
            match Schema::load_from_path(&path) {
                Ok(s) => {
                    let stride_changed = s.stride() != self.schema.stride();
                    self.schema = s;
                    self.filter.invalidate();
                    if stride_changed {
                        self.buffer = RecordBuffer::new_empty(&self.schema, 1);
                        self.status = format!(
                            "スキーマ '{}' を読み込みました (レコード長={})。データバッファをリセットしました。",
                            self.schema.name, self.schema.record_length
                        );
                    } else {
                        self.status =
                            format!("スキーマ '{}' を読み込みました。", self.schema.name);
                    }
                }
                Err(e) => self.status = format!("スキーマ読み込み失敗: {e:#}"),
            }
        }
    }

    fn save_schema_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("スキーマ (TOML)", &["toml"])
            .add_filter("スキーマ (JSON)", &["json"])
            .set_title("スキーマを保存")
            .save_file()
        {
            match self.schema.save_to_path(&path) {
                Ok(()) => self.status = format!("スキーマを {} に保存しました", path.display()),
                Err(e) => self.status = format!("スキーマ保存失敗: {e:#}"),
            }
        }
    }
}

impl eframe::App for FlrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw_menu_bar(ctx);
        self.draw_status_bar(ctx);

        if self.schema_editor_open {
            egui::SidePanel::right("schema_editor")
                .resizable(true)
                .default_width(420.0)
                .show(ctx, |ui| {
                    schema_editor::draw(
                        ui,
                        &mut self.schema,
                        &mut self.schema_editor,
                        &mut self.status,
                    );
                });
        }

        egui::TopBottomPanel::top("search_bar").show(ctx, |ui| {
            search_bar::draw(
                ui,
                &mut self.search,
                &mut self.buffer,
                &self.schema,
                &mut self.status,
                &mut self.highlighted_field,
            );
        });

        if self.table_panel_open {
            egui::TopBottomPanel::bottom("table_view")
                .resizable(true)
                .default_height(320.0)
                .min_height(0.0)
                .show(ctx, |ui| {
                    // Wrap in a ScrollArea so the panel can be shrunk below
                    // the content's natural size — otherwise the table's
                    // intrinsic min_height pushes back against the resize
                    // handle.
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            table_view::draw(
                                ui,
                                &self.schema,
                                &mut self.buffer,
                                &mut self.highlighted_field,
                                &mut self.table_edit,
                                &mut self.status,
                            );
                        });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.view_mode {
            ViewMode::Detail => hex_view::draw(
                ui,
                &self.schema,
                &mut self.buffer,
                &mut self.highlighted_field,
                &mut self.status,
            ),
            ViewMode::Spreadsheet => spreadsheet_view::draw(
                ui,
                &self.schema,
                &mut self.buffer,
                &mut self.highlighted_field,
                &mut self.spreadsheet,
                &mut self.filter,
                &mut self.status,
            ),
        });
    }
}

impl FlrApp {
    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("ファイル", |ui| {
                    if ui.button("データを開く…").clicked() {
                        ui.close_menu();
                        self.open_data_file();
                    }
                    if ui.button("上書き保存").clicked() {
                        ui.close_menu();
                        self.save_data_file(false);
                    }
                    if ui.button("名前を付けて保存…").clicked() {
                        ui.close_menu();
                        self.save_data_file(true);
                    }
                    ui.separator();
                    if ui.button("スキーマを開く…").clicked() {
                        ui.close_menu();
                        self.open_schema_file();
                    }
                    ui.menu_button("サンプルスキーマを使う", |ui| {
                        let entries: Vec<(&str, &str, &str)> = BUILTIN_SCHEMAS.to_vec();
                        for (name, desc, text) in entries {
                            let label = format!("{} — {}", name, desc);
                            if ui.button(label).clicked() {
                                ui.close_menu();
                                match Schema::from_toml_str(text) {
                                    Ok(s) => {
                                        let stride_changed = s.stride() != self.schema.stride();
                                        self.schema = s;
                                        self.filter.invalidate();
                                        if stride_changed {
                                            self.buffer =
                                                RecordBuffer::new_empty(&self.schema, 1);
                                        }
                                        self.status = format!(
                                            "サンプル '{}' を読み込みました",
                                            self.schema.name
                                        );
                                    }
                                    Err(e) => {
                                        self.status = format!("スキーマ解析失敗: {e:#}");
                                    }
                                }
                            }
                        }
                    });
                    if ui.button("スキーマを保存…").clicked() {
                        ui.close_menu();
                        self.save_schema_file();
                    }
                });
                ui.menu_button("レコード", |ui| {
                    if self.schema.is_multi_variant() {
                        ui.menu_button("新規レコードを追加…", |ui| {
                            let variants: Vec<(String, String)> = self
                                .schema
                                .variants
                                .iter()
                                .map(|v| (v.key.clone(), v.name.clone()))
                                .collect();
                            for (key, name) in variants {
                                let label = if name.is_empty() {
                                    format!("[{}]", key)
                                } else {
                                    format!("[{}] {}", key, name)
                                };
                                if ui.button(label).clicked() {
                                    ui.close_menu();
                                    let idx = self
                                        .buffer
                                        .append_record(&self.schema, Some(&key));
                                    self.buffer.current_record = idx;
                                    self.filter.invalidate();
                                    self.status = format!(
                                        "レコード {} (バリアント [{}]) を追加しました",
                                        idx + 1,
                                        key
                                    );
                                }
                            }
                        });
                    } else if ui.button("新規レコードを追加").clicked() {
                        ui.close_menu();
                        let idx = self.buffer.append_record(&self.schema, None);
                        self.buffer.current_record = idx;
                        self.filter.invalidate();
                        self.status = format!("レコード {} を追加しました", idx + 1);
                    }
                    if ui.button("現在のレコードを削除").clicked() {
                        ui.close_menu();
                        let idx = self.buffer.current_record;
                        if let Err(e) = self.buffer.delete_record(&self.schema, idx) {
                            self.status = format!("削除失敗: {e:#}");
                        } else {
                            self.filter.invalidate();
                            self.status = format!("レコード {} を削除しました", idx + 1);
                        }
                    }
                });
                ui.menu_button("表示", |ui| {
                    ui.checkbox(&mut self.schema_editor_open, "スキーマエディタ");
                    ui.checkbox(&mut self.table_panel_open, "フィールド分解パネル");
                    ui.separator();
                    ui.radio_value(&mut self.view_mode, ViewMode::Detail, "詳細ビュー (16進ダンプ)");
                    ui.radio_value(
                        &mut self.view_mode,
                        ViewMode::Spreadsheet,
                        "スプレッドシート (全レコード)",
                    );
                });

                ui.separator();
                ui.label(format!(
                    "スキーマ: {} ({} バイト/レコード, 既定={})",
                    self.schema.name, self.schema.record_length, self.schema.default_encoding
                ));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let count = self.buffer.record_count(&self.schema);
                    let cur = self.buffer.current_record;
                    ui.label(format!(
                        "レコード {}/{}",
                        if count == 0 { 0 } else { cur + 1 },
                        count
                    ));
                    if self.buffer.modified {
                        ui.colored_label(crate::theme::WARN, "● 未保存");
                    }
                });
            });
        });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let enc = self.schema.default_encoding;
                    let label = match enc {
                        Encoding::ShiftJis => "Shift_JIS",
                        Encoding::Utf8 => "UTF-8",
                        Encoding::Ascii => "ASCII",
                    };
                    ui.label(format!("既定エンコーディング: {label}"));
                });
            });
        });
    }
}
