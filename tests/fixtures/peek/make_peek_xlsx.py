# SYNTHETIC workbook for `fingerprint peek` merged-range, formula and column-type tests
# (bd-hra). Invented values, written by openpyxl (an independent writer).
#   uv run --with openpyxl python make_peek_xlsx.py .
#
# Sheet "Data":
#   A1:C1 merged title; row 3 header; rows 4-6 data (Widget/Gadget/Gizmo) with a formula in
#   column D (openpyxl stores no cached result, so the cell VALUES are empty but the formulas
#   exist); row 7 "Total" merged A7:C7 with =SUM in D7; column E has booleans.
# Sheet "Other": a single value (to check the sheet list).
import sys

import openpyxl

out = sys.argv[1]
wb = openpyxl.Workbook()
ws = wb.active
ws.title = "Data"
ws["A1"] = "Quarterly stock"
ws.merge_cells("A1:C1")
for column, name in enumerate(["Item", "Qty", "Price", "Total", "Active"], start=1):
    ws.cell(row=3, column=column, value=name)
rows = [("Widget", 3, 2.5, True), ("Gadget", 10, 1.25, False), ("Gizmo", 7, 4.0, True)]
for offset, (item, qty, price, active) in enumerate(rows):
    r = 4 + offset
    ws.cell(row=r, column=1, value=item)
    ws.cell(row=r, column=2, value=qty)
    ws.cell(row=r, column=3, value=price)
    ws.cell(row=r, column=4, value=f"=B{r}*C{r}")
    ws.cell(row=r, column=5, value=active)
ws["A7"] = "Total"
ws.merge_cells("A7:C7")
ws["D7"] = "=SUM(D4:D6)"
wb.create_sheet("Other")["A1"] = "x"
wb.save(f"{out}/merged_formulas.xlsx")
