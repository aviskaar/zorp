---
name: react-components
description: How to write a landing page or small site as React components in JSX, as a Vite project the person builds and runs themselves. Use when asked for React, JSX, components, a Vite or Next app, or to turn a page written with the landing-page skill into components. Load landing-page as well for the copy, structure and accessibility rules; this skill covers only what changes when the output is components.
---

# Writing a page as React components

This skill sits on top of `landing-page`, and does not repeat it. The
section order, the copy rules, the markup and accessibility lists, the
theme tokens and the list of what a generated page looks like all still
apply, word for word. Load that skill first if it is not already loaded.
What follows is only what changes when the deliverable is a component tree
instead of one HTML file.

## What zorp can and cannot do with this

Be honest with the person about three things, in the answer and not only
here.

1. **The side pane cannot preview it.** The pane serves files under a bare
   `Content-Security-Policy: sandbox`, so scripts do not run there, and
   there is no bundler in it either. A `.jsx` file opens as text. Do not
   try to work around this with a `<script type="text/babel">` block and a
   transpiler from a CDN: the block does not run in the sandbox, the page
   renders blank, and nothing says why.
2. **zorp has no build step.** The project is written to disk and the
   person runs it with their own Node. Say the two commands they need at
   the end of the answer.
3. **Do not install anything on your own initiative.** `npm install`
   reaches the network and writes a large tree into the workspace. If the
   person asks you to run it, run it through `run_command` like anything
   else, and it is gated like anything else.

If a preview in the pane matters, write the page with `landing-page` first,
check it there, then convert it with the rules below. That order costs one
extra file and catches every copy and layout problem before any of it is
spread across components.

## The project

Vite with React, plain JavaScript unless the person asks for TypeScript,
under `scratch/<name>/`, named for what it sells:

```
scratch/acme-site/
  package.json
  index.html          the Vite entry: one <div id="root">, the title,
                      the description meta, lang and viewport
  src/
    main.jsx          createRoot(...).render(<App />), nothing else
    App.jsx           the page: the sections, in order, and nothing else
    content.js        every repeating list as an array of objects
    globals.css       the :root tokens and the base rules
    components/
      Header.jsx
      Hero.jsx
      Features.jsx
      Faq.jsx
      Cta.jsx
      Footer.jsx
```

`package.json` needs only `react`, `react-dom`, `vite` and
`@vitejs/plugin-react`, with `dev` and `build` scripts. No UI kit, no CSS
framework, no router for a one page site, no state library. Each is a
dependency the person has to keep up to date for a page that does not need
it.

## One section, one component

Each top level section of the page is one component in its own file, and
`App.jsx` reads like the table of contents:

```jsx
export default function App() {
  return (
    <>
      <Header />
      <main>
        <Hero />
        <Features items={features} />
        <Faq items={faq} />
        <Cta />
      </main>
      <Footer />
    </>
  );
}
```

- **Repeating blocks are a `.map` over data.** Feature cards, FAQ entries,
  pricing tiers and footer links live as arrays in `content.js`, and the
  component maps over them with a stable `key` taken from the data, never
  the index. Nothing positional in the CSS: a card styled by
  `:nth-child(2)` breaks the moment the list is reordered.
- **Copy lives in JSX or in `content.js`, never in CSS.** No text in
  `::before` content and no meaning in a background image.
- **Props are data, not layout switches.** A component with a `variant`
  prop for each place it is used is three components wearing one name.
- **A component that is used once does not need props at all.** `Hero`
  can hold its own copy. Extract only what repeats.

## Styling

- `globals.css` holds the `:root` tokens from `landing-page`, unchanged,
  with the dark block beside them. Every colour in every component is a
  `var(--token)`.
- Per component styles in a CSS module next to it (`Hero.module.css`),
  keyed off class names. Ids are for anchors and label bindings.
- No inline `style={{...}}` objects for anything but a value computed at
  runtime. Inline styles cannot be themed and cannot see media queries.

## Markup is still markup

JSX is HTML with different spelling, and every rule from `landing-page`
holds: one `h1`, headings that do not skip, `alt` on every image, a
`<label htmlFor>` on every input, `<a>` to navigate and `<button>` to act,
never a clickable `div`. The JSX spellings are `className`, `htmlFor`, and
self closing void elements (`<img />`, `<input />`).

## Script, and only where it earns it

- **The page's content must not depend on an effect.** Everything a
  visitor reads is rendered from props and data on the first render. A
  section that fetches its own copy in `useEffect` is blank to a crawler
  and blank until the request lands.
- Interactive state stays inside the component that owns it: an open FAQ
  item, a mobile menu. No global store, and no script in one section that
  reaches into another's DOM.
- A form posts to a real `action` and works without the handler; the
  handler is an improvement on top.
- `prefers-reduced-motion` is honoured in CSS, and any animation added in
  script checks it too.

## Converting a page from `landing-page`

A page written with that skill already has the shape this one needs, and
the conversion is mechanical:

1. Each `<section id="...">` becomes the component of the same name.
2. Each run of identical blocks becomes an array in `content.js` and one
   `.map`.
3. The `:root` block moves to `globals.css` as it is.
4. `class` becomes `className`, `for` becomes `htmlFor`, void elements
   close themselves.
5. Any script that was scoped to one section moves into that component.

Say which section became which component in the answer, so the person can
check the mapping against the HTML they already saw.

## Before you finish

- Does `npm run build` have everything it needs: every import resolves,
  every component is exported, `package.json` lists every package imported?
- Is every list rendered from data with a stable `key`?
- Is there still exactly one `h1`, and does every input have a label?
- Would the page read correctly if no effect ever ran?
- Did the answer say that the pane cannot preview this, and give the two
  commands to run it: `npm install`, then `npm run dev`?
