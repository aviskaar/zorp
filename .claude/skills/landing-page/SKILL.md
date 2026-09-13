---
name: landing-page
description: How to write a landing page or small marketing site as one self-contained HTML file, and how to structure it so moving to JSX later is mechanical. Use when asked for a landing page, a product page, a marketing site, a coming-soon page, or any page meant to persuade a visitor rather than report a result.
---

# Writing a landing page

A landing page is a deliverable. Somebody is going to put it on a real web
server and send people to it. That makes it a different job from the report
pages covered by the `artifact-design` skill, which are written to be read
once in a side pane and thrown away.

Two rules carry most of the weight, and the rest of this file is detail
underneath them.

1. **One file, no build step.** Everything in `scratch/<name>.html`: the
   markup, a `<style>` block, and a `<script>` block only if the page
   genuinely needs one.
2. **The page works with JavaScript off.** Not as a courtesy to anybody.
   It is what makes the page previewable, fast, and crawlable, and it is
   what makes the later move to components easy instead of a rewrite.

## Where to write it

`scratch/` in the workspace, named for what it sells:
`scratch/zorp-landing.html`, not `scratch/index.html` and not
`scratch/page2.html`. The artifact pane lists what the workspace holds, and
a directory of `index.html` files is a directory nobody can navigate.

## What the preview can and cannot show

The browser's artifact pane will render the file, and this is the fastest
way to see what you wrote. It serves `.html` under a bare
`Content-Security-Policy: sandbox`, which means the document is in a unique
origin with scripting off.

So in the pane:

- **A `<script>` does not run.** Not inline, not from a CDN, not
  `type="module"`, not `type="text/babel"`. Nothing reports the failure. A
  page whose content is assembled by JavaScript previews as a blank page.
- **Nothing external loads.** No web fonts, no CDN stylesheet, no remote
  image. Treat that as a rule for the file and not just for the preview: a
  page that pulls its fonts from a third party is a page that tells that
  third party who opened it.

None of this is true of the shipped page. Once the file is on a real server
and opened in a real browser, scripts run normally. So the honest way to
read the pane is as the no-JavaScript view of your page, which is a view
worth checking on purpose.

That is why rule 2 exists. Put every word of copy, every section and every
piece of layout in HTML and CSS. If you add script at all, add it so the
page is complete without it: a scroll animation, a form that posts anyway
without the handler, a menu that is already an anchor list before the
toggle attaches. Then the preview is the page, minus the polish.

## The default structure

Unless asked for something else, this shape, in this order. Every part is
optional except the first and the last, and a short honest page beats a long
padded one.

```
header    a wordmark and at most four links, plus one call to action
hero      one sentence that says what this is, one that says who it is
          for, and one button. Not a slogan. A visitor who reads only
          this should be able to say what the product does.
proof     who uses it, what it replaced, a number with its unit. Leave
          it out rather than inventing it.
features  three to six. Each one names a job the visitor has, not a
          component the product has.
how       three or four steps, or a code block if the audience is
          technical and the product is a tool.
faq       the objections, answered plainly. This is usually the highest
          value section on the page and the one most often skipped.
cta       the same ask as the hero, repeated at the bottom where the
          convinced reader is.
footer    links, contact, copyright, and nothing else.
```

Write the copy before the CSS. A page whose sections are `<h2>Feature
One</h2>` with lorem under them is a template, and the person who asked for
a landing page will have to write the whole thing again.

## Markup

Semantics are the part that gets skipped when nobody asks, so they are the
part written down here.

- One `<h1>` on the page, in the hero, and it says what the product is.
- Headings descend without skipping. A `<h4>` under a `<h2>` is a bug.
- `<header>`, `<nav>`, `<main>`, `<section>`, `<footer>`. A `<section>`
  carries a heading or it is a `<div>`.
- A button that navigates is `<a class="button">`. A button that acts is
  `<button>`. Never a `<div onclick>`.
- Every `<img>` has `alt`, and a decorative one has `alt=""` rather than no
  attribute. Give width and height so the page does not jump as it loads.
- Every form control has a `<label>` tied to it by `for` and `id`.
  A placeholder is not a label.
- `<html lang="en">` and a `<meta name="viewport" content="width=device-width,
  initial-scale=1">`. Both are one line and both are wrong by default.
- A `<title>` and a `<meta name="description">`, because this page is going
  to be linked somewhere.

## Accessibility, the short list

- Contrast at 4.5:1 for body text against its own background. Check the
  muted grey, which is the one that usually fails.
- Visible focus. If you reset `outline`, put something back, or a keyboard
  visitor cannot see where they are.
- Do not carry meaning in colour alone.
- Tap targets around 44px.
- Respect `@media (prefers-reduced-motion: reduce)` and turn off anything
  that moves.

## Layout and type without a framework

No Tailwind, no Bootstrap, no reset library. Modern CSS does this in under
200 lines.

```css
:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #16181d;
  --muted: #5b6478;
  --line: #dde1e9;
  --accent: #1450f5;
  --measure: 65ch;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d0e11;
    --fg: #e7e9ee;
    --muted: #99a1af;
    --line: #23262d;
    --accent: #5eead4;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font: 16px/1.6 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
}
.wrap { max-width: 1100px; margin-inline: auto; padding-inline: clamp(20px, 5vw, 48px); }
.grid { display: grid; gap: 32px; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); }
p { max-width: var(--measure); }
h1, h2 { text-wrap: balance; line-height: 1.15; }
img, svg { max-width: 100%; height: auto; }
```

- Define every colour as a token on bare `:root` first, and redefine only
  what changes inside the dark block. A colour whose only definition is in
  a media query is a colour that is missing half the time.
- `repeat(auto-fit, minmax(...))` gets you a responsive feature grid with no
  media query at all.
- `clamp()` for section padding and for the hero heading size.
- Cap running text at around 65 characters. Full width paragraphs on a
  desktop monitor are unreadable.
- System font stacks, since there are no web fonts to load.

## What a generated landing page looks like, and how not to

The default output of a model asked for a landing page is recognisable, and
it is not good. Avoid:

- A purple to pink gradient behind the hero.
- Every surface at the same 8px radius with the same soft drop shadow.
- Three stat cards holding invented numbers with no units.
- Emoji standing in for icons.
- "Unlock", "Supercharge", "Seamlessly", "Revolutionise", "Empower". A
  sentence that would survive being about any product at all is a sentence
  that is not about this one.
- A testimonial from a person who does not exist. Leave the section out.

Prefer one accent colour used sparingly, real borders instead of shadows,
whitespace doing the grouping that boxes would otherwise do, and copy
specific enough that it would be wrong for a competitor.

## The path to JSX

Sooner or later the page becomes a Next or Vite app. The work here is to
make that a move rather than a rewrite, and the cost of doing so is close
to zero if you write the single file with it in mind.

**Do not try to preview JSX in the pane.** Shipping a
`<script type="text/babel">` block with a transpiler from a CDN is the
obvious idea and it does not work here: the sandbox does not run scripts and
does not load the CDN, so the page renders as nothing and says nothing about
why. Write plain HTML, and convert when there is a real build.

What makes the conversion mechanical:

- **One section, one component.** Give each top level `<section>` an `id`
  and a single wrapper class. Each one becomes `Hero.jsx`, `Features.jsx`,
  `Faq.jsx` with no restructuring.
- **Repeating blocks come from an array already.** Write the three feature
  cards as three identical blocks with the same class names and nothing
  positional in the CSS, so they become one `.map` over a list of objects.
  A card styled by `:nth-child(2)` is a card that has to be rewritten.
- **Copy sits in the markup, not in the CSS.** No text in `::before`
  content, no meaning in a background image.
- **Custom properties are the theme.** The `:root` block moves to
  `globals.css` or a theme object untouched. A page whose colours are
  hard coded at every use site has to be found and replaced by hand.
- **Class names, not ids, for styling.** Ids are for anchors and for label
  bindings. CSS modules and styled components both key off classes.
- **No global JavaScript that reaches across sections.** Anything scripted
  should be scoped to the section that owns it, or it becomes a
  `useEffect` nobody can place.

Say in the answer which sections map to which components, so the person
reading has the conversion plan and not just the file.

## Before you save

- Does the hero say what this is and who it is for, in words a competitor
  could not reuse?
- Is there a `<h1>`, and exactly one?
- Does every image have `alt` and every input have a `<label>`?
- Does it read at 380px wide, and at 1600px?
- Is every number real, and does it have its unit?
- With scripts off, is anything missing? If yes, move it into the markup.
- Does the body have an explicit background, so it does not borrow the
  pane's?
