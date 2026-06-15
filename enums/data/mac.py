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

# Rewrite the data file from scratch every run: its contents are a pure
# function of the `macs` template above. An earlier append-only contract
# (read the file, union in only `macros - old`, write the union back)
# could grow `c_macros.txt` but never prune it, so a name removed from
# `macs` lingered forever and the artifact drifted away from its own
# generator (issue #892). Writing `sorted(macros)` makes removals take
# effect on the next run.
with open(_DATA_FILE, "w") as out_file:
    for x in sorted(macros):
        out_file.write(f"{x}\n")
