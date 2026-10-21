"""Aurora desktop browser alpha — Chromium via Qt WebEngine."""

from __future__ import annotations

import sys
import sqlite3
import json
from contextlib import closing
from datetime import datetime
from pathlib import Path
from urllib.parse import quote_plus

from PySide6.QtCore import QStandardPaths, QTimer, QUrl, Qt
from PySide6.QtGui import QAction, QKeySequence
from PySide6.QtWidgets import (
    QApplication, QDialog, QDialogButtonBox, QInputDialog, QLabel, QLineEdit,
    QListWidget, QMainWindow, QMessageBox, QPushButton, QTabWidget, QToolBar,
    QVBoxLayout, QWidget,
)
from PySide6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEnginePermission
from PySide6.QtWebEngineWidgets import QWebEngineView

if __package__:
    from .crypto_profiles import ProfileError, ProfileUnlockError, ProfileVault
    from .privacy import TrackerBlocker
else:  # Support the documented `python app\main.py` launch command.
    from crypto_profiles import ProfileError, ProfileUnlockError, ProfileVault
    from privacy import TrackerBlocker


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
    def __init__(self, private_mode: bool = False) -> None:
        super().__init__()
        self.private_mode = private_mode
        self.setWindowTitle("Aurora — Private Window" if private_mode else "Aurora")
        if private_mode:
            self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose, True)
        self.resize(1440, 900)
        self.app_data = Path(QStandardPaths.writableLocation(QStandardPaths.StandardLocation.AppLocalDataLocation))
        self.app_data.mkdir(parents=True, exist_ok=True)
        self.profiles_root = self.app_data / "Profiles"
        self.profiles_root.mkdir(parents=True, exist_ok=True)
        self.active_profile_id = "Default"
        self.active_profile_name = "Default (unprotected)"
        self.active_vault: ProfileVault | None = None
        self.tracker_blocking_enabled = True
        self.tracker_blocker: TrackerBlocker | None = None
        self._profile_switching = False
        if private_mode:
            self.profile = self._create_private_engine_profile()
            self.bookmark_db = None
        else:
            self.profile = self._create_engine_profile("Default", self.app_data / "Default")
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
        self.find_bar = QLineEdit(self)
        self.find_bar.setPlaceholderText("Find on this page")
        self.find_bar.setClearButtonEnabled(True)
        self.find_bar.returnPressed.connect(self.find_next)
        self.find_bar.textChanged.connect(self.find_text)
        self.statusBar().addPermanentWidget(self.find_bar)
        self.find_bar.hide()

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

    def _create_engine_profile(self, profile_id: str, storage_dir: Path) -> QWebEngineProfile:
        storage_dir.mkdir(parents=True, exist_ok=True)
        engine_profile = QWebEngineProfile(profile_id, self)
        engine_profile.setPersistentStoragePath(str(storage_dir))
        engine_profile.setCachePath(str(storage_dir / "Cache"))
        engine_profile.setPersistentCookiesPolicy(QWebEngineProfile.PersistentCookiesPolicy.AllowPersistentCookies)
        engine_profile.setPersistentPermissionsPolicy(
            QWebEngineProfile.PersistentPermissionsPolicy.StoreInMemory)
        self.tracker_blocker = TrackerBlocker(engine_profile, self.tracker_blocking_enabled)
        engine_profile.setUrlRequestInterceptor(self.tracker_blocker)
        engine_profile.downloadRequested.connect(self._download_requested)
        return engine_profile

    def _create_private_engine_profile(self) -> QWebEngineProfile:
        engine_profile = QWebEngineProfile(self)
        engine_profile.setHttpCacheType(QWebEngineProfile.HttpCacheType.MemoryHttpCache)
        engine_profile.setPersistentCookiesPolicy(QWebEngineProfile.PersistentCookiesPolicy.NoPersistentCookies)
        engine_profile.setPersistentPermissionsPolicy(
            QWebEngineProfile.PersistentPermissionsPolicy.StoreInMemory)
        self.tracker_blocker = TrackerBlocker(engine_profile, self.tracker_blocking_enabled)
        engine_profile.setUrlRequestInterceptor(self.tracker_blocker)
        engine_profile.downloadRequested.connect(self._download_requested)
        return engine_profile

    def _install_shortcuts(self) -> None:
        for sequence, callback in (
            ("Ctrl+L", lambda: (self.address.setFocus(), self.address.selectAll())),
            ("Ctrl+T", self.new_tab),
            ("Ctrl+Shift+N", self.open_private_window),
            ("Ctrl+W", lambda: self.close_tab(self.tabs.currentIndex())),
            ("Ctrl+R", lambda: self._with_view("reload")),
            ("Ctrl+F", self.open_find_bar),
            ("Ctrl+G", self.find_next),
            ("Ctrl+Shift+G", self.find_previous),
            ("Escape", self.close_find_bar),
            ("Alt+Left", lambda: self._with_view("back")),
            ("Alt+Right", lambda: self._with_view("forward")),
        ):
            action = QAction(self)
            action.setShortcut(QKeySequence(sequence))
            action.triggered.connect(callback)
            self.addAction(action)

    def _init_bookmarks(self) -> None:
        self.bookmark_db.parent.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(self.bookmark_db)) as db, db:
            db.execute("CREATE TABLE IF NOT EXISTS bookmarks (url TEXT PRIMARY KEY, title TEXT NOT NULL, added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")
            db.execute("CREATE TABLE IF NOT EXISTS history (url TEXT PRIMARY KEY, title TEXT NOT NULL, visited_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, visits INTEGER NOT NULL DEFAULT 1)")

    def _install_menus(self) -> None:
        bookmarks_menu = self.menuBar().addMenu("Bookmarks")
        add = bookmarks_menu.addAction("Bookmark this page")
        add.setShortcut("Ctrl+D")
        add.triggered.connect(self.bookmark_current)
        show = bookmarks_menu.addAction("Show bookmarks…")
        show.triggered.connect(self.show_bookmarks)

        history_menu = self.menuBar().addMenu("History")
        show_history = history_menu.addAction("Show recent visits…")
        show_history.triggered.connect(self.show_history)
        clear_history = history_menu.addAction("Clear Aurora visit list…")
        clear_history.triggered.connect(self.clear_history)

        privacy_menu = self.menuBar().addMenu("Privacy")
        self.tracker_action = privacy_menu.addAction("Block known tracking domains")
        self.tracker_action.setCheckable(True)
        self.tracker_action.setChecked(self.tracker_blocking_enabled)
        self.tracker_action.toggled.connect(self._set_tracker_blocking)
        privacy_info = privacy_menu.addAction("About tracker protection…")
        privacy_info.triggered.connect(lambda: QMessageBox.information(
            self, "Tracker protection",
            "Aurora blocks requests to a small built-in set of known tracking domains. "
            "This list is not comprehensive and is not updated automatically. "
            "It may affect some website features. The setting applies to this app session."))
        if self.private_mode:
            bookmarks_menu.setEnabled(False)
            history_menu.setEnabled(False)

        self.profiles_menu = self.menuBar().addMenu("Profiles")
        self.profiles_menu.aboutToShow.connect(self._refresh_profile_menu)
        self.profiles_menu.setEnabled(not self.private_mode)
        self._refresh_profile_menu()

    def _set_tracker_blocking(self, enabled: bool) -> None:
        self.tracker_blocking_enabled = enabled
        if self.tracker_blocker is not None:
            self.tracker_blocker.enabled = enabled
        self.statusBar().showMessage(
            "Known tracking-domain blocking enabled" if enabled else "Tracking-domain blocking disabled",
            3000,
        )

    def _refresh_profile_menu(self) -> None:
        if self.private_mode:
            return
        self.profiles_menu.clear()
        create = self.profiles_menu.addAction("Create password-protected profile…")
        create.triggered.connect(self.create_protected_profile)
        if self.active_vault is not None:
            lock = self.profiles_menu.addAction(f"Lock {self.active_profile_name}")
            lock.triggered.connect(self.lock_protected_profile)
            change = self.profiles_menu.addAction("Change profile password…")
            change.triggered.connect(self.change_profile_password)
        self.profiles_menu.addSeparator()
        default_action = self.profiles_menu.addAction("Default (unprotected)")
        default_action.setEnabled(self.active_vault is not None)
        default_action.triggered.connect(self.switch_to_default_profile)
        for profile_id, name, profile_dir in ProfileVault.discover(self.profiles_root):
            if profile_id == self.active_profile_id:
                label = f"✓ {name} (open)"
            else:
                label = f"Unlock {name}…"
            open_action = self.profiles_menu.addAction(label)
            open_action.setEnabled(profile_id != self.active_profile_id)
            open_action.triggered.connect(lambda _checked=False, path=profile_dir: self.unlock_and_switch_profile(path))
            recovery_action = self.profiles_menu.addAction(f"Recover {name}…")
            recovery_action.setEnabled(profile_id != self.active_profile_id)
            recovery_action.triggered.connect(
                lambda _checked=False, path=profile_dir: self.recover_and_switch_profile(path))
            delete_action = self.profiles_menu.addAction(f"Delete {name}…")
            delete_action.setEnabled(profile_id != self.active_profile_id)
            delete_action.triggered.connect(lambda _checked=False, path=profile_dir, title=name: self.delete_locked_profile(path, title))

    def create_protected_profile(self) -> None:
        name, accepted = QInputDialog.getText(self, "New profile", "Profile name:")
        if not accepted:
            return
        name = name.strip()
        if not name or len(name) > 64 or name.casefold() == "default":
            QMessageBox.warning(self, "Profile name", "Enter a name from 1 to 64 characters.")
            return
        if any(existing.casefold() == name.casefold()
               for _profile_id, existing, _path in ProfileVault.discover(self.profiles_root)):
            QMessageBox.warning(self, "Profile name", "A profile with that name already exists.")
            return
        password, accepted = QInputDialog.getText(
            self, "Protect profile", "Create a password (at least 12 characters):",
            QLineEdit.EchoMode.Password)
        if not accepted:
            return
        confirmation, accepted = QInputDialog.getText(
            self, "Confirm password", "Enter the password again:", QLineEdit.EchoMode.Password)
        if not accepted:
            return
        if password != confirmation:
            QMessageBox.warning(self, "Profile password", "The passwords do not match.")
            return
        try:
            created = ProfileVault.create(self.profiles_root, password, name)
        except ProfileError as exc:
            QMessageBox.warning(self, "Could not create profile", str(exc))
            return

        recovery_dialog = QDialog(self)
        recovery_dialog.setWindowTitle("Save the recovery key")
        recovery_dialog.setModal(True)
        layout = QVBoxLayout(recovery_dialog)
        layout.addWidget(QLabel(
            "Save this key somewhere safe. It is shown once and can unlock this profile if you forget its password. "
            "The key vault encrypts Aurora bookmarks and settings only. Chromium cookies, history and cache remain "
            "unencrypted on disk.", recovery_dialog))
        recovery_field = QLineEdit(created.recovery_key, recovery_dialog)
        recovery_field.setReadOnly(True)
        recovery_field.selectAll()
        layout.addWidget(recovery_field)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok, parent=recovery_dialog)
        buttons.accepted.connect(recovery_dialog.accept)
        layout.addWidget(buttons)
        recovery_dialog.exec()
        recovery_field.clear()
        self._switch_browser_session(created.profile)

    def unlock_and_switch_profile(self, profile_dir: Path) -> None:
        password, accepted = QInputDialog.getText(
            self, "Unlock profile", "Enter the profile password:", QLineEdit.EchoMode.Password)
        if not accepted:
            return
        try:
            vault = ProfileVault.unlock_password(profile_dir, password)
        except ProfileError as exc:
            QMessageBox.warning(self, "Could not unlock profile", str(exc))
            return
        self._switch_browser_session(vault)

    def recover_and_switch_profile(self, profile_dir: Path) -> None:
        recovery, accepted = QInputDialog.getText(
            self, "Recover profile", "Enter the profile's 64-character recovery key:",
            QLineEdit.EchoMode.Password)
        if not accepted:
            return
        try:
            vault = ProfileVault.unlock_recovery(profile_dir, recovery.strip())
        except ProfileError as exc:
            QMessageBox.warning(self, "Could not recover profile", str(exc))
            return
        self._switch_browser_session(vault)

    def change_profile_password(self) -> None:
        if self.active_vault is None:
            return
        password, accepted = QInputDialog.getText(
            self, "Change profile password", "New password (at least 12 characters):",
            QLineEdit.EchoMode.Password)
        if not accepted:
            return
        confirmation, accepted = QInputDialog.getText(
            self, "Confirm password", "Enter the new password again:", QLineEdit.EchoMode.Password)
        if not accepted:
            return
        if password != confirmation:
            QMessageBox.warning(self, "Profile password", "The passwords do not match.")
            return
        try:
            self.active_vault.change_password(password)
        except ProfileError as exc:
            QMessageBox.warning(self, "Could not change password", str(exc))
            return
        self.statusBar().showMessage("Profile password changed", 3000)

    def switch_to_default_profile(self) -> None:
        if self.active_vault is not None:
            self._switch_browser_session(None)

    def lock_protected_profile(self) -> None:
        if self.active_vault is not None:
            self._switch_browser_session(None)
            self.statusBar().showMessage("Protected app-data vault locked; Chromium files remain on disk unencrypted", 6000)

    def delete_locked_profile(self, profile_dir: Path, name: str) -> None:
        answer = QMessageBox.question(
            self, "Delete profile",
            f"Delete {name} and its Aurora data? Filesystem remnants and backups may remain.",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        try:
            ProfileVault.delete_profile_directory(profile_dir)
        except (OSError, ProfileError) as exc:
            QMessageBox.warning(self, "Could not delete profile", str(exc))
        self._refresh_profile_menu()

    def _switch_browser_session(self, next_vault: ProfileVault | None) -> None:
        if self._profile_switching:
            if next_vault is not None:
                next_vault.lock()
            return
        next_id = next_vault.profile_id if next_vault else "Default"
        if next_id == self.active_profile_id:
            return
        self._profile_switching = True
        self.profiles_menu.setEnabled(False)
        self.statusBar().showMessage("Closing the current profile…")
        old_engine_profile = self.profile
        old_vault = self.active_vault
        count = self.tabs.count()
        pending = [count]

        def after_tabs_destroyed() -> None:
            pending[0] -= 1
            if pending[0] == 0:
                remove_engine_profile()

        def activate_next_profile(*_args) -> None:
            if old_vault is not None:
                old_vault.lock()
            self.active_vault = next_vault
            self.active_profile_id = next_id
            self.active_profile_name = next_vault.display_name if next_vault else "Default (unprotected)"
            storage_dir = (next_vault.profile_dir / "Chromium") if next_vault else self.app_data / "Default"
            self.profile = self._create_engine_profile(next_id, storage_dir)
            if next_vault is None:
                self.bookmark_db = self.app_data / "Default" / "aurora.sqlite3"
                self._init_bookmarks()
            else:
                self.bookmark_db = None
            self.setWindowTitle(f"Aurora — {self.active_profile_name}")
            self.new_tab()
            self._profile_switching = False
            self.profiles_menu.setEnabled(True)
            self._refresh_profile_menu()

        def remove_engine_profile() -> None:
            old_engine_profile.destroyed.connect(activate_next_profile)
            old_engine_profile.deleteLater()

        for index in reversed(range(self.tabs.count())):
            tab = self.tabs.widget(index)
            if isinstance(tab, BrowserTab):
                tab.view.stop()
            self.tabs.removeTab(index)
            if tab is not None:
                if count:
                    tab.destroyed.connect(after_tabs_destroyed)
                tab.deleteLater()
        self.profile = None
        if count == 0:
            remove_engine_profile()

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
        tab.view.loadFinished.connect(lambda ok, page=tab: self._page_loaded(page, ok))
        tab.page.permissionRequested.connect(self._handle_permission_request)
        tab.view.loadProgress.connect(lambda progress: self.statusBar().showMessage(f"Loading… {progress}%", 1500))
        index = self.tabs.addTab(tab, "New Tab")
        self.tabs.setCurrentIndex(index)
        self.address.setFocus()

    def _handle_permission_request(self, permission: QWebEnginePermission) -> None:
        if not permission.isValid():
            return
        names = {
            QWebEnginePermission.PermissionType.MediaAudioCapture: "use your microphone",
            QWebEnginePermission.PermissionType.MediaVideoCapture: "use your camera",
            QWebEnginePermission.PermissionType.MediaAudioVideoCapture: "use your camera and microphone",
            QWebEnginePermission.PermissionType.DesktopVideoCapture: "record your screen",
            QWebEnginePermission.PermissionType.DesktopAudioVideoCapture: "record your screen and audio",
            QWebEnginePermission.PermissionType.MouseLock: "lock your mouse pointer",
            QWebEnginePermission.PermissionType.Notifications: "send notifications",
            QWebEnginePermission.PermissionType.Geolocation: "access your location",
            QWebEnginePermission.PermissionType.ClipboardReadWrite: "read and write your clipboard",
            QWebEnginePermission.PermissionType.LocalFontsAccess: "read locally installed fonts",
        }
        feature = names.get(permission.permissionType())
        if feature is None:
            permission.deny()
            return
        origin = permission.origin().toString()
        answer = QMessageBox.question(
            self, "Website permission",
            f"{origin} wants to {feature}. Allow this request?\n\nPermission choices stay in memory for this profile session.",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if answer == QMessageBox.StandardButton.Yes:
            permission.grant()
        else:
            permission.deny()

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

    def open_find_bar(self) -> None:
        self.find_bar.show()
        self.find_bar.setFocus()
        self.find_bar.selectAll()

    def close_find_bar(self) -> None:
        if self.find_bar.isVisible():
            tab = self.current_tab()
            if tab:
                tab.view.findText("")
            self.find_bar.clear()
            self.find_bar.hide()
            self.address.setFocus()

    def find_text(self, text: str) -> None:
        tab = self.current_tab()
        if tab:
            tab.view.findText(text)

    def find_next(self) -> None:
        self.find_text(self.find_bar.text())

    def find_previous(self) -> None:
        tab = self.current_tab()
        if tab:
            tab.view.findText(self.find_bar.text(), QWebEnginePage.FindFlag.FindBackward)

    def _page_loaded(self, tab: BrowserTab, ok: bool) -> None:
        if self.private_mode or not ok or self.tabs.indexOf(tab) < 0:
            return
        url = tab.view.url().toString()
        if tab.view.url().scheme() not in ("http", "https"):
            return
        title = tab.view.title() or tab.view.url().host()
        visited_at = datetime.now().astimezone().isoformat(timespec="seconds")
        if self.active_vault is not None:
            record_id = self.active_vault.opaque_record_id("history", url)
            payload = json.dumps({"url": url, "title": title, "visited_at": visited_at}, ensure_ascii=False).encode("utf-8")
            self.active_vault.put("history", record_id, payload)
        else:
            with closing(sqlite3.connect(self.bookmark_db)) as db, db:
                db.execute(
                    "INSERT INTO history(url, title, visited_at) VALUES(?, ?, ?) "
                    "ON CONFLICT(url) DO UPDATE SET title=excluded.title, visited_at=excluded.visited_at, visits=history.visits+1",
                    (url, title, visited_at),
                )

    def _history_rows(self) -> list[tuple[str, str, str]]:
        if self.active_vault is not None:
            rows: list[tuple[str, str, str]] = []
            for _record_id, raw in self.active_vault.list_records("history"):
                try:
                    item = json.loads(raw.decode("utf-8"))
                    rows.append((item["title"], item["url"], item["visited_at"]))
                except (UnicodeDecodeError, json.JSONDecodeError, KeyError, TypeError):
                    continue
            return sorted(rows, key=lambda row: row[2], reverse=True)
        with closing(sqlite3.connect(self.bookmark_db)) as db, db:
            return db.execute("SELECT title, url, visited_at FROM history ORDER BY visited_at DESC LIMIT 1000").fetchall()

    def show_history(self) -> None:
        dialog = QDialog(self)
        dialog.setWindowTitle("Recent visits")
        dialog.resize(720, 480)
        layout = QVBoxLayout(dialog)
        listing = QListWidget(dialog)
        rows = self._history_rows()
        for title, url, visited_at in rows:
            listing.addItem(f"{title} — {url}    ({visited_at})")
        layout.addWidget(listing)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close, parent=dialog)
        buttons.rejected.connect(dialog.reject)
        buttons.accepted.connect(dialog.accept)
        layout.addWidget(buttons)

        def open_visit(_item) -> None:
            row = listing.currentRow()
            if row >= 0:
                self.new_tab(rows[row][1])
                dialog.accept()

        listing.itemDoubleClicked.connect(open_visit)
        dialog.exec()

    def clear_history(self) -> None:
        answer = QMessageBox.question(
            self, "Clear Aurora visit list",
            "Remove the visit list saved by Aurora? Chromium may retain other history data in its profile folder.",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if answer != QMessageBox.StandardButton.Yes:
            return
        if self.active_vault is not None:
            for record_id, _raw in self.active_vault.list_records("history"):
                self.active_vault.delete_record("history", record_id)
        else:
            with closing(sqlite3.connect(self.bookmark_db)) as db, db:
                db.execute("DELETE FROM history")
        self.statusBar().showMessage("Aurora visit list cleared", 3000)

    def bookmark_current(self) -> None:
        if self.private_mode:
            return
        tab = self.current_tab()
        if tab and tab.view.url().isValid() and tab.view.url().scheme() in ("http", "https"):
            url = tab.view.url().toString()
            title = tab.view.title() or tab.view.url().host()
            if self.active_vault is not None:
                record_id = self.active_vault.opaque_record_id("bookmarks", url)
                payload = json.dumps({"url": url, "title": title}, ensure_ascii=False).encode("utf-8")
                self.active_vault.put("bookmarks", record_id, payload)
            else:
                with closing(sqlite3.connect(self.bookmark_db)) as db, db:
                    db.execute("INSERT INTO bookmarks(url, title) VALUES(?, ?) ON CONFLICT(url) DO UPDATE SET title=excluded.title",
                               (url, title))
            self.statusBar().showMessage("Bookmark saved", 2000)

    def show_bookmarks(self) -> None:
        dialog = QDialog(self)
        dialog.setWindowTitle("Bookmarks")
        dialog.resize(560, 440)
        layout = QVBoxLayout(dialog)
        listing = QListWidget(dialog)
        if self.active_vault is not None:
            rows = []
            for _record_id, raw in self.active_vault.list_records("bookmarks"):
                try:
                    bookmark = json.loads(raw.decode("utf-8"))
                    if isinstance(bookmark.get("url"), str) and isinstance(bookmark.get("title"), str):
                        rows.append((bookmark["title"], bookmark["url"]))
                except (UnicodeDecodeError, json.JSONDecodeError, AttributeError):
                    continue
            rows.sort(key=lambda row: row[0].casefold())
        else:
            with closing(sqlite3.connect(self.bookmark_db)) as db, db:
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

    def closeEvent(self, event) -> None:
        for index in range(self.tabs.count()):
            tab = self.tabs.widget(index)
            if isinstance(tab, BrowserTab):
                tab.view.stop()
        if self.active_vault is not None:
            self.active_vault.lock()
        if self.private_mode:
            private_windows = getattr(QApplication.instance(), "_aurora_private_windows", [])
            if self in private_windows:
                private_windows.remove(self)
        super().closeEvent(event)

    def open_private_window(self) -> None:
        app = QApplication.instance()
        private_windows = getattr(app, "_aurora_private_windows", None)
        if private_windows is None:
            private_windows = []
            app._aurora_private_windows = private_windows
        window = AuroraWindow(private_mode=True)
        private_windows.append(window)
        window.show()


def main() -> int:
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    window = AuroraWindow()
    window.show()
    if "--smoke-test" in sys.argv[1:]:
        from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
        from threading import Thread

        view = window.current_tab().view
        smoke_title = "AURORA_SMOKE_OK"

        class SmokeHandler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                body = (f"<!doctype html><title>{smoke_title}</title>"
                        "<main><h1>Aurora local browser check</h1></main>").encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", 0), SmokeHandler)
        Thread(target=server.serve_forever, daemon=True).start()

        def page_loaded(ok: bool) -> None:
            if not ok:
                app.exit(2)
                return
            app.exit(0 if view.title() == smoke_title else 3)

        view.titleChanged.connect(lambda title: app.exit(0) if title == smoke_title else None)
        view.loadFinished.connect(page_loaded)
        QTimer.singleShot(15_000, lambda: app.exit(4))
        view.setUrl(QUrl(f"http://127.0.0.1:{server.server_port}/"))
        result = app.exec()
        server.shutdown()
        server.server_close()
        return result
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
