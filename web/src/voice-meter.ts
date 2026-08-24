// A live level meter for the microphone, so a recording looks like it is
// listening rather than like nothing is happening.
//
// This reads amplitude only. It never keeps a sample, never copies the audio
// anywhere, and holds nothing after stop(). The recording itself still goes
// only where the loopback client sends it.
//
// Everything here builds DOM nodes. Nothing assembles an HTML string, for the
// same reason the markdown renderer does not.

const BAR_COUNT = 28;
// Below this the bars sit at their resting height, so room noise does not make
// the meter twitch while nobody is speaking.
const NOISE_FLOOR = 0.012;
// Loud speech lands well under 1.0 in RMS terms, so the scale is stretched to
// make normal talking fill the meter rather than nudge it.
const FULL_SCALE = 0.28;
const REST = 0.08;

export interface VoiceMeterEnvironment {
  AudioContext?: typeof AudioContext;
  requestAnimationFrame?: (callback: FrameRequestCallback) => number;
  cancelAnimationFrame?: (handle: number) => void;
}

export interface VoiceMeter {
  start(stream: MediaStream): void;
  stop(): void;
}

/** A meter that does nothing, for a browser with no Web Audio. */
const INERT: VoiceMeter = { start: () => {}, stop: () => {} };

export function createVoiceMeter(
  container: HTMLElement | undefined,
  environment: VoiceMeterEnvironment = browserEnvironment(container),
): VoiceMeter {
  const AudioContextClass = environment.AudioContext;
  const raf = environment.requestAnimationFrame;
  const cancelRaf = environment.cancelAnimationFrame;
  // jsdom and older browsers have no Web Audio. A meter is decoration, so its
  // absence must never stop a recording.
  if (!container || !AudioContextClass || !raf || !cancelRaf) return INERT;

  const bars: HTMLElement[] = [];
  const levels = new Array<number>(BAR_COUNT).fill(REST);
  let context: AudioContext | null = null;
  let source: MediaStreamAudioSourceNode | null = null;
  let analyser: AnalyserNode | null = null;
  let frame: number | null = null;

  const buildBars = (): void => {
    if (bars.length) return;
    const document = container.ownerDocument;
    for (let index = 0; index < BAR_COUNT; index += 1) {
      const bar = document.createElement("span");
      bar.className = "voice-meter-bar";
      bar.style.transform = `scaleY(${REST})`;
      container.appendChild(bar);
      bars.push(bar);
    }
  };

  const render = (): void => {
    for (let index = 0; index < bars.length; index += 1) {
      bars[index].style.transform = `scaleY(${levels[index].toFixed(3)})`;
    }
  };

  const draw = (): void => {
    if (!analyser) return;
    const samples = new Uint8Array(analyser.fftSize);
    analyser.getByteTimeDomainData(samples);

    // Root mean square of the waveform around its 128 centre point.
    let sum = 0;
    for (const sample of samples) {
      const centred = (sample - 128) / 128;
      sum += centred * centred;
    }
    const rms = Math.sqrt(sum / samples.length);
    const level =
      rms <= NOISE_FLOOR ? REST : Math.min(1, REST + (rms - NOISE_FLOOR) / FULL_SCALE);

    // Newest level enters on the right and the rest shift left, so the meter
    // scrolls the way a voice note does.
    levels.shift();
    levels.push(level);
    render();
    frame = raf(draw);
  };

  const stop = (): void => {
    if (frame !== null) {
      cancelRaf(frame);
      frame = null;
    }
    source?.disconnect();
    analyser?.disconnect();
    source = null;
    analyser = null;
    void context?.close().catch(() => {});
    context = null;
    levels.fill(REST);
    render();
    container.hidden = true;
  };

  const start = (stream: MediaStream): void => {
    stop();
    try {
      buildBars();
      context = new AudioContextClass();
      analyser = context.createAnalyser();
      analyser.fftSize = 1024;
      // The meter should follow the voice closely, so smoothing stays low.
      analyser.smoothingTimeConstant = 0.6;
      source = context.createMediaStreamSource(stream);
      source.connect(analyser);
      container.hidden = false;
      frame = raf(draw);
    } catch (error) {
      // A meter is not worth failing a recording over.
      console.error("voice meter unavailable", error);
      stop();
    }
  };

  return { start, stop };
}

function browserEnvironment(container?: HTMLElement): VoiceMeterEnvironment {
  // The meter draws into the container's own document, so that document's
  // window is where its frames and Web Audio come from. There may be no
  // window at all, under a test runner, and an inert meter is the answer.
  const view =
    container?.ownerDocument?.defaultView ??
    (typeof window === "undefined" ? undefined : window);
  if (!view) return {};
  return {
    AudioContext: view.AudioContext,
    requestAnimationFrame: view.requestAnimationFrame?.bind(view),
    cancelAnimationFrame: view.cancelAnimationFrame?.bind(view),
  };
}
