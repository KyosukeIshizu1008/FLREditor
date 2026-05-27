#!/usr/bin/env python3
"""
Generate varied test data conforming to schemas/sample_120.toml.

Each record is 120 bytes:
  record_type(1) bank_code(4) branch_code(3) account_type(1) account_number(7)
  account_holder(30, Shift_JIS) transfer_date(8) amount(10) ref_no(20)
  ezk_code(1) note(20, Shift_JIS) filler(15)

Usage:
  python3 gen_varied.py samples/varied_1000.dat 1000
  python3 gen_varied.py samples/varied_100k.dat 100000
  python3 gen_varied.py samples/varied_1M.dat 1000000
"""
import os
import random
import sys

# Realistic-ish 全銀協 bank codes and corresponding short names.
BANKS = [
    ("0001", "ミズホ"),
    ("0005", "ＭＵＦＧ"),
    ("0009", "ＳＭＢＣ"),
    ("0010", "リソナ"),
    ("0017", "サイタマリソナ"),
    ("0033", "ＰａｙＰａｙ"),
    ("0036", "ジブン"),
    ("0038", "ＳＢＩシンセイ"),
    ("0040", "アオゾラ"),
    ("0397", "オラクルベリー"),
]

HOLDER_KATAKANA = [
    "ﾔﾏﾀﾞ ﾀﾛｳ", "ｽｽﾞｷ ﾊﾅｺ", "ﾀﾅｶ ｲﾁﾛｳ", "ｲﾄｳ ｻｸﾗ",
    "ﾜﾀﾅﾍﾞ ﾕｳｷ", "ﾔﾏﾓﾄ ﾐｵ", "ﾅｶﾑﾗ ｹﾝｼﾞ", "ｺﾊﾞﾔｼ ｱﾔｶ",
    "ｶﾄｳ ﾀﾞｲｽｹ", "ﾖｼﾀﾞ ﾚｲ", "ｻﾄｳ ﾐｻｷ", "ﾀｶﾊｼ ｿｳﾀ",
    "ｲﾉｳｴ ﾕｲ", "ﾓﾘ ｼｮｳﾀ", "ﾎﾝﾀﾞ ﾐｽﾞｷ",
]

HOLDER_KANJI = [
    "オラクルベリー株式会社",
    "山田商事株式会社",
    "鈴木運輸株式会社",
    "田中製作所",
    "伊藤化学工業株式会社",
    "渡辺フードサービス",
    "山本電子部品",
    "中村建設株式会社",
    "小林システムズ",
    "加藤ホールディングス",
    "吉田情報通信",
    "佐藤食品工業",
    "高橋メディカル",
    "井上不動産",
    "森印刷株式会社",
    "本田クリエイティブ",
    "東京テクノロジー研究所",
    "関西物流センター",
    "九州エネルギー機構",
    "北海道乳業株式会社",
]

NOTE_TEMPLATES = [
    "1月分給与",
    "2月分給与",
    "3月分給与",
    "請求書 No.A-2026-{n:04d}",
    "経費精算 #{n}",
    "賞与支給",
    "業務委託料",
    "コンサル料 {n}月",
    "保守料金",
    "リース料 {n}月分",
    "立替金精算",
    "返金 No.{n}",
    "決済手数料",
    "源泉徴収戻り",
    "貸付返済 #{n}",
]


def pad_right_sjis(s: str, n: int, fill: bytes = b" ") -> bytes:
    """Encode `s` in CP932 and pad right to exactly `n` bytes.
    If `s` is too long, truncate at the last full character so we never emit
    a half-byte multi-byte sequence."""
    out = bytearray()
    for ch in s:
        eb = ch.encode("cp932", errors="replace")
        if len(out) + len(eb) > n:
            break
        out += eb
    return bytes(out) + fill * (n - len(out))


def pad_left_zero(s: str, n: int) -> bytes:
    b = s.encode("ascii")
    return b"0" * (n - len(b)) + b


def make_record(
    record_type: str,
    bank: str,
    branch: str,
    account_type: str,
    account_no: str,
    holder: str,
    date: str,
    amount: str,
    ref_no: str,
    ezk: str,
    note: str,
) -> bytes:
    rec = b""
    rec += record_type.encode("ascii")
    rec += pad_left_zero(bank, 4)
    rec += pad_left_zero(branch, 3)
    rec += account_type.encode("ascii")
    rec += pad_left_zero(account_no, 7)
    rec += pad_right_sjis(holder, 30)
    rec += date.encode("ascii")
    rec += pad_left_zero(amount, 10)
    rec += pad_right_sjis(ref_no, 20, b" ")
    rec += ezk.encode("ascii")
    rec += pad_right_sjis(note, 20)
    rec += b" " * 15
    assert len(rec) == 120, len(rec)
    return rec


def random_date_2026(rng: random.Random) -> str:
    days_per_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    m = rng.randint(1, 12)
    d = rng.randint(1, days_per_month[m - 1])
    return f"2026{m:02d}{d:02d}"


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: gen_varied.py OUT_PATH N_RECORDS")
    out_path, n_str = sys.argv[1], sys.argv[2]
    n_records = int(n_str)

    rng = random.Random(0xFEED)  # deterministic seed for reproducibility
    chunks = []
    CHUNK = 50_000

    # Header (1)
    chunks.append(
        make_record(
            "1", "0397", "001", "0", "0000000",
            "オラクルベリー振込依頼", "20260101", "0000000000",
            "ZENGIN HEADER", "0", "送信元 全銀フォーマット",
        )
    )

    # Data (2) — n_records - 3 records (header, trailer, end take 3)
    body_count = max(0, n_records - 3)
    written = 1
    buf = b""
    for i in range(body_count):
        bank_code, _ = rng.choice(BANKS)
        branch = f"{rng.randint(1, 999):03d}"
        atype = rng.choice(["1", "1", "1", "1", "2", "2", "9"])  # weight: most 普通
        acct = f"{rng.randint(0, 9_999_999):07d}"
        holder = rng.choice(HOLDER_KATAKANA + HOLDER_KANJI)
        date = random_date_2026(rng)
        # amount: weight toward sub-million yen, with occasional big amounts
        if rng.random() < 0.02:
            amount = f"{rng.randint(10_000_000, 9_999_999_999)}"
        elif rng.random() < 0.10:
            amount = f"{rng.randint(100_000, 9_999_999)}"
        else:
            amount = f"{rng.randint(1_000, 999_999)}"
        ref_no = f"REF20260{rng.randint(1,12):02d}-{i+1:06d}"
        ezk = rng.choice(["0", "0", "0", "0", "1"])
        note_t = rng.choice(NOTE_TEMPLATES)
        note = note_t.format(n=i + 1) if "{n" in note_t else note_t
        buf += make_record(
            "2", bank_code, branch, atype, acct, holder, date, amount, ref_no, ezk, note
        )
        if (i + 1) % CHUNK == 0:
            chunks.append(buf)
            buf = b""
    if buf:
        chunks.append(buf)
        written += body_count

    # Trailer (8) — totals are illustrative (we don't actually compute them)
    chunks.append(
        make_record(
            "8", "0000", "000", "0", "0000000",
            "トレーラ", "20260131", f"{body_count:010d}",
            "TRAILER", "0", "合計件数",
        )
    )

    # End (9)
    chunks.append(
        make_record(
            "9", "0000", "000", "0", "0000000",
            "エンド", "00000000", "0000000000",
            "END", "0", "",
        )
    )

    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    with open(out_path, "wb") as f:
        for c in chunks:
            f.write(c)

    total_bytes = sum(len(c) for c in chunks)
    print(
        f"wrote {total_bytes // 120} records ({total_bytes} bytes) to {out_path}"
    )


if __name__ == "__main__":
    main()
