import { builder } from "../../../../../support/pe32/image.mjs";

const words = (value) => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value);
  return [...bytes];
};

export function image() {
  const b = builder(
    [
      ["kernel32.dll", ["ExitProcess"]],
      [
        "user32.dll",
        [
          "RegisterClassA",
          "CreateWindowExA",
          "ShowWindow",
          "SetActiveWindow",
          "SetFocus",
          "GetFocus",
          "GetActiveWindow",
          "PeekMessageA",
        ],
      ],
    ],
    { callbacks: [{ words: 4, result: 1, record: "window" }] },
  );
  for (const name of ["first", "second", "child", "active", "activationCount"]) b.allocate(name);
  b.allocate("message", Array(7).fill(0));
  const className = b.string("className", "Ring3ActivationFixture");
  const title = b.string("title", "authored activation control");
  const wndclass = b.data("wndclass", [
    0,
    b.callbacks[0].entry,
    0,
    0,
    0x400000,
    0,
    0,
    0,
    0,
    className,
  ]);
  const call = (...args) => b.external(...args);
  const create = (output, style = 0, parent = 0) =>
    call(
      "CreateWindowExA",
      0x20007,
      [0, className, title, style, 0, 0, 16, 16, parent, 0, 0x400000, 0],
      "pointer",
      output,
    );
  const peek = (hook, filter = 0x400) =>
    call("PeekMessageA", 0x20015, [b.addresses.message, 0, filter, filter, 0], 0, undefined, hook);
  call("RegisterClassA", 0x20001, [wndclass], "atom");
  create("first");
  call("ShowWindow", 0x20012, [b.ref("first"), 5], 0, undefined, "first-shown");
  create("second");
  call("ShowWindow", 0x20012, [b.ref("second"), 5], 0, undefined, "same-app");
  call("SetActiveWindow", 0x20112, [b.ref("first")], "handle", undefined, "same-app");
  call("GetActiveWindow", 0x20058, [], { ref: "first" }, undefined, "same-app");
  create("child", 0x50000000, b.ref("first"));
  call("SetFocus", 0x2000e, [b.ref("child")], "handle", undefined, "same-app");
  call("GetFocus", 0x2000d, [], { ref: "child" }, undefined, "same-app");
  peek("physical-blur");
  peek("physical-restore");
  peek("duplicate-restore");
  peek("no-posted-app", 0x1c);
  call("ExitProcess", 0x10003, [0], "exit");
  const guest = b.seal(),
    callback = guest.callbacks[0];
  const at = 0x200 + callback.entry - guest.entry;
  const old = [...guest.bytes.subarray(at, at + callback.length)];
  const update = [
    0x83,
    0x7c,
    0x24,
    8,
    0x1c,
    0x75,
    15,
    0x8b,
    0x44,
    0x24,
    12,
    0xa3,
    ...words(b.addresses.active),
    0xff,
    0x05,
    ...words(b.addresses.activationCount),
  ];
  const body = [...old.slice(0, -8), ...update, ...old.slice(-8)];
  guest.bytes.set(body, at);
  callback.length = body.length;
  return guest;
}
