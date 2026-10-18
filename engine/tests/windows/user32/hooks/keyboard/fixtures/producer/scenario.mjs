import assert from "node:assert/strict";
import { builder } from "../../../../../../support/pe32/image.mjs";

const u32 = (value) => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return [...bytes];
};

export function image() {
  const b = builder(
    [
      ["kernel32.dll", ["ExitProcess"]],
      ["user32.dll", [
        "RegisterClassA", "CreateWindowExA", "ShowWindow", "SetFocus", "GetFocus",
        "SetWindowsHookExA", "CallNextHookEx", "UnhookWindowsHookEx",
        "PeekMessageA", "GetMessageA", "PostMessageA",
      ]],
    ],
    { callbacks: [
      { words: 3, result: 0, record: "older" },
      { words: 3, result: 0, record: "newer" },
    ] },
  );
  for (const name of ["window", "olderHook", "newerHook", "suppressVk", "chainResult"])
    b.allocate(name);
  b.allocate("message", Array(7).fill(0));
  const wndproc = 0x404d00;
  const className = b.string("className", "Ring3KeyboardHookFixture");
  const title = b.string("title", "authored keyboard hook control");
  const wndclass = b.data("wndclass", [0, wndproc, 0, 0, 0x400000, 0, 0, 0, 0, className]);
  const call = (...args) => b.external(...args);
  const sentinel = () => {
    for (let index = 0; index < 7; index++) b.write(b.addresses.message + index * 4, 0xa5a5a5a5);
    b.write(b.addresses.chainResult, 0xffffffff);
  };
  const peek = (hook, remove, expected) => {
    sentinel();
    call("PeekMessageA", 0x20015,
      [b.addresses.message, b.ref("window"), 0x100, 0x101, remove], expected, undefined, hook);
  };
  call("RegisterClassA", 0x20001, [wndclass], "atom");
  call("CreateWindowExA", 0x20007,
    [0, className, title, 0, 0, 0, 16, 16, 0, 0, 0x400000, 0], "pointer", "window");
  call("ShowWindow", 0x20012, [b.ref("window"), 5], 0);
  call("SetFocus", 0x2000e, [b.ref("window")], "handle");
  call("GetFocus", 0x2000d, [], { ref: "window" });
  call("CallNextHookEx", 0x200d3, [0, 0, 0, 0], 0, undefined, "outside-chain");
  call("SetWindowsHookExA", 0x2011f, [2, b.callbacks[0].entry, 0, 1], "pointer", "olderHook");
  call("SetWindowsHookExA", 0x2011f, [2, b.callbacks[1].entry, 0, 1], "pointer", "newerHook");
  peek("empty", 1, 0);
  peek("peek-down", 0, 1);
  peek("peek-again", 0, 1);
  peek("remove-down", 1, 1);
  peek("removed-empty", 1, 0);
  peek("remove-up", 1, 1);
  b.write(b.addresses.suppressVk, 0x42);
  peek("suppress-noremove", 0, 0);
  b.write(b.addresses.suppressVk, 0);
  peek("suppressed-empty", 1, 0);
  b.write(b.addresses.suppressVk, 0x43);
  peek("skip-suppressed", 1, 1);
  b.write(b.addresses.suppressVk, 0);
  sentinel();
  call("GetMessageA", 0x20017,
    [b.addresses.message, b.ref("window"), 0x100, 0x101], 1, undefined, "waiting-get");
  call("PostMessageA", 0x2001c, [b.ref("window"), 0x100, 0x51, 0x12345678], 1);
  peek("posted-negative", 1, 1);
  call("UnhookWindowsHookEx", 0x20125, [b.ref("newerHook")], 1);
  call("UnhookWindowsHookEx", 0x20125, [b.ref("olderHook")], 1);
  peek("unhooked", 1, 1);
  call("ExitProcess", 0x10003, [0], "exit");
  const guest = b.seal();
  assert.ok(guest.calls.at(-1).end + 8 < wndproc - guest.entry);
  const offset = (address) => 0x200 + address - guest.entry;
  guest.bytes.set([0xb8, 1, 0, 0, 0, 0xc2, 16, 0], offset(wndproc));
  const next = guest.calls.find((item) => item.name === "CallNextHookEx");
  const iat = guest.bytes.readUInt32LE(offset(guest.entry + next.end) - 4);
  const record = (callback) => [...guest.bytes.subarray(offset(callback.entry), offset(callback.entry) + callback.length - 8)];
  const older = [
    ...record(guest.callbacks[0]),
    0x8b, 0x44, 0x24, 8,
    0x3b, 0x05, ...u32(b.addresses.suppressVk),
    0x75, 8,
    0xb8, ...u32(0x51), 0xc2, 12, 0,
    0x31, 0xc0, 0xc2, 12, 0,
  ];
  const newer = [
    ...record(guest.callbacks[1]),
    0xff, 0x74, 0x24, 12,
    0xff, 0x74, 0x24, 12,
    0xff, 0x74, 0x24, 12,
    0x68, ...u32(0xdeadbeef),
    0xff, 0x15, ...u32(iat),
    0xa3, ...u32(b.addresses.chainResult),
    0xc2, 12, 0,
  ];
  for (const [index, body] of [older, newer].entries()) {
    assert.ok(body.length < 0xe0);
    guest.bytes.set(body, offset(guest.callbacks[index].entry));
    guest.callbacks[index].length = body.length;
  }
  return { ...guest, wndproc };
}
