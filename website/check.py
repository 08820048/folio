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


for file in sorted(root.rglob("*.html")):
    page = Page(file.read_text())
    for link in page.links:
        url = urlsplit(link)
        if url.scheme or url.netloc:
            assert url.scheme == "https", link
        elif url.path == "/download":
            assert "/download https://" in (root / "_redirects").read_text()
        elif url.path:
            target = root / url.path.lstrip("/")
            assert target.exists(), f"{file.name}: missing {target}"
        elif url.fragment:
            assert url.fragment in page.ids, f"{file.name}: missing #{url.fragment}"
assert not list(root.rglob("*.env"))
print("All page links, SVG references and local assets resolve.")
