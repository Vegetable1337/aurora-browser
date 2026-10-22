from __future__ import annotations

import unittest
from types import SimpleNamespace

from PySide6.QtCore import QUrl
from PySide6.QtWebEngineCore import QWebEngineUrlRequestInfo

from app.privacy import TrackerBlocker, is_tracker_host, is_tracker_request


class TrackerBlockerTests(unittest.TestCase):
    def test_host_rules_match_domains_and_subdomains_with_boundaries(self) -> None:
        self.assertTrue(is_tracker_host("doubleclick.net"))
        self.assertTrue(is_tracker_host("metrics.doubleclick.net"))
        self.assertTrue(is_tracker_host("DOUBLECLICK.NET."))
        self.assertFalse(is_tracker_host("notdoubleclick.net"))

    def test_only_cross_site_requests_are_blocked(self) -> None:
        self.assertTrue(is_tracker_request("google-analytics.com", "shop.example"))
        self.assertFalse(is_tracker_request("google-analytics.com", "analytics.google-analytics.com"))
        self.assertFalse(is_tracker_request("cdn.example", "shop.example"))

    def test_interceptor_can_be_toggled(self) -> None:
        blocker = TrackerBlocker()
        blocked: list[bool] = []
        info = SimpleNamespace(
            requestUrl=lambda: QUrl("https://doubleclick.net/pixel"),
            firstPartyUrl=lambda: QUrl("https://shop.example/"),
            resourceType=lambda: QWebEngineUrlRequestInfo.ResourceType.ResourceTypeImage,
            block=blocked.append,
        )
        blocker.interceptRequest(info)
        self.assertEqual(blocked, [True])

        blocker.enabled = False
        blocker.interceptRequest(info)
        self.assertEqual(blocked, [True])


if __name__ == "__main__":
    unittest.main()
