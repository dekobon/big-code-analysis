import os.path

# Resolve the data file relative to this script rather than the
# caller's CWD, so the generator is hermetic no matter where it is
# invoked from (it always reads/writes its sibling c_macros.txt).
_DATA_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "c_macros.txt")

# `UINT*_MIN` are deliberately absent: the minimum of any unsigned
# integer type is 0, so the C standard defines no `UINT*_MIN` macros
# (only the signed `INT*_MIN` family and the `UINT*_MAX` family
# exist). Emitting them would over-include names that can never
# appear in real source.
macs = [
    "PRId{}", "PRIi{}", "PRIu{}", "PRIo{}", "PRIx{}", "PRIX{}",
    "PRIdLEAST{}", "PRIiLEAST{}", "PRIuLEAST{}", "PRIoLEAST{}", "PRIxLEAST{}", "PRIXLEAST{}",
    "PRIdFAST{}", "PRIiFAST{}", "PRIuFAST{}", "PRIoFAST{}", "PRIxFAST{}", "PRIXFAST{}",
    "PRIdMAX", "PRIiMAX", "PRIuMAX", "PRIoMAX", "PRIxMAX", "PRIXMAX",
    "PRIdPTR", "PRIiPTR", "PRIuPTR", "PRIoPTR", "PRIxPTR", "PRIXPTR",
    "SCNd{}", "SCNi{}", "SCNu{}", "SCNo{}", "SCNx{}",
    "SCNdLEAST{}", "SCNiLEAST{}", "SCNuLEAST{}", "SCNoLEAST{}", "SCNxLEAST{}",
    "SCNdFAST{}", "SCNiFAST{}", "SCNuFAST{}", "SCNoFAST{}", "SCNxFAST{}",
    "SCNdMAX", "SCNiMAX", "SCNuMAX", "SCNoMAX", "SCNxMAX",
    "SCNdPTR", "SCNiPTR", "SCNuPTR", "SCNoPTR", "SCNxPTR",
    "INT{}_MIN", "INT_FAST{}_MIN", "INT_LEAST{}_MIN", "INT{}_C",
    "INTPTR_MIN", "INTMAX_MIN",
    "INT{}_MAX", "INT_FAST{}_MAX", "INT_LEAST{}_MAX",
    "INTPTR_MAX", "INTMAX_MAX",
    "UINT{}_C",
    "UINT{}_MAX", "UINT_FAST{}_MAX", "UINT_LEAST{}_MAX",
    "UINTPTR_MAX", "UINTMAX_MAX",
]

macros = set()

for x in macs:
    for i in [8, 16, 32, 64]:
        macros.add(x.format(i))

old = set()

if os.path.isfile(_DATA_FILE):
    with open(_DATA_FILE, "r") as in_file:
        for line in in_file.readlines():
            old.add(line.strip())

diff = macros - old
if diff:
    for d in diff:
        old.add(d)
    with open(_DATA_FILE, "w") as out_file:
        for x in sorted(old):
            out_file.write(f"{x}\n")
