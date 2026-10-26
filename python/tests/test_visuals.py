import base64
import json

import pytest

from popu import DialogRequest, Notification, chart_block, image_block, image_source, svg_source


SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32"/></svg>'


def test_file_images_are_captured_before_the_file_is_deleted(tmp_path):
    path = tmp_path / "graphic.svg"
    path.write_text(SVG, encoding="utf-8")
    source = image_source(path, alt="Graphic")
    path.unlink()
    assert base64.b64decode(source["src"].split(",", 1)[1]).decode() == SVG


def test_visual_blocks_work_in_both_request_builders():
    source = svg_source(SVG, alt="Icon")
    chart = chart_block({"mark": "bar", "data": {"values": [{"x": 1}]}}, title="Chart", height=200)
    image = image_block(source, caption="Snapshot", height=150)
    for request in [DialogRequest.confirm(title="Review", blocks=[chart, image], icon=source),
                    Notification.success(title="Report", blocks=[chart, image], icon=source)]:
        content = json.loads(json.dumps(request.as_dict()))["content"]
        assert content["icon"] == source
        assert [block["type"] for block in content["blocks"]] == ["chart", "image"]
        assert content["blocks"][0]["engine"] == "vega_lite"


def test_image_sources_reject_empty_alt_and_oversized_images():
    with pytest.raises(ValueError):
        svg_source(SVG, alt=" ")
    with pytest.raises(ValueError):
        svg_source("x" * (256 * 1024), alt="Huge")
