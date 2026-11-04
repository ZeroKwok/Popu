"""Inspect Windows wheel binaries without depending on Visual Studio tools."""
from __future__ import annotations

import re
import struct

WINDOWS_TARGET = "x86_64-pc-windows-msvc"


def imported_dlls(data: bytes) -> set[str]:
    """Read normal and delay imports from a bounded AMD64 PE32+ image."""
    def unpack(format: str, offset: int):
        if offset < 0:
            raise ValueError("negative PE offset")
        return struct.unpack_from(format, data, offset)[0]

    try:
        if data[:2] != b"MZ":
            raise ValueError("runtime must be a Windows PE executable")
        pe = unpack("<I", 0x3C)
        if data[pe:pe + 4] != b"PE\0\0" or unpack("<H", pe + 4) != 0x8664:
            raise ValueError("runtime must target Windows x64 (AMD64)")
        optional = pe + 24
        optional_size = unpack("<H", pe + 20)
        if unpack("<H", optional) != 0x20B or optional_size < 128:
            raise ValueError("runtime must contain a PE32+ optional header")
        sections = optional + optional_size
        count = unpack("<H", pe + 6)
        if not 0 < count <= 96 or sections + count * 40 > len(data):
            raise ValueError("invalid PE section table")

        def offset(rva: int) -> int:
            if 0 <= rva < min(unpack("<I", optional + 60), len(data)):
                return rva
            for index in range(count):
                section = sections + index * 40
                virtual = unpack("<I", section + 12)
                size = unpack("<I", section + 16)
                raw = unpack("<I", section + 20)
                if virtual <= rva < virtual + size and raw + rva - virtual < len(data):
                    return raw + rva - virtual
            raise ValueError(f"invalid PE RVA: {rva:#x}")

        imports: set[str] = set()
        directories = min(unpack("<I", optional + 108), (optional_size - 112) // 8)
        for index, width in ((1, 20), (13, 32)):
            if index >= directories:
                continue
            rva = unpack("<I", optional + 112 + index * 8)
            size = unpack("<I", optional + 116 + index * 8)
            if not rva:
                continue
            if size < width or size // width > 4096:
                raise ValueError("invalid PE import directory size")
            for item in range(size // width):
                start = offset(rva + item * width)
                descriptor = data[start:start + width]
                if len(descriptor) != width:
                    raise ValueError("truncated PE import descriptor")
                if not any(descriptor):
                    break
                name = unpack("<I", start + (12 if index == 1 else 4))
                if index == 13 and not unpack("<I", start) & 1:
                    name -= unpack("<Q", optional + 24)
                start = offset(name)
                end = data.find(b"\0", start, start + 260)
                if end == -1:
                    raise ValueError("unterminated PE DLL name")
                imports.add(data[start:end].decode("ascii").lower())
            else:
                raise ValueError("unterminated PE import directory")
        return imports
    except (struct.error, UnicodeError) as error:
        raise ValueError(f"malformed Windows executable: {error}") from error


def verify_portable_binary(data: bytes) -> set[str]:
    imports = imported_dlls(data)
    vc = sorted(name for name in imports if re.match(r"(?:vcruntime|msvcp|msvcr|concrt)\d.*\.dll$", name))
    if vc:
        raise ValueError(f"Native wheel requires static CRT; found {', '.join(vc)}. Build with `just build-wheel`.")
    return imports
