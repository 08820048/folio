# Security

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |
| Older / unreleased commits | No |

Folio is a local desktop application. It has no account system and sends no
project content anywhere. The update check is the only network request a
packaged build makes.

## Reporting

**Do not open a public issue for a vulnerability.**

Email **ilikexff@gmail.com** with:

- what the problem is and what an attacker could do with it
- how to reproduce it, including the smallest project or file that shows it
- the operating system and the Folio version / commit
- whether you want to be credited

You should hear back within **7 days**. A fix is targeted within **30 days**
for a confirmed issue. Please give the project time to ship a fix before
talking about it in public. There is no paid bounty.

## Scope

Reports are especially welcome for:

- a way to make Folio read or write outside the opened project without asking
- command or path injection through file names, Git output, the updater, or
  the built-in terminal's launch paths
- a bypass of the save conflict check or the update signature
- a crash or data loss caused by a hostile file, repository, or terminal
  escape sequence
- a dependency advisory that is actually reachable from Folio

Out of scope: social engineering, physical access, issues in a project the
user opened, and vulnerabilities that require the attacker to already run code
on the machine.

## Safe harbor

Good-faith research is welcome. Do not access data you do not own, do not
degrade other users' systems, and do not publish details before a fix. The
project will not pursue legal action for research that follows this policy.
