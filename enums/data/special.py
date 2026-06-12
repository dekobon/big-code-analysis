import os.path

# Resolve the data file relative to this script rather than the
# caller's CWD, so the generator is hermetic no matter where it is
# invoked from (it always reads/writes its sibling c_specials.txt).
_DATA_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "c_specials.txt")

# `char{}_t` only expands to the real C/C++ character types
# `char8_t`, `char16_t`, `char32_t`; there is no `char64_t`, so the
# 64-bit width is excluded below. `charptr_t` is likewise not a real
# type name and is omitted (the genuine pointer-difference type is
# `ptrdiff_t`).
#
# these ids mustn't be treated as macros
specs = [
    "int{}_t", "int_fast{}_t", "int_least{}_t",
    "uint{}_t", "uint_fast{}_t", "uint_least{}_t",
    "bool", "char", "int", "long", "short", "float", "double",
    "size_t", "ssize_t", "intmax_t", "intptr_t", "uintptr_t",
    "uintmax_t", "ptrdiff_t", "max_align_t", "wchar_t",
    "signed", "unsigned", "false", "true", "nullptr", "NULL",
    "static", "const", "inline", "restrict", "constexpr", "mutable", "explicit", "namespace",
]

# `char{}_t` has no 64-bit form, so it is widened separately over the
# real widths only.
char_widths = ["char{}_t"]

specials = set()

for x in specs:
    for i in [8, 16, 32, 64]:
        specials.add(x.format(i))

for x in char_widths:
    for i in [8, 16, 32]:
        specials.add(x.format(i))

old = set()

if os.path.isfile(_DATA_FILE):
    with open(_DATA_FILE, "r") as in_file:
        for line in in_file.readlines():
            old.add(line.strip())

diff = specials - old
if diff:
    for d in diff:
        old.add(d)
    with open(_DATA_FILE, "w") as out_file:
        for x in sorted(old):
            out_file.write(f"{x}\n")
