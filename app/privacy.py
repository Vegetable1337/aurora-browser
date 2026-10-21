"""Small built-in tracker-host blocker for Aurora's early alpha.

This intentionally is not an Adblock Plus/EasyList parser. The bundled host
set is limited, reviewed in source, and must not be described as comprehensive.
"""

from __future__ import annotations

from PySide6.QtWebEngineCore import QWebEngineUrlRequestInterceptor


TRACKER_HOSTS = frozenset({
    "adnxs.com",
    "amplitude.com",
    "analytics.twitter.com",
    "ads-twitter.com",
    "clarity.ms",
    "clicktale.net",
    "criteo.com",
    "doubleclick.net",
    "everesttech.net",
    "facebook.net",
    "googlesyndication.com",
    "googleadservices.com",
    "google-analytics.com",
    "hotjar.com",
    "hotjar.io",
    "mixpanel.com",
    "outbrain.com",
    "quantserve.com",
    "scorecardresearch.com",
    "segment.com",
    "segment.io",
    "fullstory.com",
    "newrelic.com",
    "nr-data.net",
    "optimizely.com",
    "taboola.com",
})


def _host(value: str) -> str:
    return value.strip().rstrip(".").casefold()


def is_tracker_host(host: str) -> bool:
    candidate = _host(host)
    return any(candidate == blocked or candidate.endswith("." + blocked)
               for blocked in TRACKER_HOSTS)


def is_tracker_request(request_host: str, first_party_host: str) -> bool:
    request = _host(request_host)
    first_party = _host(first_party_host)
    if not is_tracker_host(request):
        return False
    if first_party and (request == first_party or request.endswith("." + first_party)
                        or first_party.endswith("." + request)):
        return False
    return True


class TrackerBlocker(QWebEngineUrlRequestInterceptor):
    def __init__(self, parent=None, enabled: bool = True) -> None:
        super().__init__(parent)
        self.enabled = enabled

    def interceptRequest(self, info) -> None:
        if self.enabled and is_tracker_request(
                info.requestUrl().host(), info.firstPartyUrl().host()):
            info.block(True)
