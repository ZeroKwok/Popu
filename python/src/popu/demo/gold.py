"""Show a simulated gold market briefing with a 30-day price chart."""
import argparse
from datetime import date, timedelta
import sys

from popu import Client, PopuError


# Fixed, fictional daily closes in USD per troy ounce. No market API is used.
PRICES = (
    2642, 2651, 2646, 2660, 2674, 2668, 2683, 2691, 2687, 2703,
    2715, 2706, 2698, 2711, 2725, 2738, 2730, 2722, 2740, 2755,
    2746, 2762, 2775, 2768, 2784, 2791, 2782, 2798, 2812, 2824,
)


def notification() -> dict:
    today = date.today()
    prices = [
        {"date": (today - timedelta(days=29 - index)).isoformat(), "price": price}
        for index, price in enumerate(PRICES)
    ]
    latest = PRICES[-1]
    daily_change = latest - PRICES[-2]
    monthly_change = (latest / PRICES[0] - 1) * 100
    return {
        "version": "1.0", "mode": "notify", "template": "notification",
        "window": {"width": 480, "height": 680}, "duration_ms": 20000,
        "content": {
            "severity": "info", "title": "Gold briefing | Monthly high",
            "vertical_align": "start",
            "blocks": [
                {"type": "paragraph", "text": "METALS DESK / DEMO EDITION\nGold extends its rally as safe-haven demand builds."},
                {"type": "properties", "items": [
                    {"label": "Latest close", "text": f"USD {latest:,.2f} / troy oz"},
                    {"label": "Daily move", "text": f"{daily_change:+.2f} ({daily_change / PRICES[-2]:+.2%})"},
                    {"label": "30-day move", "text": f"{monthly_change:+.2f}%"},
                    {"label": "30-day range", "text": f"USD {min(PRICES):,} - {max(PRICES):,}"},
                ]},
                {"type": "chart", "title": "Last 30 days / USD per troy ounce", "engine": "vega_lite", "height": 200,
                 "spec": {
                    "description": "Simulated daily gold prices over the last 30 days",
                    "data": {"values": prices},
                    "layer": [
                        {"mark": {"type": "area", "color": "#d6a538", "opacity": 0.15},
                         "encoding": {"y2": {"datum": min(PRICES) - 20}}},
                        {"mark": {"type": "line", "color": "#d6a538", "strokeWidth": 2.5,
                                  "point": {"filled": True, "size": 20, "color": "#d6a538"}}},
                    ],
                    "encoding": {
                        "x": {"field": "date", "type": "temporal", "axis": {"title": None, "format": "%b %d", "tickCount": 5, "grid": False}},
                        "y": {"field": "price", "type": "quantitative", "scale": {"zero": False}, "axis": {"title": None, "format": ",.0f", "tickCount": 4}},
                        "tooltip": [
                            {"field": "date", "type": "temporal", "title": "Date", "format": "%b %d, %Y"},
                            {"field": "price", "type": "quantitative", "title": "USD / troy oz", "format": ",.2f"},
                        ],
                    },
                }},
                {"type": "unordered_list", "title": "Market pulse / fictional scenario", "items": [
                    "Softer yields and a weaker dollar lift demand for bullion.",
                    "Jewellers slow restocking as prices reach a monthly high.",
                ]},
                {"type": "paragraph", "text": "Simulated prices and fictional commentary.\nSource: Popu demo feed / 30 daily observations."},
            ],
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m popu.demo gold", description=__doc__)
    parser.add_argument("--wait", action="store_true", help="Wait for the notification to close")
    parser.add_argument("--executable", help="Use a local Popu executable for source development")
    args = parser.parse_args(argv)
    try:
        client = Client(args.executable)
        receipt = client.notify(notification())
        print("Gold briefing sent. All prices and commentary are simulated.", flush=True)
        if args.wait:
            client.wait(receipt.request_id)
            print("Notification finished.", flush=True)
    except KeyboardInterrupt:
        print("\nDemo stopped.", flush=True)
        return 130
    except PopuError as error:
        print(f"Could not run the demo: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
