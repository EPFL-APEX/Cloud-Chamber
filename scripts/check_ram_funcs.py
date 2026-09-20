#!/usr/bin/env python3
"""Vérifie qu'aucun code exécuté depuis la RAM ne saute vers la flash.

Les routines d'écriture flash de `drivers::flash_rp2040` s'exécutent pendant
que le XIP est coupé : à ce moment-là, aucune instruction en flash n'est
lisible, sur aucun des deux cœurs. Elles sont donc placées en RAM par
`#[unsafe(link_section = ".data.ram_func")]`.

Le piège, c'est qu'une fonction « en RAM » peut parfaitement appeler du code
resté en flash — une fonction générique, une fermeture, ou même un
`core::ptr::read_volatile` non inliné en build debug. Le compilateur ne dit
rien ; l'éditeur de liens pose juste un tremplin
(`__Thumbv6MABSLongThunk...`) en RAM, et ça se termine en hard fault sur la
carte. Les deux cas se sont produits en écrivant ce module.

Ce script relit l'ELF et échoue s'il trouve un tel tremplin.

    python3 scripts/check_ram_funcs.py target/thumbv6m-none-eabi/debug/cloud_chamber
"""

import struct
import sys

RAM_START, RAM_END = 0x20000000, 0x20100000


def ram_symbols(path):
    data = open(path, "rb").read()
    if data[:4] != b"\x7fELF":
        raise SystemExit(f"{path} n'est pas un ELF")

    shoff, = struct.unpack_from("<I", data, 0x20)
    shentsize, = struct.unpack_from("<H", data, 0x2E)
    shnum, = struct.unpack_from("<H", data, 0x30)
    shstrndx, = struct.unpack_from("<H", data, 0x32)

    def section(i):
        return struct.unpack_from("<IIIIIIIIII", data, shoff + i * shentsize)

    shstr = section(shstrndx)[4]

    def sec_name(off):
        return data[shstr + off: data.index(b"\0", shstr + off)].decode()

    symtab = next(section(i) for i in range(shnum) if section(i)[1] == 2)
    strtab = next(section(i) for i in range(shnum) if sec_name(section(i)[0]) == ".strtab")

    off, size, entsize, stro = symtab[4], symtab[5], symtab[9], strtab[4]
    for k in range(size // entsize):
        name_off, value, sym_size = struct.unpack_from("<III", data, off + k * entsize)
        name = data[stro + name_off: data.index(b"\0", stro + name_off)].decode()
        if value and sym_size and RAM_START <= value < RAM_END:
            yield value, sym_size, name


def main(argv):
    if len(argv) != 2:
        raise SystemExit(__doc__)

    thunks = []
    for value, size, name in sorted(ram_symbols(argv[1])):
        if "Thunk" in name:
            thunks.append((value, size, name))
            print(f"  {value:#x} {size:>5} o  {name}   <-- saut hors RAM")

    if thunks:
        print(f"\nECHEC : {len(thunks)} tremplin(s) vers la flash en RAM.")
        print("Une fonction `.data.ram_func` appelle du code reste en flash.")
        return 1

    print("OK : aucun saut hors RAM depuis le code resident.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
