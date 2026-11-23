// How wide the shell's screens grow. The shell itself spans the window (up to 160rem), so a lab's
// page and its network diagram use a wide window; the list and form screens (overview, labs,
// machine, servers, cloud) stop at LIST_SCREEN, where their rows still read as rows, centred.

/** A list or form screen: full width up to 90rem (1440px), then centred. Wider than the 72rem
 *  the shell used to cap every screen at, so a 1920px window still uses most of its width. */
export const LIST_SCREEN = "mx-auto w-full max-w-[90rem]";

/** Running text (a brief, a challenge's prompt): a readable line however wide its column. */
export const PROSE = "max-w-[75ch]";

/** A form inside a wide panel (a challenge's answer field and its help): no wider than this. */
export const FORM = "max-w-[48rem]";
