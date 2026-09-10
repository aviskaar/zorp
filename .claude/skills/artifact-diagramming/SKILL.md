---
name: artifact-diagramming
description: How to hand-author an SVG diagram or chart for zorp's side pane, where scripts do not run and no chart library will load. Use when drawing an architecture diagram, a data flow, a state machine, a sequence, or a chart.
---

# Drawing a diagram

zorp's browser shows `.svg` files, and `.html` files with inline SVG, in its
side pane. **Scripts do not run there and no library will load**, so every
diagram is one you write by hand. See `artifact-design` for the rest of that
constraint.

Hand-authored SVG is not a downgrade here. A diagram worth showing has about
a dozen shapes in it, and a dozen shapes is less markup than the call that
would generate them.

## The shape of the file

```html
<figure>
  <svg viewBox="0 0 640 320" role="img" aria-labelledby="t d">
    <title id="t">How a turn reaches the model</title>
    <desc id="d">The browser posts to zorp-web, which seeds from the store and calls the provider.</desc>
    ...
  </svg>
  <figcaption>Every turn is rebuilt from the store, never from memory.</figcaption>
</figure>
```

- `viewBox` and no `width`/`height`, plus `svg { max-width: 100%; height:
  auto }`. That is the whole of making it responsive.
- `<title>` and `<desc>` with `aria-labelledby`, because a diagram nobody can
  read aloud is a diagram half the readers do not get.
- A `<figcaption>` saying what the diagram is *for*. If the caption is
  "Diagram", the diagram is not finished.

## Colour

Use `currentColor` for strokes and text and inherit from the page. A diagram
with `stroke="#333"` disappears on a dark background, and the pane follows
the browser's colour scheme rather than the app's.

```css
svg { color: var(--fg); }
.node { fill: none; stroke: currentColor; stroke-width: 1.5; }
.label { fill: currentColor; font: 500 13px ui-sans-serif, system-ui, sans-serif; }
.edge { stroke: currentColor; stroke-width: 1.5; fill: none; opacity: 0.55; }
.accent { stroke: var(--accent); }
```

Hardcode a colour only where it carries meaning, and then say what it means
in the caption or a legend. Three meaningful colours is plenty; a colour per
box is decoration.

## Draw the mechanism, not the boxes

The failure mode of a generated diagram is five rounded rectangles in a row
with arrows between them and nothing learned. A diagram earns its place by
showing something the prose cannot say in a sentence.

So put the load-bearing detail on the page:

- **Label the edges**, not only the nodes. "posts JSON", "reads", "one row
  per attempt" is the content; an unlabelled arrow is a claim that two things
  are related.
- **Show direction**, with a marker, and show where it is two-way.
- **Say what crosses the boundary.** A trust boundary or a process boundary
  is worth a dashed line and a note about what passes through it.
- **Show the thing that is surprising.** A retry, a fallback, the one path
  that skips a step.

An arrowhead, once, reused:

```html
<defs>
  <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5"
          markerWidth="6" markerHeight="6" orient="auto-start-reverse">
    <path d="M0 0 L10 5 L0 10 z" fill="currentColor" />
  </marker>
</defs>
<path class="edge" marker-end="url(#arrow)" d="M120 60 H260" />
```

Give every `id` a prefix specific to this diagram. Two inline SVGs on one
page with the same `arrow` id will take each other's markers.

## Charts

Same rules, plus:

- **Axes with real ticks and real labels**, including units. A bar chart with
  no y-axis is a picture of some rectangles.
- **Start bar charts at zero.** A truncated axis on bars is a lie told with
  geometry. Line charts may start elsewhere; say so on the axis.
- **Label the bars directly** when there are few enough, and drop the legend.
  A legend is a lookup table the reader has to hold in their head.
- **Put the numbers in a table under the chart**, inside `<details>` if it is
  long. The pane is often narrow, the reader often wants the value, and the
  table is what makes the chart checkable.
- Sort bars by value unless the category has its own order (time, size
  buckets). Alphabetical is almost never the order anybody wants.

Bars are `<rect>`, a line is one `<path>` with `fill="none"`, and both are
straightforward arithmetic from your data to the `viewBox` coordinates. Work
in the `viewBox` space and let the browser scale it.

## Before you save

- Does the caption say what the diagram is for, in a sentence?
- Are the edges labelled?
- Does it read on a dark background? Check that nothing is a hardcoded dark
  colour.
- Does it read at 380px wide, or does it need `overflow-x: auto` around it?
- Do the `id`s collide with anything else on the page?
- For a chart: are the units on the axis, and do the bars start at zero?
