# FLR Editor

固定長レコード (Fixed-Length Record) ファイルを編集するための GUI エディタ。
全銀協フォーマットなどの金融データを想定して設計されており、Shift_JIS / UTF-8 / ASCII の混在、
レコード種別ごとに異なるフィールドレイアウト（マルチバリアント）、1ファイル数百万件規模の高速表示に対応します。

- 言語: Rust (edition 2021)
- GUI: [egui](https://github.com/emilk/egui) + [eframe](https://docs.rs/eframe) (wgpu バックエンド)
- 対応OS: macOS / Linux / Windows

---

## 主な機能

- **詳細ビュー (16進ダンプ)**: 1レコードを16進＋ASCIIで表示。フィールド境界をハイライト
- **スプレッドシートビュー**: 全レコードを行・全フィールドを列で一覧表示・編集
- **マルチバリアントスキーマ**: 先頭バイトの「レコード区分」でフィールドレイアウトを動的切替（全銀協 1/2/8/9 など）
- **検索 / フィルタ**: フィールド単位の絞り込み、複数条件の AND 結合
- **エンコーディング**: Shift_JIS / UTF-8 / ASCII。スキーマ既定値とフィールド単位の上書きの両方
- **大容量対応**: `memmap2` でメモリマップ読み込み、`rayon` で並列処理
- **スキーマエディタ**: フィールドの追加・編集・並び替えを GUI で行い TOML/JSON に保存

---

## クイックスタート

### 必要なもの

- Rust toolchain (rustup 推奨, 1.75以降)
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  # または Homebrew:
  brew install rustup && rustup-init
  ```
- OS別の追加依存:
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`)
  - **Windows**: Visual Studio Build Tools (C++ ワークロード)
  - **Linux**: `libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev`

### ビルド・実行

```sh
git clone git@github.com:KyosukeIshizu1008/FLREditor.git
cd FLREditor
cargo run --release
```

初回ビルドは `eframe` / `wgpu` のコンパイルで数分〜10分程度かかります。

### 起動直後の挙動

引数なしで起動すると、組み込みの「120バイト・サンプルスキーマ」を読み込んで空の1レコードを表示します。
「ファイル」メニューから既存のデータファイルやスキーマを開けます。

---

## 使い方

### メニュー

| メニュー | 項目 | 説明 |
|---|---|---|
| ファイル | データを開く… | 固定長データファイルを読み込み |
| ファイル | 上書き保存 / 名前を付けて保存… | 編集結果を書き出し |
| ファイル | スキーマを開く… | TOML / JSON のスキーマを読み込み |
| ファイル | スキーマを保存… | 現在のスキーマを書き出し |
| レコード | 新規レコードを追加 | 末尾に1レコード追加（パディング値で初期化） |
| レコード | 現在のレコードを削除 | 選択中のレコードを削除 |
| 表示 | スキーマエディタ | 右サイドパネルでフィールドを編集 |
| 表示 | 詳細ビュー / スプレッドシート | 中央パネルの表示方式を切替 |

### 詳細ビュー

- 16進ダンプ・ASCII列・フィールド表が連動。どこをクリックしても対応箇所がハイライト
- 表のセルを編集すると、エンコード結果がそのままバイト列に反映

### スプレッドシートビュー

- 行 = レコード、列 = フィールド
- マルチバリアントスキーマでは、レコードごとに異なる列構成になり、列ヘッダはバリアント別に動的に切り替わる
- セルクリックで編集、複数条件フィルタ、フィールド指定検索

---

## スキーマ仕様

スキーマは `schemas/*.toml`（または `.json`）で定義します。拡張子で自動判別。
詳細は [`src/schema.rs`](src/schema.rs) を参照。

### トップレベル

| キー | 必須 | 説明 |
|---|---|---|
| `name` | ✓ | スキーマ名 |
| `record_length` | ✓ | レコード長 (バイト) |
| `default_encoding` | | `shift_jis` / `utf8` / `ascii` (デフォ `shift_jis`) |
| `record_separator` | | `none` / `lf` / `crlf` (デフォ `none`) |
| `fields` | △ | 単一レイアウトの場合のフィールド配列 |
| `discriminator` | △ | マルチバリアント時のレコード判別位置 |
| `variants` | △ | レコード種別ごとのフィールド配列 |

### フィールド (`[[fields]]` または `[[variants.fields]]`)

```toml
name = "amount"
offset = 54         # レコード先頭からのバイト位置
length = 10
kind = "decimal"    # text / numeric / decimal / date / bytes / filler
pad = 0x30          # text=0x20, numeric/decimal=0x30 がデフォルト
scale = 0           # decimal 専用 (固定小数の桁数)
signed = false      # numeric / decimal 専用 (先頭 +/- 許容)
format = "YYYYMMDD" # date 専用 (Y/M/D プレースホルダ + リテラル)
encoding = "ascii"  # 省略時はスキーマの default_encoding
description = "振込金額 (円)"
```

### `kind` の種類

| kind | 用途 | パラメータ |
|---|---|---|
| `text` | 任意文字列 (右パディング) | `pad` |
| `numeric` | 数字文字列 (左ゼロパディング) | `pad`, `signed` |
| `decimal` | 固定小数 ("0000012345" + scale=2 → 123.45) | `pad`, `scale`, `signed` |
| `date` | 日付 (フォーマット文字列で書式指定) | `format` |
| `bytes` | 不透明バイト (HEX表示のみ) | — |
| `filler` | 予備領域 | `pad` |

### マルチバリアント

レコード先頭の数バイトでフィールドレイアウトを切り替えます。

```toml
[discriminator]
offset = 0      # レコード先頭からのバイト位置
length = 1      # 何バイト読むか
encoding = "ascii"

[[variants]]
key = "1"
name = "ヘッダレコード"
[[variants.fields]]
name = "レコード区分"
offset = 0
length = 1
kind = "text"
encoding = "ascii"
# …続く…

[[variants]]
key = "2"
name = "データレコード"
[[variants.fields]]
# …別のレイアウト…
```

判別優先順:
1. discriminator のバイト列に一致する `variants[*].key` があればそのレイアウト
2. 一致しなければトップレベル `[[fields]]` にフォールバック
3. それも無ければ最初の variant のレイアウト

### バリデーション

ロード時に以下を自動チェックし、不正なら起動失敗・エラー表示。

- `record_length > 0`
- 各フィールド `length > 0`
- `offset + length ≤ record_length`
- 同じバリアント内のフィールド重複（前フィールドの末尾 > 次フィールドの先頭）
- discriminator の範囲チェック・バリアントキーの重複

---

## サンプル

`schemas/` および `samples/` に同梱:

- `schemas/sample_120.toml` — 単一レイアウト、120バイト振込フォーマット
- `schemas/zengin_120_multi.toml` — 全銀協 120バイト4種混在 (ヘッダ/データ/トレーラ/エンド)
- `samples/sample_120.dat` — `sample_120.toml` 用の正常データ
- `samples/sample_120_broken.dat` — エラーハンドリング確認用の壊れたデータ
- `samples/varied_1000.dat` — 1,000件サンプル
- `samples/varied_100k.dat` — 100,000件サンプル

### 大容量サンプルの再生成

`samples/big_1M.dat` と `samples/varied_1M.dat`（各 約114MB）は GitHub の100MB制限のためリポジトリには含まれていません。
必要なら下記スクリプトで生成できます。

```sh
cd samples
python3 gen_varied.py   # varied_*.dat 系
./make_sample.sh        # sample_120 系
```

---

## プロジェクト構成

```
src/
├── main.rs             # エントリポイント / CJKフォント設定
├── app.rs              # FlrApp（最上位状態）/ メニュー / 全体レイアウト
├── encoding.rs         # Shift_JIS / UTF-8 / ASCII の encode/decode
├── schema.rs           # Schema / Field / Variant / Discriminator + バリデーション
├── record.rs           # RecordBuffer（mmap or owned）+ フィールド読み書き
├── theme.rs            # egui テーマ・カラーパレット
└── ui/
    ├── hex_view.rs         # 詳細ビュー（16進ダンプ + フィールド表）
    ├── spreadsheet_view.rs # 全レコード一覧 + インライン編集
    ├── table_view.rs       # 詳細ビュー右側のフィールド表
    ├── schema_editor.rs    # スキーマ編集サイドパネル
    ├── search_bar.rs       # 検索バー（フィールド絞り込み対応）
    ├── filter.rs           # 複数条件フィルタ（AND）
    └── mod.rs

schemas/                # 同梱スキーマ
samples/                # 同梱データ
```

### 主要なデータフロー

1. `main.rs` が `FlrApp::new()` を起動 → 既定の `Schema::sample_120()` をロード
2. ユーザーがデータファイルを開く → `RecordBuffer::load_from_path` で mmap or 読み込み
3. ビューは `Schema::fields_for(record_bytes)` で動的にレイアウトを取得
4. 編集は `RecordBuffer::set_field_text` 経由でエンコードして書き込み
5. 保存は元ファイルが mmap の場合は `make_owned` でコピーオンライトしてから書き出し

---

## 開発

### テスト

```sh
cargo test
```

`schema.rs` にマルチバリアント判別のユニットテストあり。

### デバッグログ

```sh
RUST_LOG=debug cargo run
```

`env_logger` を使用。`info` / `warn` / `error` も同様。

### リリースビルド

```sh
cargo build --release
# 成果物: target/release/flr-editor (Windowsは .exe)
```

`Cargo.toml` の `[profile.release]` で `opt-level = 3`, `lto = "thin"` 指定済み。

---

## 既知の制限

- レコード長がバリアントごとに異なる可変長フォーマットは未対応（全レコードが同一の `record_length` 固定）
- 編集中はファイル全体をメモリに展開（mmap → owned コピー）。極端な巨大ファイル編集は要メモリ容量に注意
- フィールド重複（オーバーラップ）禁止。複数フィールドで同じバイト範囲を解釈する用途は別途対応が必要

---

## ライセンス

未定。
