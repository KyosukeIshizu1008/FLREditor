#!/usr/bin/env python3
"""
Generate demo sample data for the 3 new schemas:

  schemas/product_80.toml      -> samples/product_80.dat
  schemas/pos_100_multi.toml   -> samples/pos_100.dat
  schemas/employee_120.toml    -> samples/employee_120.dat

Usage:
  python3 samples/gen_demo_samples.py
"""
import os
import random
import struct


# ---------- byte helpers ----------

def pad_right_sjis(s: str, n: int, fill: bytes = b" ") -> bytes:
    """Encode `s` in CP932 and right-pad to exactly `n` bytes.
    Truncates at the last full character so we never emit a half-byte
    multi-byte sequence."""
    out = bytearray()
    for ch in s:
        eb = ch.encode("cp932", errors="replace")
        if len(out) + len(eb) > n:
            break
        out += eb
    return bytes(out) + fill * (n - len(out))


def pad_right_utf8(s: str, n: int, fill: bytes = b" ") -> bytes:
    eb = s.encode("utf-8", errors="replace")
    if len(eb) > n:
        # Re-encode char by char to truncate at a char boundary.
        out = bytearray()
        for ch in s:
            cb = ch.encode("utf-8")
            if len(out) + len(cb) > n:
                break
            out += cb
        eb = bytes(out)
    return eb + fill * (n - len(eb))


def pad_right_ascii(s: str, n: int, fill: bytes = b" ") -> bytes:
    b = s.encode("ascii", errors="replace")[:n]
    return b + fill * (n - len(b))


def pad_left_zero(n_or_str, width: int) -> bytes:
    s = str(n_or_str)
    return s.encode("ascii").rjust(width, b"0")


def signed_numeric(value: int, width: int) -> bytes:
    """Encode a signed integer as fixed-width ASCII, first byte is +/-
    and remaining width-1 bytes are digits left-padded with '0'."""
    sign = b"+" if value >= 0 else b"-"
    digits = str(abs(value)).encode("ascii").rjust(width - 1, b"0")
    return sign + digits


def signed_decimal(value_x100: int, width: int, scale: int) -> bytes:
    """Encode `value_x100` (already scaled by 10**scale) as signed fixed
    decimal: leading +/- followed by digits. The 'decimal point' is
    implicit at position scale from the right."""
    sign = b"+" if value_x100 >= 0 else b"-"
    digits = str(abs(value_x100)).encode("ascii").rjust(width - 1, b"0")
    return sign + digits


# ---------- 1. product_80 ----------

PRODUCTS = [
    ("4901234567894", "オラクルベリージュース 500ml", 198, 240, 20260415, "DRINK"),
    ("4901234567900", "オラクルベリーグミ 100g",      298, 120, 20260420, "FOOD"),
    ("4912345600013", "ハイビスカスティー ティーバッグ20p", 580, 35, 20260301, "DRINK"),
    ("4912345600020", "国産はちみつ 300g",            1280, 18, 20260225, "FOOD"),
    ("4923456700017", "コットンマスク 30枚入",          498,  -5, 20260510, "HEALTH"),
    ("4923456700024", "アルコール除菌スプレー 250ml",   398,   0, 20260512, "HEALTH"),
    ("4934567800014", "A4コピー用紙 500枚",            780,  60, 20260101, "STAT"),
    ("4934567800021", "中性洗剤詰替 800ml",            248, 999, 20260205, "HOUSE"),
    ("4945678900011", "電動歯ブラシ 替えブラシ4本",    1480,   8, 20260318, "HEALTH"),
    ("4956789000018", "ノートPCスタンド アルミ",       3980,  12, 20260420, "OFFICE"),
]


def gen_product_80(path: str) -> int:
    records = []
    for jan, name, price, stock, date, category in PRODUCTS:
        rec = b""
        rec += pad_right_ascii(jan, 13)
        rec += pad_right_sjis(name, 30)
        rec += pad_left_zero(price, 6)
        rec += signed_numeric(stock, 4)
        rec += str(date).encode("ascii")
        rec += pad_right_ascii(category, 7)
        rec += b" " * 12
        assert len(rec) == 80, f"product record is {len(rec)} bytes"
        records.append(rec)

    with open(path, "wb") as f:
        for r in records:
            f.write(r)
    return len(records)


# ---------- 2. pos_100_multi ----------

POS_TRANSACTIONS = [
    {
        "tx_id":   "TX2026052701",
        "store":   1023,
        "date":    20260527,
        "time":    "094512",
        "register": 3,
        "clerk":   "田中 健司",
        "items": [
            ("4901234567894", "オラクルベリージュース 500ml",  198, 2,    0),
            ("4934567800021", "中性洗剤詰替 800ml",            248, 1,    0),
            ("4923456700017", "コットンマスク 30枚入",          498, 3, -100),
        ],
    },
    {
        "tx_id":   "TX2026052702",
        "store":   1023,
        "date":    20260527,
        "time":    "101205",
        "register": 1,
        "clerk":   "佐藤 美咲",
        "items": [
            ("4956789000018", "ノートPCスタンド アルミ",       3980, 1,    0),
            ("4945678900011", "電動歯ブラシ 替えブラシ4本",    1480, 2, -200),
            ("4934567800014", "A4コピー用紙 500枚",            780,  5,    0),
            ("4912345600020", "国産はちみつ 300g",            1280, -1,    0),  # 返品
        ],
    },
]


def gen_pos_100(path: str) -> int:
    records = []
    for tx in POS_TRANSACTIONS:
        # --- header ---
        h = b"H"
        h += pad_right_ascii(tx["tx_id"], 12)
        h += pad_left_zero(tx["store"], 4)
        h += str(tx["date"]).encode("ascii")
        h += pad_right_ascii(tx["time"], 6)
        h += pad_left_zero(tx["register"], 4)
        h += pad_right_sjis(tx["clerk"], 30)
        h += b" " * 35
        assert len(h) == 100, f"pos header is {len(h)} bytes"
        records.append(h)

        # --- details ---
        total = 0
        for jan, name, unit, qty, discount in tx["items"]:
            sub = unit * qty + discount  # may be negative (refund)
            total += sub
            d = b"D"
            d += pad_right_ascii(jan, 13)
            d += pad_right_sjis(name, 30)
            d += pad_left_zero(unit, 8)
            d += signed_numeric(qty, 6)
            d += signed_decimal(sub, 10, scale=0)
            d += pad_left_zero(abs(discount), 10)
            d += b" " * 22
            assert len(d) == 100, f"pos detail is {len(d)} bytes"
            records.append(d)

        # --- footer ---
        tax = int(total * 0.10)
        t = b"T"
        t += pad_left_zero(len(tx["items"]), 6)
        t += signed_decimal(total, 12, scale=0)[1:].rjust(12, b"0")  # unsigned in footer
        t += pad_left_zero(tax if tax >= 0 else 0, 10)
        t += str(tx["date"]).encode("ascii")
        t += b" " * 63
        assert len(t) == 100, f"pos footer is {len(t)} bytes"
        records.append(t)

    with open(path, "wb") as f:
        for r in records:
            f.write(r)
    return len(records)


# ---------- 3. employee_120 ----------

EMPLOYEES = [
    # (社員番号, 氏名, 入社, 生年月日, 基本給, 評価係数x100, 部署, 役職(utf8), メモ)
    (10000001, "山田 太郎",       20100401, "1985-06-15", 380000, 125, "ES", "シニアエンジニア",   "勤続15年"),
    (10000042, "鈴木 花子",       20180401, "1995-11-30", 320000, 110, "PM", "プロダクトマネージャー", "新卒入社"),
    (10000103, "田中 一郎",       20050701, "1978-03-22", 580000,  95, "EX", "執行役員 CTO",       ""),
    (10000204, "伊藤 さくら",     20220401, "2000-09-08", 280000, 100, "ES", "ジュニアエンジニア", "研修修了"),
    (10000305, "渡辺 由紀",       20191001, "1992-04-14", 350000,  85, "QA", "QAリード",            "賃下げ調整-50000"),
    (10000356, "山本 美桜",       20230401, "2001-12-01", 260000, 105, "DS", "データサイエンティスト", "新卒2年目"),
    (10000420, "中村 賢治",       20120401, "1986-08-19", 420000, 120, "ES", "プリンシパルエンジニア", "海外プロジェクト経験あり"),
    (10000511, "小林 彩花",       20210401, "1998-02-27", 310000, 115, "PM", "アソシエイトPM",     ""),
    (10000687, "加藤 大輔",       20081101, "1980-10-05", 510000, 130, "EX", "VP of Engineering",   ""),
    (10000732, "吉田 玲",         20240401, "2002-07-11", 250000,  90, "ES", "新卒エンジニア",     "試用期間中"),
]


def gen_employee_120(path: str) -> int:
    rng = random.Random(0xBEEF)
    records = []
    for emp_id, name, hire, dob, salary, eval_x100, dept, title, memo in EMPLOYEES:
        rec = b""
        rec += pad_left_zero(emp_id, 8)
        rec += pad_right_sjis(name, 30)
        rec += str(hire).encode("ascii")
        rec += dob.encode("ascii")
        rec += signed_decimal(salary, 10, scale=0)
        rec += signed_decimal(eval_x100, 8, scale=2)
        rec += pad_right_ascii(dept, 2)
        rec += pad_right_utf8(title, 20)
        rec += pad_right_sjis(memo, 12)
        # 12 bytes of opaque "identifier": random but deterministic
        rec += struct.pack(">III", emp_id, hire, rng.randint(0, 0xFFFFFFFF))
        assert len(rec) == 120, f"employee record is {len(rec)} bytes"
        records.append(rec)

    with open(path, "wb") as f:
        for r in records:
            f.write(r)
    return len(records)


# ---------- main ----------

def main():
    here = os.path.dirname(os.path.abspath(__file__))
    os.chdir(here)

    n1 = gen_product_80("product_80.dat")
    print(f"wrote samples/product_80.dat  ({n1} records × 80 = {n1 * 80} bytes)")

    n2 = gen_pos_100("pos_100.dat")
    print(f"wrote samples/pos_100.dat     ({n2} records × 100 = {n2 * 100} bytes)")

    n3 = gen_employee_120("employee_120.dat")
    print(f"wrote samples/employee_120.dat ({n3} records × 120 = {n3 * 120} bytes)")


if __name__ == "__main__":
    main()
