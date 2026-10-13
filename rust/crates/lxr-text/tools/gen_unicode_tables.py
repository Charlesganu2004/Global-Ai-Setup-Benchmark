"""Writes src/unicode_data.rs: the character classes the BERT normaliser needs.

The tables are read off the installed `tokenizers` library, because that library
is what the reference model uses, and its Unicode tables are older than Python's.
Run it with the interpreter that has `tokenizers` installed:

    python -I gen_unicode_tables.py <tokenizer.json> <output .rs>
"""
import string
import sys
import unicodedata as ud

from tokenizers import Tokenizer

CJK = [(0x4E00, 0x9FFF), (0x3400, 0x4DBF), (0x20000, 0x2A6DF), (0x2A700, 0x2B73F),
       (0x2B740, 0x2B81F), (0x2B920, 0x2CEAF), (0xF900, 0xFAFF), (0x2F800, 0x2FA1F)]


def is_cjk(c):
    return any(a <= c <= b for a, b in CJK)


def ranges(values):
    out = []
    for v in sorted(values):
        if out and out[-1][1] + 1 == v:
            out[-1][1] = v
        else:
            out.append([v, v])
    return out


def emit(name, doc, values):
    lines = [f"/// {doc}", "#[rustfmt::skip]", f"pub(crate) static {name}: &[(u32, u32)] = &["]
    row = []
    for a, b in ranges(values):
        row.append(f"(0x{a:X}, 0x{b:X}),")
        if len(row) == 6:
            lines.append("    " + " ".join(row))
            row = []
    if row:
        lines.append("    " + " ".join(row))
    lines.append("];")
    return "\n".join(lines)


def main():
    tok = Tokenizer.from_file(sys.argv[1])
    tok.no_padding()
    cps = [c for c in range(0x110000) if not 0xD800 <= c <= 0xDFFF]
    encs = tok.encode_batch(["a" + chr(c) + "b" for c in cps], add_special_tokens=False)
    a = tok.encode("a", add_special_tokens=False).ids[0]
    b = tok.encode("b", add_special_tokens=False).ids[0]
    ab = tok.encode("ab", add_special_tokens=False).ids
    a_b = tok.encode("a b", add_special_tokens=False).ids
    dropped = set()
    isolated = set()
    for c, e in zip(cps, encs):
        if e.ids == ab:
            dropped.add(c)
        elif e.ids != a_b and len(e.ids) >= 2 and e.ids[0] == a and e.ids[-1] == b:
            isolated.add(c)
    control = {c for c in dropped if ud.category(chr(c)) in ("Cc", "Cf", "Co")} | {0xFFFD}
    marks = dropped - control
    ascii_punct = {ord(x) for x in string.punctuation}
    punct = {c for c in isolated
             if c > 0x7F and not is_cjk(c) and ud.normalize("NFD", chr(c)) == chr(c)}
    text = "\n\n".join([
        "//! Character classes of the BERT normaliser, read off the reference tokenizer by\n"
        "//! `tools/gen_unicode_tables.py`. Do not edit by hand.",
        emit("CONTROL", "Characters that `clean_text` removes: control and format characters, private use, U+FFFD.", control),
        emit("MARKS", "Characters that accent stripping removes after canonical decomposition.", marks),
        emit("PUNCT", "Non-ASCII characters that the pre-tokenizer splits off as words of their own.", punct),
    ]) + "\n"
    with open(sys.argv[2], "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print("control", len(control), "marks", len(marks), "punct", len(punct))
    print("odd marks", [(hex(c), ud.category(chr(c))) for c in marks if ud.category(chr(c)) != "Mn"])


main()
