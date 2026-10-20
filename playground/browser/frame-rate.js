const states = {idle:'not running',ready:'not running',checking:'checking',starting:'starting',
  loading:'loading',paused:'paused',stopped:'stopped',exited:'exited',error:'unavailable'};

export function frameRateLabel(info, elapsed = 0) {
  if (!['running','waiting'].includes(info?.state))
    return `— FPS · ${Object.hasOwn(states,info?.state) ? states[info.state] : 'unavailable'}`;
  if (elapsed >= 2000) return '— FPS · waiting for update';
  const value = info.presentation;
  if (!value?.frames) return '— FPS · no completed frames';
  if (Number.isFinite(value.sinceLastPresentMs) && value.sinceLastPresentMs >= 2000)
    return '— FPS · no recent frames';
  if (!Number.isInteger(value.samples) || value.samples < 1 ||
      !Number.isFinite(value.sinceLastPresentMs) || value.sinceLastPresentMs < 0 ||
      !Number.isFinite(value.intervalFps) || value.intervalFps <= 0 ||
      !Number.isFinite(value.meanIntervalMs) || value.meanIntervalMs <= 0)
    return '— FPS · waiting for frames';
  const fps = value.intervalFps < 0.1 ? '<0.1' : value.intervalFps.toFixed(1);
  const milliseconds = value.meanIntervalMs < 0.1 ? '<0.1' : value.meanIntervalMs.toFixed(1);
  return `${fps} FPS · ${milliseconds} ms`;
}
