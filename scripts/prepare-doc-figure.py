"""Make the supplied draw.io SVG render consistently with the offline SVG loader.

Keep diode-plug_simple.svg as the source of truth. draw.io puts visible labels
inside <switch>/<foreignObject> with embedded PNG fallbacks; usvg ignores the
switch. It also writes CSS light-dark() which usvg does not support. This
prepares a copy with the fallback images as ordinary SVG children and retains
the drawing's explicit fill/stroke attributes.
"""

from pathlib import Path
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
source = ROOT / "diode-plug_simple.svg"
destination = ROOT / "diode-plug_offline.svg"

tree = ET.parse(source)
svg = tree.getroot()
svg.attrib.pop("content", None)  # draw.io's embedded editor state
svg.attrib.pop("style", None)


def clean(parent):
    for child in list(parent):
        tag = child.tag.rsplit("}", 1)[-1]
        if tag == "foreignObject":
            parent.remove(child)
        elif tag == "switch":
            index = list(parent).index(child)
            for fallback in list(child):
                if fallback.tag.rsplit("}", 1)[-1] != "foreignObject":
                    child.remove(fallback)
                    parent.insert(index, fallback)
                    index += 1
                    clean(fallback)
            parent.remove(child)
        else:
            # Explicit SVG attributes (stroke/fill) are the light-theme source.
            # The draw.io light-dark() CSS overrides them in some rasterizers.
            child.attrib.pop("style", None)
            clean(child)


clean(svg)
ET.register_namespace("", "http://www.w3.org/2000/svg")
ET.register_namespace("xlink", "http://www.w3.org/1999/xlink")
tree.write(destination, encoding="utf-8", xml_declaration=True)
print(f"Prepared {destination.relative_to(ROOT)} from {source.name}")
