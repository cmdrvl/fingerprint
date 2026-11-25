# Parquet fixtures for the footer-only assertions (bd-yq3), written by pyarrow (an independent
# writer). Run from this directory:
#   uv run --with pyarrow python make_parquet_fixtures.py .
#
# delivery.parquet    30 rows, 3 row groups, decimal/date/timestamp/list columns, key/value metadata
# body_zeroed.parquet same schema with 3000 rows; every byte between the leading magic and the
#                     footer is then overwritten with zeros. Footer-only assertions must still
#                     match: that is the proof that no data page is read.
import datetime as dt
import decimal
import struct
import sys

import pyarrow as pa
import pyarrow.parquet as pq

out = sys.argv[1]

schema = pa.schema(
    [
        ("loan_id", pa.string()),
        ("balance", pa.decimal128(18, 2)),
        ("as_of", pa.date32()),
        ("ts", pa.timestamp("us", tz="UTC")),
        ("rate", pa.float64()),
        ("active", pa.bool_()),
        ("seq", pa.int64()),
        ("tags", pa.list_(pa.string())),
    ]
)


def table(rows):
    return pa.table(
        {
            "loan_id": [f"L{i}" for i in range(rows)],
            "balance": [decimal.Decimal(i) / 100 for i in range(rows)],
            "as_of": [dt.date(2026, 1, 1) + dt.timedelta(days=i % 28) for i in range(rows)],
            "ts": [dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc) + dt.timedelta(seconds=i) for i in range(rows)],
            "rate": [0.05 + i / 10000 for i in range(rows)],
            "active": [i % 2 == 0 for i in range(rows)],
            "seq": list(range(rows)),
            "tags": [["a", "b"] if i % 3 == 0 else [] for i in range(rows)],
        },
        schema=schema,
    ).replace_schema_metadata({"delivery": "cmbs.v2", "producer": "fixture-gen", "secret_note": "do-not-print-me"})


pq.write_table(table(30), f"{out}/delivery.parquet", row_group_size=10, compression="none")

big = f"{out}/body_zeroed.parquet"
pq.write_table(table(3000), big, row_group_size=1000, compression="none")
data = bytearray(open(big, "rb").read())
footer_len = struct.unpack("<I", data[-8:-4])[0]
footer_start = len(data) - 8 - footer_len
assert data[:4] == b"PAR1" and data[-4:] == b"PAR1"
data[4:footer_start] = bytes(footer_start - 4)
open(big, "wb").write(bytes(data))

# canary.parquet: unique marker strings in a cell, a column NAME, a metadata key and a metadata
# value. `fingerprint peek` must never print the cell or the metadata value, and prints the name
# and key only with --explicit (tests/peek.rs).
canary = pa.table(
    {
        "canary_column_name": ["CANARY-CELL-7f3a", "CANARY-CELL-7f3b"],
        "amount": [1.5, 2.5],
    }
).replace_schema_metadata({"canary_key": "CANARY-META-VALUE-9c1d"})
pq.write_table(canary, f"{out}/canary.parquet", compression="none")
