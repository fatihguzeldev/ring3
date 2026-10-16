import hashlib
import struct

def inspect(path):
    data = path.read_bytes()
    u16 = lambda at: struct.unpack_from("<H", data, at)[0]
    u32 = lambda at: struct.unpack_from("<I", data, at)[0]
    pe = u32(0x3c)
    optional = pe + 24
    sections = [(u32(at + 12), u32(at + 8), u32(at + 20), u32(at + 16)) for at in range(optional + u16(pe + 20), optional + u16(pe + 20) + 40 * u16(pe + 6), 40)]
    def offset(rva):
        if rva < u32(optional + 60):
            return rva
        for virtual, size, raw, stored in sections:
            if virtual <= rva < virtual + max(size, stored):
                return raw + rva - virtual
        raise ValueError(f"invalid PE RVA {rva:x}")
    def string(rva):
        at = offset(rva)
        return data[at:data.index(0, at)].decode("ascii")
    imports = {}
    if u32(optional + 104):
        at = offset(u32(optional + 104))
        while u32(at + 12):
            names = []
            thunk = offset(u32(at) or u32(at + 16))
            while u32(thunk):
                value = u32(thunk)
                names.append("#" + str(value & 65535) if value & 0x80000000 else string(value + 2))
                thunk += 4
            imports[string(u32(at + 12))] = names
            at += 20
    exports = []
    if u32(optional + 96):
        at = offset(u32(optional + 96))
        names = offset(u32(at + 32))
        exports = [string(u32(names + index * 4)) for index in range(u32(at + 24))]
    return {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data), "machine": u16(pe + 4), "imageBase": u32(optional + 28), "entryRva": u32(optional + 16), "tlsDirectory": u32(optional + 96 + 9 * 8), "imports": imports, "exports": exports}
