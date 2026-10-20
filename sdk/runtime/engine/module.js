export async function loadEngineModule(url, signal, compiled) {
  if (
    compiled !== undefined &&
    (!(compiled?.module instanceof WebAssembly.Module) || !/^[a-f0-9]{64}$/.test(compiled.hash))
  )
    throw new TypeError("invalid compiled engine artifact");
  signal?.throwIfAborted();
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`engine HTTP ${response.status}`);
  const bytes = await response.arrayBuffer();
  signal?.throwIfAborted();
  const hash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))]
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("");
  signal?.throwIfAborted();
  const module = compiled?.hash === hash ? compiled.module : await WebAssembly.compile(bytes);
  signal?.throwIfAborted();
  return { module, hash };
}
