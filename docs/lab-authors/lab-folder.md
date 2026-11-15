# A lab's folder

What the launcher reads from a lab's folder when a player imports it (**Labs > Import a lab**):
the lab itself, and what its page shows. A lab of the catalogue gets its page from Cyber CTF
instead.

## The lab: `isoloom.yml`

The folder must hold an `isoloom.yml`, the lab's description in
[Isoloom](https://www.isoloom.com)'s format. The launcher copies the folder and runs the lab as
containers on the player's machine, named after the spec's `name`. Deliberately vulnerable web
applications a lab can run are in [training apps](training-apps.md).

## Its page: `.ctf/`

Without a `.ctf/` folder, the lab's page is the lab itself: its name, state, deployment steps and
log, network diagram, where it runs, the attack box, and Stop, Open in browser and the shell. With
one, the page also shows what these files define, each read on its own:

| File                   | On the page                                                                                                                                           |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `.ctf/metadata.json`   | the title, description, difficulty (1 to 3), category and capabilities (as tags); without it, the title is the spec's `name` and the rest is left out |
| `.ctf/objectives.json` | the objectives (`{ "objectives": [...] }`): each one's title, prompt and guided steps, to read only; without it, no objectives section                |

The objectives are read only: progress and answers are checked and kept by Cyber CTF for
published labs, so an imported lab's page says so instead of offering an answer field. The answers
(`check`, `dev`) and the hint texts never reach the page. A file that is there but can't be read
(not JSON, a missing `title`, a difficulty out of range, a key used twice) is named on the page
with why ("metadata.json can't be read: …"); the other file is still read.

The page reads the launcher's copy, not your folder: to see a change, stop the lab and import the
folder again.
