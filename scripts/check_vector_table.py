#!/usr/bin/env python3
"""Vérifie que `TIMER_IRQ_0` est bien branché dans la table de vecteurs.

L'ISR de scrutation de l'encodeur vit dans la bibliothèque
(`ui::console`), pas dans chaque binaire. `#[interrupt]` en fait un symbole
fort censé écraser l'entrée faible que `cortex-m-rt` a posée dans la table
de vecteurs — mais un symbole fort qui dort dans une archive n'écrase rien.
L'éditeur de liens ne tire un objet de `libCloud_Chamber.rlib` que si
quelque chose y fait référence.

Ça marche ici parce que `start_encoder` est dans le même objet que l'ISR,
et que les binaires l'appellent. Ça ne se voit à la compilation ni d'une
façon ni de l'autre : si l'objet n'était pas tiré, le binaire se lierait
sans un mot et le `DefaultHandler` prendrait l'interruption sur la carte —
l'encodeur simplement mort.

Le piège en écrivant ce script : comparer l'entrée de la table au symbole
`TIMER_IRQ_0` ne prouve **rien**. Quand l'ISR manque, `cortex-m-rt` laisse
un alias faible de ce nom, de taille nulle, pointant sur `DefaultHandler` —
et la comparaison réussit trivialement. Vérifié en retirant l'ISR : le
script disait OK. Il faut donc deux contrôles qui, eux, discriminent :

1. l'entrée ne pointe pas sur `DefaultHandler` ;
2. un symbole de `ui::console` porte bien le corps de l'ISR (provenance).

    python3 scripts/check_vector_table.py target/thumbv6m-none-eabi/debug/ui_test ...

Sur RP2040 (Cortex-M0+), TIMER_IRQ_0 est l'IRQ 0 : entrée 16 de la table,
soit l'offset 0x40 depuis son début.
"""

import struct
import sys

TIMER_IRQ_0_INDEX = 16


def elf(path):
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

    def name_at(base, off):
        return data[base + off: data.index(b"\0", base + off)].decode()

    sections = {name_at(shstr, section(i)[0]): section(i) for i in range(shnum)}

    symtab = next(section(i) for i in range(shnum) if section(i)[1] == 2)
    stro = sections[".strtab"][4]
    symbols = {}
    off, size, entsize = symtab[4], symtab[5], symtab[9]
    for k in range(size // entsize):
        name_off, value, sym_size = struct.unpack_from("<III", data, off + k * entsize)
        name = name_at(stro, name_off)
        if name:
            symbols.setdefault(name, (value, sym_size))

    return data, sections, symbols


ISR_OWNER = ("ui", "console", "__cortex_m_rt_TIMER_IRQ_0")


def check(path):
    data, sections, symbols = elf(path)

    if ".vector_table" not in sections:
        print(f"{path} : ECHEC — pas de section .vector_table")
        return False
    vt = sections[".vector_table"]
    vt_off, vt_size = vt[4], vt[5]

    byte_off = TIMER_IRQ_0_INDEX * 4
    if byte_off + 4 > vt_size:
        print(f"{path} : ECHEC — table de vecteurs trop courte ({vt_size} octets)")
        return False

    entry, = struct.unpack_from("<I", data, vt_off + byte_off)
    default = symbols.get("DefaultHandler", (None, 0))[0]

    # 1. L'entrée ne doit pas retomber sur le gestionnaire par défaut.
    if default is not None and entry & ~1 == default & ~1:
        print(f"{path} : ECHEC — IRQ 0 pointe sur DefaultHandler (0x{entry:08x}).")
        print("        L'objet de ui::console n'a pas ete tire de l'archive :")
        print("        l'encodeur serait mort sur la carte, sans un mot au build.")
        return False

    # 2. Le corps de l'ISR doit venir de `ui::console` et de nulle part
    #    ailleurs — un autre module qui reprendrait l'IRQ passerait le
    #    contrôle 1 sans que rien ne le signale.
    owner = next(
        (n for n in symbols if all(part in n for part in ISR_OWNER)),
        None,
    )
    if owner is None:
        print(f"{path} : ECHEC — aucun symbole d'ISR TIMER_IRQ_0 issu de ui::console")
        return False

    print(f"{path} : OK — IRQ 0 -> 0x{entry:08x}, corps dans ui::console")
    return True


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    return 0 if all([check(p) for p in argv[1:]]) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
