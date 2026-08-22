/**
 * How wide the three columns are, and who decides.
 *
 * The app shell is one CSS grid: sessions, conversation, artifact. Both outer
 * columns are sized by a custom property, so making them draggable is a
 * matter of writing a number onto the root element rather than of moving any
 * markup around. That is the whole trick, and everything here exists to keep
 * the number honest.
 *
 * Two rules shape the rest. A pane can never be dragged to nothing and never
 * over the conversation: the conversation is the thing being read, and a
 * layout control that can hide it is a way to break the page by accident. And
 * the handle answers the keyboard, because a control that only a pointer can
 * reach is a control some people do not have.
 *
 * Nothing here writes HTML. It sets custom properties, dataset keys and ARIA
 * attributes, which is all the layout needs.
 */

/** The narrowest the sessions sidebar may be dragged. */
export const SIDEBAR_MIN = 180;
/** The widest, before the conversation gets a say. */
export const SIDEBAR_MAX = 480;
/** The narrowest the artifact pane may be dragged. */
export const ARTIFACTS_MIN = 300;
/** The widest, before the conversation gets a say. */
export const ARTIFACTS_MAX = 1000;
/**
 * What the conversation keeps whatever else happens.
 *
 * The reading column is 46rem at its widest and this is well under that, so a
 * pane dragged to its limit crowds the conversation without making it
 * unusable. Below this the messages start wrapping every few words.
 */
export const MAIN_MIN = 420;

/** How far one arrow key moves a handle, and how far with Shift held. */
export const STEP = 16;
export const BIG_STEP = 64;

/** The smallest and largest a pane may be, right now. */
export interface Bounds {
  min: number;
  max: number;
}

/** How much room there is to divide up. */
export interface Room {
  /** The window's inner width, in CSS pixels. */
  viewport: number;
  /** How wide the other side pane is, or 0 when it is closed. */
  other: number;
}

function ceilingFor(hardMax: number, room: Room, floor: number): number {
  const left = room.viewport - room.other - MAIN_MIN;
  const ceiling = Math.min(hardMax, left);
  // A window too small to satisfy both the pane and the conversation is a
  // window where the minimum wins. The narrow layouts take over well before
  // this happens, so it is a guard and not a case anyone should meet.
  return ceiling < floor ? floor : ceiling;
}

export function sidebarBounds(room: Room): Bounds {
  return { min: SIDEBAR_MIN, max: ceilingFor(SIDEBAR_MAX, room, SIDEBAR_MIN) };
}

export function artifactsBounds(room: Room): Bounds {
  return { min: ARTIFACTS_MIN, max: ceilingFor(ARTIFACTS_MAX, room, ARTIFACTS_MIN) };
}

/** Put a width inside its bounds. A width that is not a number becomes the minimum. */
export function clampTo(px: number, bounds: Bounds): number {
  if (!Number.isFinite(px)) {
    return bounds.min;
  }
  return Math.min(bounds.max, Math.max(bounds.min, Math.round(px)));
}

/* ------------------------------------------------------------------ */
/* what survives a reload                                              */
/* ------------------------------------------------------------------ */

const KEY_SIDEBAR = "zorp.layout.sidebar";
const KEY_ARTIFACTS = "zorp.layout.artifacts";
const KEY_COLLAPSED = "zorp.layout.sidebarCollapsed";

/** The saved layout. A null width means "whatever the stylesheet says". */
export interface SavedLayout {
  sidebar: number | null;
  artifacts: number | null;
  collapsed: boolean;
}

/**
 * The store, or null when there is not one.
 *
 * Reading `localStorage` throws outright in some privacy modes, so even
 * asking for it is wrapped. A page with no store still works; it just starts
 * at the default widths every time.
 */
export function layoutStore(): Storage | null {
  try {
    const store = window.localStorage;
    // Touching it is the only way to find out whether it really answers.
    store.getItem(KEY_SIDEBAR);
    return store;
  } catch {
    return null;
  }
}

function readNumber(store: Storage, key: string): number | null {
  const raw = store.getItem(key);
  if (raw === null) {
    return null;
  }
  const value = Number(raw);
  return Number.isFinite(value) && value > 0 ? value : null;
}

export function readLayout(store: Storage | null): SavedLayout {
  if (!store) {
    return { sidebar: null, artifacts: null, collapsed: false };
  }
  try {
    return {
      sidebar: readNumber(store, KEY_SIDEBAR),
      artifacts: readNumber(store, KEY_ARTIFACTS),
      collapsed: store.getItem(KEY_COLLAPSED) === "yes",
    };
  } catch {
    return { sidebar: null, artifacts: null, collapsed: false };
  }
}

/** Save one width. Null forgets it, so the next load takes the default. */
export function saveWidth(
  store: Storage | null,
  which: "sidebar" | "artifacts",
  px: number | null,
): void {
  if (!store) {
    return;
  }
  const key = which === "sidebar" ? KEY_SIDEBAR : KEY_ARTIFACTS;
  try {
    if (px === null) {
      store.removeItem(key);
    } else {
      store.setItem(key, String(Math.round(px)));
    }
  } catch {
    // A full or refusing store costs the reader their saved widths and
    // nothing else. It is not worth an error card.
  }
}

export function saveCollapsed(store: Storage | null, collapsed: boolean): void {
  if (!store) {
    return;
  }
  try {
    store.setItem(KEY_COLLAPSED, collapsed ? "yes" : "no");
  } catch {
    // Same as above.
  }
}

/* ------------------------------------------------------------------ */
/* the sidebar's collapsed state                                       */
/* ------------------------------------------------------------------ */

/**
 * Collapse or restore the sessions sidebar.
 *
 * A dataset key on the shell and nothing else, because the stylesheet is
 * where a layout decision belongs. The narrow layout ignores it: down there
 * the sidebar is already a drawer, and collapsing a drawer that is shut has
 * no meaning.
 *
 * `toggle` is the topbar button that brings it back, so its `aria-expanded`
 * is set from the same call that changes the state. Two places setting that
 * attribute is how it ends up lying.
 */
export function setSidebarCollapsed(
  app: HTMLElement,
  collapsed: boolean,
  toggle?: HTMLElement | null,
): void {
  if (collapsed) {
    app.dataset.sidebar = "collapsed";
  } else {
    delete app.dataset.sidebar;
  }
  toggle?.setAttribute("aria-expanded", collapsed ? "false" : "true");
}

export function sidebarIsCollapsed(app: HTMLElement): boolean {
  return app.dataset.sidebar === "collapsed";
}

/* ------------------------------------------------------------------ */
/* the handles                                                         */
/* ------------------------------------------------------------------ */

export interface ResizerOptions {
  /** The grabbable element. Carries the separator role and the ARIA values. */
  handle: HTMLElement;
  /** Where the custom property is written. */
  root: HTMLElement;
  /** The custom property this handle drives. */
  property: string;
  /**
   * Which way is wider.
   *
   * `1` for a pane on the left of its handle, where dragging right widens it.
   * `-1` for a pane on the right, where dragging right makes it narrower.
   */
  sign: 1 | -1;
  /** The limits, asked for fresh every time because the window can change. */
  bounds: () => Bounds;
  /**
   * The pane's width as the browser currently has it.
   *
   * Only consulted while no explicit width is set, which is how a drag that
   * starts from the stylesheet's default starts from the right number.
   */
  measure: () => number;
  /** Called whenever a gesture ends. Null means "back to the default". */
  onCommit: (px: number | null) => void;
}

/**
 * One draggable pane edge.
 *
 * Pointer events rather than mouse events, so a pen or a touch works the same
 * way, and pointer capture so a fast drag that outruns the 7px handle keeps
 * going. Arrow keys move it too, Home and End take it to its limits, and a
 * double click gives the pane its default width back.
 */
export class PaneResizer {
  private readonly options: ResizerOptions;
  /** The set width, or null while the stylesheet's default is in force. */
  private width: number | null = null;
  private startX = 0;
  private startWidth = 0;
  private dragging = false;

  constructor(options: ResizerOptions) {
    this.options = options;
    const { handle } = options;
    handle.addEventListener("pointerdown", this.onPointerDown);
    handle.addEventListener("keydown", this.onKeyDown);
    handle.addEventListener("dblclick", this.onDoubleClick);
    // A separator with no values on it is a separator that tells a screen
    // reader nothing, so it says where it stands from the moment it exists.
    this.describe();
  }

  /** The width in force, measured if the stylesheet is still deciding. */
  get current(): number {
    return this.width ?? this.options.measure();
  }

  /**
   * Set the width, or hand it back to the stylesheet with null.
   *
   * Nothing is persisted from here; that is `onCommit`'s job, so that a drag
   * writes to the store once at the end and not on every frame.
   */
  set(px: number | null): void {
    if (px === null) {
      this.width = null;
      this.options.root.style.removeProperty(this.options.property);
    } else {
      this.width = clampTo(px, this.options.bounds());
      this.options.root.style.setProperty(this.options.property, `${this.width}px`);
    }
    this.describe();
  }

  /** Re-clamp what is already set, for when the window changed size. */
  reclamp(): void {
    if (this.width !== null) {
      this.set(this.width);
    } else {
      this.describe();
    }
  }

  /** Tell assistive technology where the handle stands. */
  describe(): void {
    const { handle } = this.options;
    const bounds = this.options.bounds();
    handle.setAttribute("aria-valuemin", String(Math.round(bounds.min)));
    handle.setAttribute("aria-valuemax", String(Math.round(bounds.max)));
    handle.setAttribute("aria-valuenow", String(Math.round(this.current)));
  }

  private readonly onPointerDown = (event: Event): void => {
    const point = event as PointerEvent;
    // Only the primary button. A right click here is a context menu.
    if (typeof point.button === "number" && point.button !== 0) {
      return;
    }
    event.preventDefault();
    this.dragging = true;
    this.startX = point.clientX;
    this.startWidth = this.current;
    this.options.root.dataset.resizing = "yes";
    const { handle } = this.options;
    handle.dataset.dragging = "yes";
    if (typeof handle.setPointerCapture === "function" && point.pointerId !== undefined) {
      try {
        handle.setPointerCapture(point.pointerId);
      } catch {
        // Capture is a nicety. Without it the window listeners still work.
      }
    }
    window.addEventListener("pointermove", this.onPointerMove);
    window.addEventListener("pointerup", this.onPointerUp);
    window.addEventListener("pointercancel", this.onPointerUp);
  };

  private readonly onPointerMove = (event: Event): void => {
    if (!this.dragging) {
      return;
    }
    const point = event as PointerEvent;
    const moved = (point.clientX - this.startX) * this.options.sign;
    this.set(this.startWidth + moved);
  };

  private readonly onPointerUp = (): void => {
    if (!this.dragging) {
      return;
    }
    this.dragging = false;
    delete this.options.root.dataset.resizing;
    delete this.options.handle.dataset.dragging;
    window.removeEventListener("pointermove", this.onPointerMove);
    window.removeEventListener("pointerup", this.onPointerUp);
    window.removeEventListener("pointercancel", this.onPointerUp);
    this.options.onCommit(this.width);
  };

  private readonly onKeyDown = (event: Event): void => {
    const key = event as KeyboardEvent;
    const bounds = this.options.bounds();
    const step = key.shiftKey ? BIG_STEP : STEP;
    let next: number | null = null;
    if (key.key === "ArrowLeft") {
      next = this.current - step * this.options.sign;
    } else if (key.key === "ArrowRight") {
      next = this.current + step * this.options.sign;
    } else if (key.key === "Home") {
      next = this.options.sign === 1 ? bounds.min : bounds.max;
    } else if (key.key === "End") {
      next = this.options.sign === 1 ? bounds.max : bounds.min;
    } else if (key.key === "Enter") {
      // The documented way to get the default back without a pointer.
      event.preventDefault();
      this.set(null);
      this.options.onCommit(null);
      return;
    } else {
      return;
    }
    event.preventDefault();
    this.set(next);
    this.options.onCommit(this.width);
  };

  private readonly onDoubleClick = (event: Event): void => {
    event.preventDefault();
    this.set(null);
    this.options.onCommit(null);
  };
}
