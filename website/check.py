"""Run with python3 website/check.py (or npm run check); check publishable
links and assets. Operates on dist/, so run `npm run build` first."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit

root = Path(__file__).parent / "dist"
assert root.exists(), "dist/ missing — run npm run build first"


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids, self.links = set(), []
        self.feed(text)

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if "id" in attrs:
            assert attrs["id"] not in self.ids, f"Duplicate id: {attrs['id']}"
            self.ids.add(attrs["id"])
        self.links.extend(attrs[key] for key in ("src", "href") if key in attrs)


# Parsed once per file: a link and a directory listing may both point at the
# same page, and the duplicate-id assertion should only run once for it.
parsed: dict[Path, Page] = {}


def page_for(path: Path) -> Page:
    if path not in parsed:
        parsed[path] = Page(path.read_text())
    return parsed[path]


def resolve(url_path: str) -> Path | None:
    """Pages clean URLs: `/updates` may be `updates.html` or `updates/index.html`."""
    base = root / url_path.lstrip("/")
    for candidate in (base, base.with_suffix(".html"), base / "index.html"):
        if candidate.exists():
            return candidate
    return None


for file in sorted(root.rglob("*.html")):
    page = page_for(file)
    for link in page.links:
        url = urlsplit(link)
        if url.scheme or url.netloc:
            assert url.scheme == "https", f"{file.name}: non-https {link}"
        elif url.path == "/download":
            assert "/download https://" in (root / "_redirects").read_text()
        elif url.path:
            target = resolve(url.path)
            assert target is not None, f"{file.name}: missing {url.path}"
            # A clean URL serves a page, so `/#features` and `/updates#v0.1.1`
            # have to resolve their fragments against that page's ids.
            if url.fragment:
                index = target / "index.html" if target.is_dir() else target
                if index.exists() and index.suffix == ".html":
                    assert url.fragment in page_for(index).ids, (
                        f"{file.name}: {link} has no #{url.fragment}"
                    )
        elif url.fragment:
            assert url.fragment in page.ids, f"{file.name}: missing #{url.fragment}"
assert not list(root.rglob("*.env"))
print("All page links, SVG references and local assets resolve.")
