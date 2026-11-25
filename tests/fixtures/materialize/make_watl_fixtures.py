# SYNTHETIC CMBS watchlist workbooks for the `fingerprint materialize` tests (bd-uai).
# They imitate the layout documented in tests/fixtures/cmbs-watl/README.md (CREFC boilerplate on
# row 2, header on row 11, odd sheet name, blank row inside the data, footnotes below), but every
# value is invented. There is no real servicer workbook in this repository; do not read these as
# real data. Written by openpyxl (an independent writer).
#   uv run --with openpyxl python make_watl_fixtures.py .
import datetime as dt
import sys

import openpyxl

out = sys.argv[1]
HEADER = ["Loan ID", "Property Name", "Servicer Status", "Maturity Date", "Balance", "DSCR"]
DATA = [
    ("L001", "Harbor Plaza", "Watchlist", dt.datetime(2027, 3, 1), 12500000.0, 1.18),
    ("L002", "Elm Court", "Watchlist", dt.datetime(2026, 12, 15), 8400000.5, 0.97),
    ("L003", "North Gate, Bldg A", "Special", dt.datetime(2028, 1, 1, 9, 30), 21000000.0, 1.42),
    ("L004", "Riverside Lofts", "Watchlist", dt.datetime(2027, 7, 1), 5000000.0, 1.05),
    None,  # a blank row inside the data
    ("L005", "Station Square", "Watchlist", dt.datetime(2029, 2, 1), 33250000.0, 1.31),
]


def base(sheet_title):
    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = sheet_title
    ws["A1"] = "Synthetic Servicer Report"
    ws["A2"] = "CREFC Investor Reporting Package (IRP) - Servicer Watch List"
    ws["A5"] = "Reporting period"
    ws["B5"] = "2026-10"
    for column, name in enumerate(HEADER, start=1):
        ws.cell(row=11, column=column, value=name)
    row = 12
    for record in DATA:
        if record is not None:
            for column, value in enumerate(record, start=1):
                cell = ws.cell(row=row, column=column, value=value)
                if isinstance(value, dt.datetime):
                    cell.number_format = "yyyy-mm-dd" if value.hour == 0 else "yyyy-mm-dd hh:mm:ss"
        row += 1
    # data rows are 12..17 (row 16 blank); footnotes follow the data
    ws["A19"] = "Footnote 1: synthetic data."
    ws["A20"] = "Footnote 2: not a real servicer report."
    return wb, ws


wb, ws = base("Servicer Watch List")
wb.create_sheet("Notes")["A1"] = "scratch"
wb.save(f"{out}/watl_msc.xlsx")

# Next period: same layout, L002's balance and DSCR moved (the change shape/rvl should see).
wb, ws = base("Servicer Watch List")
ws["E13"] = 8100000.5
ws["F13"] = 0.91
wb.save(f"{out}/watl_msc_next.xlsx")

wb, ws = base("Servicer Watch List")
ws["E13"] = "#DIV/0!"  # becomes an Excel error cell
ws["E13"].data_type = "e"
wb.save(f"{out}/watl_error_cell.xlsx")

wb, ws = base("Servicer Watch List")
ws["B11"] = "Loan ID"  # duplicate header
wb.save(f"{out}/watl_dup_header.xlsx")

wb, ws = base("Servicer Watch List")
ws["D11"] = None  # empty header cell
wb.save(f"{out}/watl_empty_header.xlsx")

wb = openpyxl.Workbook()
wb.active.title = "Summary"
wb.active["A1"] = "not a watchlist"
wb.save(f"{out}/not_watl.xlsx")
