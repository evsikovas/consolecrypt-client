#!/usr/bin/env python3
"""Check ARM64 ELF mapping and RELRO protection boundaries for 16 KiB pages."""
import struct
import sys
import zipfile

PAGE = 16384


def verify_relro(writable, relros):
    # Android rounds RELRO protection outwards to whole pages. An unaligned
    # end is valid when RELRO covers the whole RW segment (e.g. Flutter's
    # 64 KiB-aligned engine); it must not cover any still-writable bytes.
    # See bionic/linker/linker_phdr_16kib_compat.cpp: phdr_table_get_relro_min_align.
    for start, end in relros:
        protected_start = start // PAGE * PAGE
        protected_end = (end + PAGE - 1) // PAGE * PAGE
        for rw_start, rw_end in writable:
            for lo, hi in ((rw_start, min(rw_end, start)), (max(rw_start, end), rw_end)):
                assert max(lo, protected_start) >= min(hi, protected_end), 'RELRO overlaps writable data'


def verify_elf(data, name):
    assert data[:6] == b'\x7fELF\x02\x01', f'Not little-endian ELF64: {name}'
    assert struct.unpack_from('<H', data, 18)[0] == 183, f'Not AArch64: {name}'
    offset = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    loads, writable, relros = 0, [], []
    for i in range(count):
        kind, flags, file_offset, address, _, _, mem_size, alignment = struct.unpack_from(
            '<IIQQQQQQ', data, offset + i * size)
        if kind == 1:
            loads += 1
            assert alignment >= PAGE and alignment & (alignment - 1) == 0, f'Invalid LOAD alignment: {name}'
            assert (address - file_offset) % PAGE == 0, f'Incongruent LOAD offset: {name}'
            if flags & 2:
                writable.append((address, address + mem_size))
        if kind == 0x6474E552:
            relros.append((address, address + mem_size))
    assert loads, f'ELF has no LOAD segments: {name}'
    verify_relro(writable, relros)


def verify_apk(path):
    with zipfile.ZipFile(path) as apk:
        libraries = [n for n in apk.namelist() if n.startswith('lib/') and n.endswith('.so')]
        for required in ('libcc_bridge.so', 'libflutter.so', 'libapp.so'):
            assert f'lib/arm64-v8a/{required}' in libraries, f'{required} missing'
        for name in libraries:
            assert name.startswith('lib/arm64-v8a/'), f'Unexpected ABI: {name}'
            verify_elf(apk.read(name), name)
            print(f'16 KiB ELF layout verified: {name}')


if __name__ == '__main__':
    verify_apk(sys.argv[1])
