export const CPU_SHA256 = 'ce21a0e21e2e2c9cc0bae9acfdf4615daf22664a5a3b94e907aea139e9693fa6';
export const PHYSICAL_BYTES = 160 * 1024 * 1024;
export const BACKING_BASE = 16 * 1024 * 1024;
export const PAGE = 4096;
export const PRIVATE_VA = 0xff000000;
export const DIRECTORY = 0x200000;
export const TABLES = DIRECTORY + PAGE;
export const GDT = 0x100000;
export const IDT = GDT + PAGE;
export const TSS = GDT + PAGE * 2;
export const STACK = GDT + PAGE * 3;
export const HANDLERS = GDT + PAGE * 6;
export const PARK = PRIVATE_VA + PAGE * 7;
export const TRAP_PORT = 0xe0;
export const physicalPrivate = address => PRIVATE_VA + address - GDT;
export const X87 = 4236;
export const SIMD = X87 + 128;
export const SEGMENTS = SIMD + 160;
export const YMM = SEGMENTS + 64 + 24;
export const SEGMENT_BASES = YMM + 144;
export const SHIPPING_JIT = [
  [0,0],[1,3],[2,1],[3,250],[4,0],[5,1],[6,0],[7,5],[8,8],
  [10,0],[11,1],[12,1],[13,1],[14,24],[15,0],[16,96],[17,8],[19,0],
  [20,1],[21,0],[22,1],[23,0],[24,0],[25,9],[26,0],[27,32],[28,1],[29,0],
  [30,0xfffff],[31,1],
];
export const CODEGEN_SWITCHES = [['jit_page_tails',1],['jit_hot_edge_regions',0],['jit_guard_groups',1]];
