#!/usr/bin/env python3
"""Translate the MASM subset of 7-Zip's Asm/x86/LzmaDecOpt.asm into GNU as.

The output is fully expanded Intel-syntax assembly (`.intel_syntax noprefix`)
for one ABI, ready for `global_asm!`: no macros, no register aliases, no
assembler-time conditionals. Only the constructs the two upstream files use
are supported, and each one is translated by rule:

  ; comment                   -> # comment (top level only; macro bodies
                                 lose their comments)
  comment ~ ... ~             -> dropped
  include FILE                -> inlined (searched next to the input)
  ifdef / ifndef / if /       -> evaluated here; `-D` names the ifdef
    elseif / else / endif        symbols, `equ`'d names count as defined
  NAME equ REGISTER-ALIAS     -> every use becomes the final register name
  NAME equ NUMERIC-EXPR       -> `.set LNAME, expr` (SHL / SHR -> << / >>),
                                 every use becomes LNAME
  NAME equ [reg].Struct.      -> `NAME field` becomes
                                 `size ptr [reg + LStruct_field]`
  NAME struct ... ends        -> `.set LStruct_field, offset` (db / dd / dq
                                 laid out back to back, as MASM does here)
                                 and `.set Lsizeof_Struct, size`
  NAME macro args ... endm    -> expanded inline, parameters replaced
                                 token-wise, @CatStr(a, b) concatenated
  MY_PROC / MY_ENDP           -> the entry label is left to the includer;
                                 MY_ENDP becomes `ret`
  MY_ASM_START, OPTION, SEGMENT / ENDS, PROC / ENDP, .code, end -> dropped
  @@: / @F / @B               -> 1: / 1f / 1b
  label:                      -> Llabel: (assembler-local on Mach-O)
  align N                     -> .p2align log2(N)
  SHORT / near ptr            -> dropped (the assembler picks the encoding)
  BYTE PTR, SHL, RSP, ...     -> lowercase / GNU spelling
  123h                        -> 0x123

Every identifier in an instruction must resolve to a register, a constant, a
struct field, a label or the mnemonic itself; anything else is an error, so a
construct this script does not know cannot slip through silently.

Usage: masm2gas.py [-D NAME ...] LzmaDecOpt.asm > out.s
"""

import argparse
import re
import sys
from pathlib import Path

REGS = set()
for _r in ("ax", "bx", "cx", "dx", "si", "di", "bp", "sp"):
    REGS |= {"r" + _r, "e" + _r, _r}
REGS |= {"al", "bl", "cl", "dl", "ah", "bh", "ch", "dh", "sil", "dil", "bpl", "spl"}
for _i in range(8, 16):
    REGS |= {f"r{_i}", f"r{_i}d", f"r{_i}w", f"r{_i}b"}

SIZE_PTR = {1: "byte", 2: "word", 4: "dword", 8: "qword"}
FIELD_SIZES = {"db": 1, "dw": 2, "dd": 4, "dq": 8}
PTR_SIZES = {"byte", "word", "dword", "qword"}
DROPPED_DIRECTIVES = {"option", ".code", ".386", ".model", "end", "include"}
CMP_OPS = {"eq": "==", "ne": "!=", "gt": ">", "lt": "<", "ge": ">=", "le": "<="}

TOKEN_RE = re.compile(
    r"""
    (?P<ws>[ \t]+)
  | (?P<str>'[^']*')
  | (?P<ident>[A-Za-z_@$?.][A-Za-z0-9_@$?]*)
  | (?P<num>[0-9][0-9A-Fa-f]*[hH]?)
  | (?P<other>.)
""",
    re.X,
)
LABEL_RE = re.compile(r"^\s*([A-Za-z_@][A-Za-z0-9_@]*):\s*(.*)$")
STRUCT_PREFIX_RE = re.compile(r"^\[\s*([A-Za-z_][A-Za-z0-9_]*)\s*\]\.([A-Za-z_][A-Za-z0-9_]*)\.$")


def tokenize(s):
    return [(m.lastgroup, m.group()) for m in TOKEN_RE.finditer(s)]


def untokenize(toks):
    return "".join(v for _, v in toks)


def strip_ws(toks):
    return [t for t in toks if t[0] != "ws"]


def masm_number(s):
    if s[-1] in "hH":
        return int(s[:-1], 16)
    return int(s, 10)


def gas_number(s):
    if s[-1] in "hH":
        return "0x" + s[:-1].upper()
    return s


def split_args(argstr):
    """Split a macro argument list at top-level commas (`(a, b)` allowed)."""
    argstr = argstr.strip()
    if argstr.startswith("(") and argstr.endswith(")"):
        argstr = argstr[1:-1]
    args, depth, cur = [], 0, ""
    for ch in argstr:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            args.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip() or args:
        args.append(cur.strip())
    return args


class TranslationError(Exception):
    pass


class Translator:
    def __init__(self, defines):
        self.defined = set(defines)
        self.equ_text = {}  # name -> single-identifier alias (registers, mostly)
        self.numeric = {}  # name -> (expression tokens, value)
        self.numeric_order = []
        self.struct_prefix = {}  # name -> (register alias, struct name)
        self.macros = {}  # name -> (params, body lines)
        self.structs = {}  # name -> (fields {lower: (name, off, size)}, size)
        self.labels = set()
        self.code = []
        self.used_numeric = set()
        self.used_fields = set()
        self.used_sizeof = set()
        self.cond = []  # [parent active, this branch active, a branch was taken]
        self.where = "?"

    # ---- errors ----

    def fail(self, msg):
        raise TranslationError(f"{self.where}: {msg}")

    # ---- input ----

    def load(self, path):
        """Reads `path`, inlines `include`, drops `comment ~ ... ~` blocks."""
        lines = []
        skipping = False
        for lineno, raw in enumerate(path.read_text().splitlines(), 1):
            line = raw.rstrip()
            code = line.split(";", 1)[0]
            if skipping:
                if "~" in code:
                    skipping = False
                continue
            m = re.match(r"^\s*comment\s+~", code, re.I)
            if m:
                skipping = "~" in code[m.end():]
                skipping = not skipping
                continue
            m = re.match(r"^\s*include\s+(\S+)", code, re.I)
            if m:
                lines.extend(self.load(path.parent / m.group(1)))
                continue
            lines.append((f"{path.name}:{lineno}", line))
        return lines

    # ---- conditionals ----

    def active(self):
        return all(c[1] for c in self.cond)

    def is_defined(self, name):
        return name in self.defined or name in self.equ_text or name in self.numeric or name in self.struct_prefix

    def eval_expr(self, toks):
        """Evaluates a MASM constant expression to an int."""
        parts = []
        for kind, val in strip_ws(toks):
            if kind == "num":
                parts.append(str(masm_number(val)))
            elif kind == "ident":
                low = val.lower()
                if low == "shl":
                    parts.append("<<")
                elif low == "shr":
                    parts.append(">>")
                elif low in CMP_OPS:
                    parts.append(CMP_OPS[low])
                elif low == "sizeof":
                    parts.append("SIZEOF")
                elif parts and parts[-1] == "SIZEOF":
                    parts[-1] = str(self.structs[val][1])
                elif val in self.numeric:
                    parts.append(str(self.numeric[val][1]))
                elif val in self.equ_text:
                    parts.append(str(self.eval_expr(tokenize(self.equ_text[val]))))
                else:
                    raise KeyError(val)
            elif val == "/":
                parts.append("//")
            else:
                parts.append(val)
        return int(eval(" ".join(parts), {"__builtins__": {}}, {}))

    def handle_conditional(self, first, rest):
        low = first.lower()
        if low in ("ifdef", "ifndef"):
            parent = self.active()
            name = rest.strip()
            val = self.is_defined(name)
            if low == "ifndef":
                val = not val
            self.cond.append([parent, parent and val, val])
        elif low == "if":
            parent = self.active()
            val = parent and self.eval_expr(tokenize(rest)) != 0
            self.cond.append([parent, parent and val, val])
        elif low == "elseif":
            c = self.cond[-1]
            if c[2]:
                c[1] = False
            else:
                val = c[0] and self.eval_expr(tokenize(rest)) != 0
                c[1] = c[0] and val
                c[2] = val
        elif low == "else":
            c = self.cond[-1]
            c[1] = c[0] and not c[2]
            c[2] = True
        elif low == "endif":
            self.cond.pop()
        else:
            return False
        return True

    # ---- definitions ----

    def define_equ(self, name, value):
        value = value.strip()
        m = STRUCT_PREFIX_RE.match(value)
        if m:
            self.struct_prefix[name] = (m.group(1), m.group(2))
            return
        toks = tokenize(value)
        try:
            val = self.eval_expr(toks)
        except (KeyError, SyntaxError, NameError, TypeError):
            self.equ_text[name] = value
            return
        self.numeric[name] = (toks, val)
        self.numeric_order.append(name)

    def define_struct(self, name, body):
        fields, off = {}, 0
        for where, line in body:
            toks = strip_ws(tokenize(line.split(";", 1)[0]))
            if not toks:
                continue
            fname = toks[0][1]
            ftype = toks[1][1]
            while ftype in self.equ_text:  # PTR_FIELD equ dq ?
                ftype = strip_ws(tokenize(self.equ_text[ftype]))[0][1]
            size = FIELD_SIZES.get(ftype.lower())
            if size is None:
                self.fail(f"{where}: unknown field type {ftype!r} in struct {name}")
            fields[fname.lower()] = (fname, off, size)
            off += size
        self.structs[name] = (fields, off)

    # ---- resolution ----

    def resolve(self, name):
        """Final spelling of an identifier used as an operand."""
        if name in self.numeric:
            self.used_numeric.add(name)
            return "L" + name
        if name in self.equ_text:
            alias = strip_ws(tokenize(self.equ_text[name]))
            if len(alias) != 1 or alias[0][0] != "ident":
                self.fail(f"{name!r} is not a register alias: {self.equ_text[name]!r}")
            return self.resolve(alias[0][1])
        if name.lower() in REGS:
            return name.lower()
        if name in self.labels:
            return "L" + name
        self.fail(f"unknown identifier {name!r}")

    def field_access(self, prefix, field):
        reg_alias, sname = self.struct_prefix[prefix]
        fields, _ = self.structs[sname]
        if field.lower() not in fields:
            self.fail(f"struct {sname} has no field {field!r}")
        fname, _, size = fields[field.lower()]
        self.used_fields.add((sname, fname))
        return f"{SIZE_PTR[size]} ptr [{self.resolve(reg_alias)} + L{sname}_{fname}]"

    def translate_instruction(self, toks):
        out = []
        i, n = 0, len(toks)
        mnemonic_done = False

        def skip_ws(j):
            while j < n and toks[j][0] == "ws":
                j += 1
            return j

        while i < n:
            kind, val = toks[i]
            i += 1
            if kind == "ws":
                out.append(val)
                continue
            if kind == "num":
                out.append(gas_number(val))
                continue
            if kind != "ident":
                out.append(val)
                continue
            if not mnemonic_done:
                mnemonic_done = True
                out.append(val.lower())
                continue
            low = val.lower()
            if val in ("@F", "@f"):
                out.append("1f")
            elif val in ("@B", "@b"):
                out.append("1b")
            elif low == "short":
                i = skip_ws(i)
            elif low == "near":
                i = skip_ws(i)
                if i < n and toks[i][1].lower() == "ptr":
                    i = skip_ws(i + 1)
                else:
                    self.fail("`near` without `ptr`")
            elif low in PTR_SIZES or low == "ptr":
                out.append(low)
            elif low == "shl":
                out.append("<<")
            elif low == "shr":
                out.append(">>")
            elif low == "sizeof":
                i = skip_ws(i)
                sname = toks[i][1]
                i += 1
                if sname not in self.structs:
                    self.fail(f"SIZEOF of unknown struct {sname!r}")
                self.used_sizeof.add(sname)
                out.append(f"Lsizeof_{sname}")
            elif val in self.struct_prefix:
                i = skip_ws(i)
                field = toks[i][1]
                i += 1
                out.append(self.field_access(val, field))
            else:
                out.append(self.resolve(val))
        return "".join(out)

    # ---- macro expansion ----

    def substitute(self, line, mapping):
        toks = tokenize(line)
        out = []
        for kind, val in toks:
            if kind == "ident" and val in mapping:
                out.extend(mapping[val])
            else:
                out.append((kind, val))
        # @CatStr(a, b) -> ab, after parameter substitution, as MASM does.
        while True:
            idx = next((k for k, t in enumerate(out) if t[1] == "@CatStr"), None)
            if idx is None:
                break
            j = idx + 1
            while out[j][0] == "ws":
                j += 1
            if out[j][1] != "(":
                self.fail("@CatStr without parentheses")
            depth, k = 0, j
            while True:
                if out[k][1] == "(":
                    depth += 1
                elif out[k][1] == ")":
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            joined = "".join(v for kd, v in out[j + 1:k] if kd != "ws" and v != ",")
            out[idx:k + 1] = [("ident", joined)]
        return untokenize(out)

    def expand_macro(self, name, argstr, where):
        params, body = self.macros[name]
        args = split_args(argstr)
        if len(args) > len(params):
            self.fail(f"macro {name} takes {len(params)} argument(s), got {len(args)}")
        args += [""] * (len(params) - len(args))
        mapping = {p: tokenize(a) for p, a in zip(params, args)}
        lines = [(f"{where} <{name}>", self.substitute(l.split(";", 1)[0], mapping)) for l in body]
        self.process(lines)

    # ---- main loop ----

    def emit(self, text):
        self.code.append(text)

    def process(self, lines):
        it = iter(lines)
        for where, raw in it:
            self.where = where
            in_macro = "<" in where
            code, _, comment = raw.partition(";")
            code = code.rstrip()
            stripped = code.strip()
            toks = strip_ws(tokenize(code))
            first = toks[0][1] if toks else ""
            rest = code.strip()[len(first):] if toks else ""

            if toks and self.handle_conditional(first, rest):
                continue
            if not self.active():
                continue
            if not toks:
                if comment and not in_macro:
                    self.emit("#" + comment)
                elif not in_macro:
                    self.emit("")
                continue

            low = first.lower()
            second = toks[1][1].lower() if len(toks) > 1 else ""

            # Definitions.
            if second == "equ":
                self.define_equ(first, stripped[len(first):].strip()[3:])
                continue
            if second == "=":
                continue  # proc_numParams = ...; unused
            if second == "macro":
                params = [p.strip().split(":")[0] for p in stripped[len(first):].strip()[5:].split(",") if p.strip()]
                body = []
                for where2, raw2 in it:
                    if raw2.split(";", 1)[0].strip().lower() == "endm":
                        break
                    body.append(raw2)
                self.macros[first] = (params, body)
                continue
            if second == "struct":
                body = []
                for where2, raw2 in it:
                    if raw2.split(";", 1)[0].strip().lower() == f"{first.lower()} ends":
                        break
                    body.append((where2, raw2))
                self.define_struct(first, body)
                continue

            # Directives without a translation.
            if low in DROPPED_DIRECTIVES or second in ("segment", "ends", "proc", "endp"):
                continue
            if low == ".err":
                self.fail(f"assembler-time error triggered: {stripped}")
            if low == "my_asm_start":
                continue
            if low == "my_proc":
                self.emit(f"# {stripped}: entry label emitted by the includer")
                continue
            if low == "my_endp":
                self.emit("        ret")
                continue
            if low == "align":
                n = self.eval_expr(toks[1:])
                if n & (n - 1):
                    self.fail(f"align {n} is not a power of two")
                self.emit(f"        .p2align {n.bit_length() - 1}")
                continue

            # Labels.
            m = LABEL_RE.match(code)
            if m:
                label, tail = m.groups()
                self.emit("1:" if label == "@@" else f"L{label}:")
                if tail.strip():
                    self.process([(where, tail)])
                continue

            # Macro calls; a top-level call is echoed so the expansion can be
            # matched against the upstream source.
            if first in self.macros:
                if not in_macro and not first.startswith("MY_ALIGN"):
                    self.emit(f"# {stripped}")
                self.expand_macro(first, rest, where)
                continue

            # Instructions.
            text = "        " + self.translate_instruction(tokenize(code.strip()))
            if comment and not in_macro:
                text += " #" + comment
            self.emit(text)

    # ---- output ----

    def numeric_closure(self):
        wanted = set(self.used_numeric)
        while True:
            more = set()
            for name in wanted:
                for kind, val in self.numeric[name][0]:
                    if kind == "ident" and val in self.numeric and val not in wanted:
                        more.add(val)
            if not more:
                break
            wanted |= more
        return [n for n in self.numeric_order if n in wanted]

    def numeric_expr(self, toks):
        out = []
        for kind, val in toks:
            if kind == "num":
                out.append(gas_number(val))
            elif kind == "ident":
                low = val.lower()
                if low == "shl":
                    out.append("<<")
                elif low == "shr":
                    out.append(">>")
                elif val in self.numeric:
                    out.append("L" + val)
                else:
                    self.fail(f"non-numeric name {val!r} in a constant expression")
            else:
                out.append(val)
        return "".join(out).strip()

    def render(self):
        out = ["# ---- constants ----"]
        for name in self.numeric_closure():
            toks, val = self.numeric[name]
            out.append(f".set L{name}, {self.numeric_expr(toks)}")
        out.append("")
        out.append("# ---- struct offsets ----")
        for sname, (fields, size) in self.structs.items():
            for fname, off, fsize in sorted(fields.values(), key=lambda f: f[1]):
                if (sname, fname) in self.used_fields:
                    out.append(f".set L{sname}_{fname}, {off}")
            if sname in self.used_sizeof:
                out.append(f".set Lsizeof_{sname}, {size}")
        out.append("")
        out.append("# ---- code ----")
        prev_blank = False
        for line in self.code:
            blank = line == ""
            if blank and prev_blank:
                continue
            out.append(line)
            prev_blank = blank
        return "\n".join(out).rstrip() + "\n"

    def run(self, path):
        lines = self.load(path)
        for _, raw in lines:
            m = LABEL_RE.match(raw.split(";", 1)[0])
            if m and m.group(1) != "@@":
                self.labels.add(m.group(1))
        self.process(lines)
        if self.cond:
            self.fail("unterminated conditional")
        clash = self.labels & set(self.numeric)
        if clash:
            self.fail(f"label and constant share a name: {sorted(clash)}")
        return self.render()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("-D", dest="defines", action="append", default=[], metavar="NAME")
    ap.add_argument("input", type=Path)
    args = ap.parse_args()
    try:
        sys.stdout.write(Translator(args.defines).run(args.input))
    except TranslationError as e:
        sys.exit(f"masm2gas: {e}")


if __name__ == "__main__":
    main()
