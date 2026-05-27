#!/usr/bin/env bash
# Generate a 3-record sample binary file matching schemas/sample_120.toml.
# Layout (120 bytes / record):
#   record_type(1) bank_code(4) branch_code(3) account_type(1) account_number(7)
#   account_holder(30, Shift_JIS) transfer_date(8) amount(10) ref_no(20)
#   ezk_code(1) note(20, Shift_JIS) filler(15)
set -euo pipefail

OUT="${1:-samples/sample_120.dat}"

emit_record() {
    python3 - "$@" <<'PY'
import sys
rt, bank, branch, atype, acct, holder, date, amount, ref, ezk, note = sys.argv[1:12]
def pad_right(s, n, enc, fill=b' '):
    b = s.encode(enc, errors='replace')
    if len(b) > n:
        b = b[:n]
    return b + fill * (n - len(b))
def pad_left_zero(s, n):
    b = s.encode('ascii')
    return b'0' * (n - len(b)) + b
rec = b''
rec += rt.encode('ascii')
rec += pad_left_zero(bank, 4)
rec += pad_left_zero(branch, 3)
rec += atype.encode('ascii')
rec += pad_left_zero(acct, 7)
rec += pad_right(holder, 30, 'cp932')
rec += date.encode('ascii')
rec += pad_left_zero(amount, 10)
rec += pad_right(ref, 20, 'ascii')
rec += ezk.encode('ascii')
rec += pad_right(note, 20, 'cp932')
rec += b' ' * 15
assert len(rec) == 120, len(rec)
sys.stdout.buffer.write(rec)
PY
}

mkdir -p "$(dirname "$OUT")"
{
    emit_record 1 0001 001 0 0000000 "ヘッダレコード"        20260101 0000000000 ""                     0 ""                     >/dev/null
    emit_record 1 0001 001 0 0000000 "ﾐｽﾞﾎ ﾀﾛｳ"             20260101 0000000000 "ZENGIN HEADER"        0 "送信元振込依頼人"
    emit_record 2 0001 234 1 1234567 "ﾔﾏﾀﾞ ﾊﾅｺ"             20260131 0001234567 "REF20260131-0001"     0 "1月分給与"
    emit_record 2 0009 876 2 7654321 "オラクルベリー株式会社" 20260131 0010000000 "REF20260131-0002"     1 "請求書 No.A-2026-09"
    emit_record 8 0000 000 0 0000000 "ﾄﾚｰﾗ"                  20260131 0000002000 ""                     0 ""
    emit_record 9 0000 000 0 0000000 "ｴﾝﾄﾞ"                  00000000 0000000000 ""                     0 ""
} > "$OUT" || true

# The first `emit_record … >/dev/null` line is intentionally suppressed so we
# emit exactly 5 useful records below. Replay properly:
python3 - "$OUT" <<'PY'
import sys, os
out = sys.argv[1]
def pad_right(s, n, enc, fill=b' '):
    b = s.encode(enc, errors='replace')
    if len(b) > n: b = b[:n]
    return b + fill * (n - len(b))
def pad_left_zero(s, n):
    b = s.encode('ascii')
    return b'0' * (n - len(b)) + b
def rec(rt, bank, branch, atype, acct, holder, date, amount, ref, ezk, note):
    r = (rt.encode('ascii')
        + pad_left_zero(bank, 4)
        + pad_left_zero(branch, 3)
        + atype.encode('ascii')
        + pad_left_zero(acct, 7)
        + pad_right(holder, 30, 'cp932')
        + date.encode('ascii')
        + pad_left_zero(amount, 10)
        + pad_right(ref, 20, 'ascii')
        + ezk.encode('ascii')
        + pad_right(note, 20, 'cp932')
        + b' ' * 15)
    assert len(r) == 120, len(r)
    return r

data = b''
data += rec('1','0001','001','0','0000000','ﾐｽﾞﾎ ﾀﾛｳ','20260101','0000000000','ZENGIN HEADER',       '0','送信元振込依頼人')
data += rec('2','0001','234','1','1234567','ﾔﾏﾀﾞ ﾊﾅｺ','20260131','0001234567','REF20260131-0001',   '0','1月分給与')
data += rec('2','0009','876','2','7654321','オラクルベリー株式会社','20260131','0010000000','REF20260131-0002','1','請求書 No.A-2026-09')
data += rec('8','0000','000','0','0000000','ﾄﾚｰﾗ',     '20260131','0000002000','',                    '0','')
data += rec('9','0000','000','0','0000000','ｴﾝﾄﾞ',     '00000000','0000000000','',                    '0','')

os.makedirs(os.path.dirname(out) or '.', exist_ok=True)
with open(out, 'wb') as f:
    f.write(data)
print(f"wrote {len(data)} bytes ({len(data)//120} records) to {out}")
PY
