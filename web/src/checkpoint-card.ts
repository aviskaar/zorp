/**
 * The checkpoint card: a research checkpoint, drawn.
 *
 * Not the approval card with different words on it, and the difference is
 * the whole reason this file exists. An approval asks whether a tool call
 * may run, and denying it means the call does not run. A checkpoint asks
 * whether a track stays alive, and declining it kills the track: the run
 * record is closed, no write-up is produced for it, and the question has
 * to be rephrased before anything can be asked again. Two questions with
 * consequences that far apart must not share a widget, because the cost of
 * pressing the wrong one is not symmetric.
 *
 * The run is parked until a button is pressed and nothing here presses
 * one. Five minutes with no answer is not a rejection either: the server
 * ends the run with an error and writes no decision, because nobody
 * answering is not the same fact as somebody saying no.
 *
 * Every string lands through `textContent`. The prompt is composed out of
 * a recorded metric, a pre-registered threshold and the attempt's own
 * summary, which is model-authored text, and this is the place a person
 * reads most carefully.
 *
 * Its own module, like `approval-card.ts`, because `main.ts` runs the whole
 * app on import and cannot be loaded from a test.
 */

export type CheckpointOutcome = "kept" | "killed" | "abandoned" | "stopped";

/**
 * How a settled card describes itself.
 *
 * Records rather than nested conditionals, so a new outcome is a compile
 * error here instead of falling into whichever branch was last. Who
 * decided is the distinction they carry: "abandoned" means nobody did and
 * the run ended without writing a decision, "stopped" means the reader
 * ended the run while the question was on screen. Neither is a rejection,
 * and drawing either as one would tell somebody they killed a track they
 * did not kill.
 */
export const CHECKPOINT_TITLES: Record<CheckpointOutcome, string> = {
  kept: "Track kept alive",
  killed: "Track killed",
  abandoned: "Checkpoint unanswered",
  stopped: "Run stopped",
};

export const CHECKPOINT_NOTES: Record<CheckpointOutcome, string> = {
  kept: "You kept this track alive, so the run carried on.",
  killed: "You killed this track. No write-up is produced for it.",
  abandoned:
    "The run ended before this was answered, so no decision was recorded and the track is untouched.",
  stopped:
    "You stopped the run while this was waiting, so no decision was recorded and the track is untouched.",
};

/**
 * What declining this particular checkpoint costs, said before it is
 * pressed.
 *
 * The pre-registration checkpoint kills a track that has no evidence in it
 * yet, which costs an afternoon. The post-attempt one kills a track that
 * does, which throws away attempts that already ran. A card that rendered
 * both the same way would be hiding the difference at the moment it
 * matters.
 */
export function checkpointStake(kind: string): string {
  if (kind === "investigate-prereg") {
    return "This commits the metric and the kill threshold before the first attempt runs. Declining kills the track before it has any evidence in it.";
  }
  if (kind === "investigate") {
    return "This is the checkpoint after an attempt. Declining kills the track, and the attempts that have already run stay in the record with it.";
  }
  return "Declining kills the track.";
}

export interface CheckpointCard {
  root: HTMLDetailsElement;
  keep: HTMLButtonElement;
  kill: HTMLButtonElement;
  /** Turn both buttons on or off together. */
  enable(on: boolean): void;
  /** The line under the buttons: progress, or why the last click failed. */
  note(text: string): void;
  /** Decided. The buttons go, the head says the outcome, the card folds. */
  settle(outcome: CheckpointOutcome): void;
}

export function checkpointCard(
  doc: Document,
  kind: string,
  prompt: string,
  icon?: Node,
): CheckpointCard {
  const root = doc.createElement("details");
  root.className = "card card-checkpoint";
  // Open while it waits, because a person must see what they are deciding
  // on. It folds to its head once settled.
  root.open = true;

  const head = doc.createElement("summary");
  head.className = "card-head";
  if (icon) {
    head.append(icon);
  }
  const title = doc.createElement("span");
  title.className = "card-title";
  title.textContent = "Checkpoint";
  head.append(title);
  const kindLabel = doc.createElement("span");
  kindLabel.className = "checkpoint-kind";
  kindLabel.textContent = kind;
  head.append(kindLabel);
  root.append(head);

  const stake = doc.createElement("p");
  stake.className = "checkpoint-stake";
  stake.textContent = checkpointStake(kind);
  root.append(stake);

  const body = doc.createElement("pre");
  body.className = "checkpoint-prompt";
  body.textContent = prompt;
  root.append(body);

  const buttons = doc.createElement("div");
  buttons.className = "checkpoint-buttons";
  const keep = doc.createElement("button");
  keep.type = "button";
  keep.className = "checkpoint-keep";
  keep.textContent = "Keep the track alive";
  const kill = doc.createElement("button");
  kill.type = "button";
  kill.className = "checkpoint-kill";
  kill.textContent = "Kill the track";
  buttons.append(keep, kill);
  root.append(buttons);

  const note = doc.createElement("p");
  note.className = "checkpoint-note";
  root.append(note);

  return {
    root,
    keep,
    kill,
    enable(on: boolean): void {
      keep.disabled = !on;
      kill.disabled = !on;
    },
    note(text: string): void {
      note.textContent = text;
    },
    settle(outcome: CheckpointOutcome): void {
      title.textContent = CHECKPOINT_TITLES[outcome];
      note.textContent = CHECKPOINT_NOTES[outcome];
      buttons.remove();
      root.dataset.outcome = outcome;
      root.open = false;
    },
  };
}
