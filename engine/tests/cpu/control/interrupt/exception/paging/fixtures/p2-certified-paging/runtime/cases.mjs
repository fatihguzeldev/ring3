// each row declares application mappings and a literal first access fault.
export const rows = [
  {id: 'read-zero', label: 'MOV unmapped page records known CR2 zero',
    code: 'a100000000ebfe', kind: 'read', address: 0, linear: 0, error: 4, profiles: 'four',
    rights: [[0, null]], pages: [[0, 3]], continuation: true},
  {id: 'store-late-protection', label: 'cross-page store readonly fault preserves both pages',
    code: 'c705fe5f000078563412ebfe', kind: 'store', address: 0x5ffe, linear: 0x6000, error: 7,
    profiles: 'four', rights: [[0x6000, 1]], pages: [[0x5000, 3], [0x6000, 3]]},
  {id: 'exchange-late-protection', label: 'implicit LOCK XCHG uses Write fault without publishing EAX or RAM',
    code: '8705fe5f0000ebfe', kind: 'exchange', address: 0x5ffe, linear: 0x6000, error: 7,
    profiles: 'four', rights: [[0x6000, 1]], pages: [[0x5000, 3], [0x6000, 3]]},
  {id: 'store-not-present', label: 'store unmapped fault distinguishes absence from protection',
    code: 'c7050060000078563412ebfe', kind: 'store', address: 0x6000, linear: 0x6000, error: 6,
    rights: [[0x6000, null]], pages: [[0x6000, 3]]},
  {id: 'wrapped-read-low', label: 'nonflat valid MOV wraps high to zero and faults at zero',
    code: 'a10e000000ebfe', kind: 'read', address: 0xfffffffe, logical: 14, linear: 0, error: 4,
    dsBase: 0xfffffff0, dsLimit: 0xff, rights: [[0, null]], pages: [[0xfffff000, 3], [0, 3]]},
  {id: 'wrapped-read-high-priority', label: 'first unmapped high byte outranks later unmapped low page',
    code: 'a10e000000ebfe', kind: 'read', address: 0xfffffffe, logical: 14,
    linear: 0xfffffffe, error: 4, dsBase: 0xfffffff0, dsLimit: 0xff,
    rights: [[0xfffff000, null], [0, null]], pages: [[0xfffff000, 3], [0, 3]]},
  {id: 'unmapped-read', label: 'default application memory produces PF without opt-in',
    code: 'a100600000ebfe', kind: 'read', address: 0x6000, linear: 0x6000, error: 4, rights: [], pages: []},
  {id: 'store-permission', label: 'default readonly mapping produces a precise Write fault',
    code: 'c7050060000078563412ebfe', kind: 'store', address: 0x6000, linear: 0x6000, error: 7,
    rights: [], pages: [[0x6000, 1]]},
  {id: 'gather-dword', label: 'Gather retains completed lane and remaining mask across wrapped fault',
    code: 'c4e269900420ebfe', kind: 'gather', width: 4, geometry: 1, profiles: 'four',
    address: 0xfffffffe, linear: 0, error: 4, dsBase: 0xfffffff0, dsLimit: 0xff,
    rights: [[0, null]], pages: [[0xfffff000, 3], [0, 3]]},
  {id: 'gather-qword', label: 'qword Gather retains its eight-byte fault geometry',
    code: 'c4e2ed900420ebfe', kind: 'gather', width: 8, geometry: 4, profiles: 'both',
    address: 0xfffffffe, linear: 0, error: 4, dsBase: 0xfffffff0, dsLimit: 0xff,
    rights: [[0, null]], pages: [[0xfffff000, 3], [0, 3]]},
];
export const protocolRows = [
  {...rows[0], id: 'protocol-width', label: 'malformed span after real PF rejects before CPU publication',
    profiles: undefined, continuation: false, mutation: 'width', protocol: 'shape'},
  {...rows[0], id: 'protocol-witness', label: 'changed PF code cannot replace the actual native occurrence',
    profiles: undefined, continuation: false, mutation: 'code', protocol: 'witness'},
  {...rows[0], id: 'protocol-forged', label: 'successful real read forged as PF has no native occurrence',
    profiles: undefined, continuation: false, rights: [[0, 3]], success: true, mutation: 'forge', protocol: 'pairing'},
  {...rows[0], id: 'protocol-downgraded', label: 'real PF downgraded to valid legacy success cannot retire',
    profiles: undefined, continuation: false, mutation: 'downgrade', protocol: 'pairing'},
];
export function profiles(row) {
  return row.profiles === 'four' ? [[false, false], [false, true], [true, false], [true, true]] :
    row.profiles === 'both' ? [[false, false], [true, false]] : [[false, false]];
}
