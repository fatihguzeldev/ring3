interface PointerSample {
  clientX: number;
  clientY: number;
  movementX: number;
  movementY: number;
}

export function pointerDelta(
  sample: PointerSample,
  previous: { x: number; y: number } | undefined,
  locked: boolean,
  width: number,
  height: number,
  boundsWidth: number,
  boundsHeight: number,
): [number, number] | undefined {
  if (boundsWidth === 0 || boundsHeight === 0) return undefined;
  let dx: number;
  let dy: number;
  if (locked) {
    dx = sample.movementX;
    dy = sample.movementY;
  } else {
    if (!previous) return undefined;
    dx = sample.clientX - previous.x;
    dy = sample.clientY - previous.y;
  }
  const x = Math.round((dx * width) / boundsWidth);
  const y = Math.round((dy * height) / boundsHeight);
  return x !== 0 || y !== 0 ? [x, y] : undefined;
}
