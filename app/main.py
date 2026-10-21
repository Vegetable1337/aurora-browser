"""Aurora desktop browser alpha — Chromium via Qt WebEngine."""

from __future__ import annotations

import sys
import sqlite3
from pathlib import Path
from urllib.parse import quote_plus

from PySide6.QtCore import QStandardPaths, QUrl
from PySide6.QtGui import QAction, QKeySequence
from PySide6.QtWidgets import (
    QApplication, QHBoxLayout, QLineEdit, QMainWindow, QMessageBox,
    QListWidget, QDialog, QDialogButtonBox, QPushButton, QTabWidget, QToolBar, QVBoxLayout, QWidget,
)
from PySide6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEngineSettings
from PySide6.QtWebEngineWidgets import QWebEngineView


APP_NAME = "Aurora"
SEARCH_URL = "https://duckduckgo.com/?q={}"
class BrowserTab(QWidget):
    def __init__(self, profile: QWebEngineProfile, address: str = "") -> None:
        super().__init__()
        self.view = QWebEngineView(self)
        self.page = QWebEnginePage(profile, self.view)
        self.view.setPage(self.page)
        if address:
            self.view.setUrl(QUrl(address))
        else:
            self.view.setHtml("""
                <!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">
                <style>html,body{height:100%;margin:0}body{display:grid;place-items:center;background:radial-gradient(ellipse at 50% 35%,#293047,#12151e 65%);color:#f2f4fa;font:16px system-ui,sans-serif}.card{text-align:center;padding:48px 64px;border:1px solid #ffffff20;border-radius:28px;background:#ffffff0b;box-shadow:0 24px 80px #0005}h1{font-size:48px;letter-spacing:.12em;font-weight:500;margin:0 0 10px}p{color:#b6bdd0;margin:0}</style>
                </head><body><main class="card"><h1>AURORA</h1><p>Search or enter an address to begin</p></main></body></html>
            """, QUrl("about:blank"))
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.view)


class AuroraWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("Aurora")
        self.resize(1440, 900)
        self.app_data = Path(QStandardPaths.writableLocation(QStandardPaths.StandardLocation.AppLocalDataLocation))
        self.app_data.mkdir(parents=True, exist_ok=True)
        self.profile = QWebEngineProfile("Default", self)
        self.profile.setPersistentStoragePath(str(self.app_data / "Default"))
        self.profile.setCachePath(str(self.app_data / "Default" / "Cache"))
        self.profile.setPersistentCookiesPolicy(QWebEngineProfile.PersistentCookiesPolicy.ForcePersistentCookies)
        self.profile.downloadRequested.connect(self._download_requested)
        self.bookmark_db = self.app_data / "Default" / "aurora.sqlite3"
        self._init_bookmarks()

        self.tabs = QTabWidget(self)
        self.tabs.setDocumentMode(True)
        self.tabs.setMovable(True)
        self.tabs.setTabsClosable(True)
        self.tabs.tabCloseRequested.connect(self.close_tab)
        self.tabs.currentChanged.connect(self._current_tab_changed)
        self.setCentralWidget(self.tabs)

        bar = QToolBar("Navigation", self)
        bar.setMovable(False)
        bar.setObjectName("navigation")
        self.addToolBar(bar)
        self.back_button = self._button("‹", "Back", bar, lambda: self._with_view("back"))
        self.forward_button = self._button("›", "Forward", bar, lambda: self._with_view("forward"))
        self.reload_button = self._button("↻", "Reload", bar, lambda: self._with_view("reload"))
        self.address = QLineEdit(self)
        self.address.setPlaceholderText("Search or enter an address")
        self.address.setClearButtonEnabled(True)
        self.address.returnPressed.connect(self.navigate)
        bar.addWidget(self.address)
        self._button("☆", "Bookmark this page", bar, self.bookmark_current)
        self._button("+", "New tab", bar, self.new_tab)

        self._install_shortcuts()
        self._install_menus()
        self._apply_theme()
        self.new_tab()

    def _button(self, text: str, tooltip: str, toolbar: QToolBar, callback) -> QPushButton:
        button = QPushButton(text, self)
        button.setToolTip(tooltip)
        button.setObjectName("navButton")
        button.clicked.connect(callback)
        toolbar.addWidget(button)
        return button

    def _install_shortcuts(self) -> None:
        for sequence, callback in (
            ("Ctrl+L", lambda: (self.address.setFocus(), self.address.selectAll())),
            ("Ctrl+T", self.new_tab),
            ("Ctrl+W", lambda: self.close_tab(self.tabs.currentIndex())),
            ("Ctrl+R", lambda: self._with_view("reload")),
            ("Alt+Left", lambda: self._with_view("back")),
            ("Alt+Right", lambda: self._with_view("forward")),
        ):
            action = QAction(self)
            action.setShortcut(QKeySequence(sequence))
            action.triggered.connect(callback)
            self.addAction(action)

    def _init_bookmarks(self) -> None:
        self.bookmark_db.parent.mkdir(parents=True, exist_ok=True)
        with sqlite3.connect(self.bookmark_db) as db:
            db.execute("CREATE TABLE IF NOT EXISTS bookmarks (url TEXT PRIMARY KEY, title TEXT NOT NULL, added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")

    def _install_menus(self) -> None:
        bookmarks_menu = self.menuBar().addMenu("Bookmarks")
        add = bookmarks_menu.addAction("Bookmark this page")
        add.setShortcut("Ctrl+D")
        add.triggered.connect(self.bookmark_current)
        show = bookmarks_menu.addAction("Show bookmarks…")
        show.triggered.connect(self.show_bookmarks)

    def _apply_theme(self) -> None:
        self.setStyleSheet("""
            QMainWindow { background: #10131b; }
            QToolBar#navigation { background: rgba(29, 34, 47, 238); border: 0; spacing: 8px;
                padding: 10px 14px; }
            QLineEdit { background: rgba(255,255,255,0.10); color: #f5f7ff; border: 1px solid rgba(255,255,255,0.12);
                border-radius: 18px; padding: 10px 16px; selection-background-color: #7389ff; }
            QPushButton#navButton { color: #edf0ff; background: rgba(255,255,255,0.08); border: 1px solid rgba(255,255,255,0.08);
                border-radius: 17px; min-width: 34px; min-height: 34px; font-size: 17px; }
            QPushButton#navButton:hover { background: rgba(132,153,255,0.25); }
            QTabWidget::pane { border: 0; background: #10131b; }
            QTabBar { background: #171b26; }
            QTabBar::tab { color: #c5cad9; background: #202534; border-radius: 10px; padding: 8px 16px; margin: 5px 3px; min-width: 110px; }
            QTabBar::tab:selected { color: white; background: #343c53; }
        """)

    def current_tab(self) -> BrowserTab | None:
        tab = self.tabs.currentWidget()
        return tab if isinstance(tab, BrowserTab) else None

    def _with_view(self, method: str) -> None:
        tab = self.current_tab()
        if tab:
            getattr(tab.view, method)()

    def new_tab(self, address: str = "") -> None:
        tab = BrowserTab(self.profile, address)
        tab.view.titleChanged.connect(lambda title, page=tab: self._title_changed(page, title))
        tab.view.urlChanged.connect(lambda url, page=tab: self._url_changed(page, url))
        tab.view.loadProgress.connect(lambda progress: self.statusBar().showMessage(f"Loading… {progress}%", 1500))
        index = self.tabs.addTab(tab, "New Tab")
        self.tabs.setCurrentIndex(index)
        self.address.setFocus()

    def close_tab(self, index: int) -> None:
        if self.tabs.count() <= 1:
            self.new_tab()
        widget = self.tabs.widget(index)
        self.tabs.removeTab(index)
        if widget:
            widget.deleteLater()

    def navigate(self) -> None:
        text = self.address.text().strip()
        if not text:
            return
        if " " in text or ("." not in text and "://" not in text):
            url = SEARCH_URL.format(quote_plus(text))
        else:
            url = text if "://" in text else f"https://{text}"
        tab = self.current_tab()
        if tab:
            tab.view.setUrl(QUrl(url))
        self.address.clearFocus()

    def _title_changed(self, tab: BrowserTab, title: str) -> None:
        index = self.tabs.indexOf(tab)
        if index >= 0:
            short = title[:24] + ("…" if len(title) > 24 else "")
            self.tabs.setTabText(index, short or "New Tab")
            if index == self.tabs.currentIndex():
                self.setWindowTitle(f"{title or 'New Tab'} — Aurora")

    def _url_changed(self, tab: BrowserTab, url: QUrl) -> None:
        if tab is self.current_tab():
            self.address.setText(url.toString() if url.scheme() != "about" else "")
        self.back_button.setEnabled(tab.view.history().canGoBack())
        self.forward_button.setEnabled(tab.view.history().canGoForward())

    def _current_tab_changed(self, _index: int) -> None:
        tab = self.current_tab()
        if tab:
            url = tab.view.url()
            self.address.setText(url.toString() if url.scheme() != "about" else "")
            self.setWindowTitle(f"{tab.view.title() or 'New Tab'} — Aurora")
            self.back_button.setEnabled(tab.view.history().canGoBack())
            self.forward_button.setEnabled(tab.view.history().canGoForward())

    def bookmark_current(self) -> None:
        tab = self.current_tab()
        if tab and tab.view.url().isValid() and tab.view.url().scheme() in ("http", "https"):
            with sqlite3.connect(self.bookmark_db) as db:
                db.execute("INSERT INTO bookmarks(url, title) VALUES(?, ?) ON CONFLICT(url) DO UPDATE SET title=excluded.title",
                           (tab.view.url().toString(), tab.view.title() or tab.view.url().host()))
            self.statusBar().showMessage("Bookmark saved", 2000)

    def show_bookmarks(self) -> None:
        dialog = QDialog(self)
        dialog.setWindowTitle("Bookmarks")
        dialog.resize(560, 440)
        layout = QVBoxLayout(dialog)
        listing = QListWidget(dialog)
        with sqlite3.connect(self.bookmark_db) as db:
            rows = db.execute("SELECT title, url FROM bookmarks ORDER BY title COLLATE NOCASE").fetchall()
        for title, url in rows:
            listing.addItem(f"{title}  —  {url}")
        layout.addWidget(listing)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close, parent=dialog)
        buttons.rejected.connect(dialog.reject)
        buttons.accepted.connect(dialog.accept)
        layout.addWidget(buttons)

        def open_bookmark(_item) -> None:
            row = listing.currentRow()
            if row >= 0:
                self.new_tab(rows[row][1])
                dialog.accept()

        listing.itemDoubleClicked.connect(open_bookmark)
        dialog.exec()

    def _download_requested(self, download) -> None:
        from PySide6.QtWidgets import QFileDialog
        suggested = download.suggestedFileName()
        target, _ = QFileDialog.getSaveFileName(self, "Save download", str(Path.home() / "Downloads" / suggested))
        if not target:
            download.cancel()
            return
        download.setDownloadDirectory(str(Path(target).parent))
        download.setDownloadFileName(Path(target).name)
        download.accept()


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    window = AuroraWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
