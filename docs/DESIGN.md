# Launcher design system

The launcher implements the Cyber family identity, **Lit from within** (source of truth: the
`cyber-design-system` repo: `tokens.css`, `components.css`, `IMPLEMENTATION.md`), at app density.
The product jewel is **Cyber CTF emerald**. This file describes the system as it is implemented here.

## Principles

- An app, not a marketing page: dense rows (3.25rem), compact panels, a Vercel-dashboard feel.
- Neutrals carry the screen. The emerald jewel appears only on the mark, the primary button, an
  italic word in a page title and the few "selected" accents (radio, switch, focus ring).
- Status colours (ok, warn, fail) are separate tokens and never the product colour.
- No glow field, no grain, no decorative grid in the launcher. The only dotted background is the
  network diagram canvas.
- `rem` everywhere; `px` only for 1px hairlines and React Flow geometry.
- No em or en dashes in copy, comments or commits.

## Files

| File | What it holds |
|---|---|
| `src/app/globals.css` | Tokens for Dark, Black, Light; the Tailwind theme mapping; surface, button, dot, meter and type utilities; motion |
| `src/app/topology.css` | The network diagram (React Flow) styles, token-driven |
| `src/app/layout.tsx` | Fonts: Geist and Geist Mono (`geist` package), Newsreader (`src/app/fonts`, variable opsz + wght, normal and italic). Nothing loads from Google at runtime |
| `src/lib/appearance.ts`, `public/appearance.js` | The Dark / Black / Light preference, applied before first paint and synced across windows |
| `src/components/brand/mark.tsx` | `CtfMark`, the layered up-chevron, coloured by `--mark-top` / `--mark-base` |
| `src/components/ui/*` | The primitives below |
| `src/lib/dev-mock.ts` | Development only: `pnpm dev` then `http://localhost:3000/?mock` previews every screen in a browser with sample data |

## Modes

Dark is the default (bare `:root`). **Black** (true `#000`, for OLED) and **Light** (Paper) are a
class on `<html>` chosen in Settings > Appearance and saved in `localStorage`
(`cyberctf.appearance`). `public/appearance.js` applies it before the first paint (the CSP only
allows same-origin scripts, so it is a file, not inline). Every window follows a change made in
another window through the `storage` event (`useAppearance`).

## Tokens

Use the Tailwind names (`bg-card`, `text-faint`...). Never a hex value or a palette class
(`emerald-500`, `zinc-*`) in a component: add a token instead.

| Token | Use |
|---|---|
| `background`, `surface` | Window, alt band (sidebar uses `card`) |
| `card`, `card-2` | Bottom and top of the panel gradient |
| `popover` | Floating surfaces |
| `foreground`, `muted-foreground`, `faint` | Ink: primary, secondary, tertiary (mono meta, labels) |
| `accent` | Hover fill |
| `glass`, `glass-2` | Translucent fills: inputs, chips, nav hover / selected nav, inset highlight |
| `border`, `input` (`border-strong`), `edge` | Hairlines, control rings, lit top edge |
| `success`, `warning`, `destructive` | Status only |
| `jewel`, `jewel-text`, `jewel-solid`, `jewel-on` (+ `--jewel-top`, `--jewel-bottom`, `--jewel-rgb`) | The emerald accent |
| `primary`, `ring`, `link` | Aliases of the jewel |
| `you`, `you-text` | The player's attacker in the network diagram (amethyst reads as "you") |
| `--hue-*` | Categorical colours for diagram service types (data, not status) |
| `--log`, `--scrim`, `--edge-line` | Log wells, modal backdrop, diagram cables |

Radii: `rounded-xs` 0.3125, `rounded-sm` 0.5 (nav items, chips, small tiles), `rounded-control`
0.625 (inputs, segmented, list tiles), `rounded-md` 0.75, `rounded-panel` 0.875 (panels, cards),
`rounded-lg` 1.25, `rounded-xl` 1.75, `rounded-full` (buttons, badges).

## Type

- **Page titles:** `.page-title` (Newsreader 350, 2rem, tracking -0.03em). At most one word in
  `<em>` (italic, jewel). Smaller serif: `.serif-title` (empty states, stat values, figures).
- **UI:** Geist. Body 0.875rem, rows and controls 0.8125rem, secondary 0.75rem.
- **Data:** Geist Mono for IPs, ports, images, versions, commits, timers, counts and meta.
  Meta is 0.6875rem `text-faint`.
- **Labels:** `.section-label` (mono uppercase 0.625rem, faint) for groups; `.eyebrow` (mono with
  a hairline) where a kicker is needed.

## Surfaces

- `.surface-panel`: the default container (gradient `card-2` to `card`, inset lit edge, hairline
  ring). Used by `Panel`, `Card`, `EmptyState`, the diagram host frame.
- `.surface-glass`: floating things (command menu, dialogs, tooltips, toasts).
- `.surface-log`: log output, code blocks, terminal text.
- `.dotted-canvas`: the network diagram background only.

## Components (`src/components/ui`)

| Component | Notes |
|---|---|
| `Button` | Pill. `default`/`primary` = jewel gradient (one per view), `outline`/`secondary` = ghost glass, `inverted`, `ghost`, `destructive` (fail text on glass, fail ring), `link`. Sizes `lg` 3rem, default 2.25rem, `sm` 2rem, `xs` 1.75rem (rows), `icon`, `icon-sm`. Press = scale(.97) |
| `Panel`, `PanelHeader`, `KeyValue`, `RailLabel` | Panel with a 2.75rem header (title, mono `meta`, `action`); key/value rows; label above a panel |
| `PageHeader`, `StatusStrip` | Serif title, lead or mono status strip, actions on the right |
| `StatusDot`, `StatusPill` | Dots: ok (halo), warn, fail, idle; `pulse` for in progress |
| `Badge`, `LevelBadge` | Mono pills. Levels: beginner green, intermediate amber, advanced red |
| `Meter`, `Sparkline` | Usage bar (jewel, warm over 75%, fail over 90%); stat-card sparkline |
| `Segmented`, `Chip` | Glass segmented control (view switches, filters); filter chips (selected = ink) |
| `Input`, `Select`, `Textarea`, `fieldClass` | 2.25rem (sm 2rem) glass fields, jewel focus ring with a soft glow |
| `Switch`, `RadioRow`/`RadioList`, `Choice`/`ChoiceGrid` | Selected = jewel |
| `ConfirmDialog`, `Tip`, `Toaster`/`toast()` | Glass overlays; toasts bottom right (`tell()` uses them while the window has focus) |
| `LogConsole`, `Markdown`, `CopyValue` | Log well with a header and timer; lab write-ups (serif headings); copyable mono values |
| `EmptyState`, `Skeleton`, `Spinner` | Serif empty title in a panel; glass skeletons |
| `TypeIcon` | Glass tile at the start of a row |

## Patterns

- **Screen:** `PageHeader` (serif title, one italic jewel word at most; lead line in muted), then
  panels with `gap-5` (1.25rem). Two-column screens use `minmax(0,1.35fr) minmax(0,1fr)` or a
  fixed right rail of about 19rem.
- **Rows:** 3.25rem high, `px-4`, `border-t border-border` between rows (`first:border-t-0`),
  `hover:bg-glass`. Leading status dot or `TypeIcon`, title (0.8125rem medium) over mono meta
  (0.6875rem faint), right-hand mono columns, then an `xs` action.
- **Stat cards:** a panel with a label and mono detail, a serif value (`.serif-title` 2rem with
  a small unit), then a `Sparkline` or `Meter`.
- **Status:** a dot plus a word, never a coloured block. Warnings and errors as a panel row or a
  callout with a dot or icon in the status colour and muted text.
- **Shell:** sidebar 14.5rem on `card` (mark + version, Find with the shortcut, nav, Running labs
  with live dots and steps, Settings, account); 3rem top bar with breadcrumb and the engine status.
- **Diagram:** host frame > lab network zones (dashed, jewel label) > machines with typed service
  rows and port doors; the attacker in `you` violet with animated dashed links; published ports as
  tabs on the frame's bottom edge; dotted canvas.
- **Motion:** subtle, CSS only, always from a visible state; all of it off under
  `prefers-reduced-motion`.
