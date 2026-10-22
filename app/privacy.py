"""Network filtering built on Brave's Rust adblock engine.

The initial Aurora rules are a small, bundled domain set expressed as standard
ABP rules. The engine understands rule syntax and request types, but this is
not a full EasyList subscription or a guarantee that every tracker is blocked.
"""

from __future__ import annotations

from adblock import Engine, FilterSet
from PySide6.QtWebEngineCore import QWebEngineUrlRequestInfo, QWebEngineUrlRequestInterceptor


# Maintained as host rules for the alpha. They can later be supplemented with
# user-selected filter lists without changing the request-interceptor API.
TRACKER_HOSTS = frozenset({
    "adnxs.com", "amplitude.com", "analytics.twitter.com", "ads-twitter.com",
    "clarity.ms", "clicktale.net", "criteo.com", "doubleclick.net",
    "everesttech.net", "facebook.net", "googlesyndication.com",
    "googleadservices.com", "google-analytics.com", "hotjar.com", "hotjar.io",
    "mixpanel.com", "outbrain.com", "quantserve.com", "scorecardresearch.com",
    "segment.com", "segment.io", "fullstory.com", "newrelic.com", "nr-data.net",
    "optimizely.com", "taboola.com",
})

_RESOURCE_TYPES = {
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeMainFrame: "document",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeSubFrame: "subdocument",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeStylesheet: "stylesheet",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeScript: "script",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeImage: "image",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeFontResource: "font",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeObject: "object",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeMedia: "media",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeXhr: "xmlhttprequest",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypePing: "ping",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeCspReport: "csp_report",
    QWebEngineUrlRequestInfo.ResourceType.ResourceTypeFavicon: "image",
}


def _make_engine() -> Engine:
    filters = FilterSet()
    filters.add_filters([f"||{host}^$third-party" for host in sorted(TRACKER_HOSTS)], "standard")
    return Engine(filters, True)


def _host(value: str) -> str:
    return value.strip().rstrip(".").casefold()


def is_tracker_host(host: str) -> bool:
    candidate = _host(host)
    return any(candidate == blocked or candidate.endswith("." + blocked)
               for blocked in TRACKER_HOSTS)


def is_tracker_request(request_host: str, first_party_host: str) -> bool:
    request = _host(request_host)
    first_party = _host(first_party_host)
    return is_tracker_host(request) and not (
        first_party and (request == first_party or request.endswith("." + first_party)
                         or first_party.endswith("." + request)))


class TrackerBlocker(QWebEngineUrlRequestInterceptor):
    def __init__(self, parent=None, enabled: bool = True) -> None:
        super().__init__(parent)
        self.enabled = enabled
        self._engine = _make_engine()

    def interceptRequest(self, info) -> None:
        if not self.enabled:
            return
        url = info.requestUrl().toString()
        source = info.firstPartyUrl().toString()
        request_type = _RESOURCE_TYPES.get(info.resourceType(), "other")
        if self._engine.check_network_urls(url, source, request_type).matched:
            info.block(True)
