/**
 * The composer's send control, which is also its stop control.
 *
 * One button, three states. It could have been two buttons, and two buttons is
 * the easier thing to write: no swapping, no label to keep in step. It is the
 * worse thing to use. The composer has room for one control at the end of the
 * text field, that spot is where a person's hand already is, and a stop button
 * parked somewhere else is a stop button you have to look for while the thing
 * you want stopped keeps running.
 *
 * Its own module because the interesting part is the announcement, not the
 * icon. Everything here is set together on purpose: a control that swapped its
 * picture and kept its `aria-label` would be a button that shows a stop square
 * and tells a screen reader it sends a message, which is worse than not having
 * the feature.
 */

export const SEND_LABEL = "Send message";
export const STOP_LABEL = "Stop this turn";
export const STOPPING_LABEL = "Stopping this turn";

/**
 * - `send`: nothing is running, the button sends what is typed.
 * - `stop`: a turn is running and this ends it.
 * - `stopping`: a stop is on its way to the server. Still a stop to look at,
 *   because the run has not ended yet and flipping back to an arrow would say
 *   it had, but not one to press again.
 */
export type SendControlState = "send" | "stop" | "stopping";

export function setSendControl(button: HTMLButtonElement, state: SendControlState): void {
  const stopping = state === "stopping";
  const stops = stopping || state === "stop";
  const label = stopping ? STOPPING_LABEL : stops ? STOP_LABEL : SEND_LABEL;

  button.classList.toggle("is-stop", stops);
  button.setAttribute("aria-label", label);
  button.title = label;
  // Only ever disabled for the moment a stop is in flight. The send button
  // used to be disabled for the whole run, which is exactly the thing this
  // control exists to stop being true.
  button.disabled = stopping;
}
