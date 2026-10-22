"""Aurora desktop browser alpha — Chromium via Qt WebEngine."""

from __future__ import annotations

import sys
import sqlite3
import json
from contextlib import closing
from datetime import datetime
from pathlib import Path
from urllib.parse import quote_plus

from PySide6.QtCore import QEvent, QStandardPaths, QTimer, QUrl, Qt
from PySide6.QtGui import QAction, QKeySequence, QMouseEvent
from PySide6.QtWidgets import (
    QApplication, QDialog, QDialogButtonBox, QInputDialog, QLabel, QLineEdit,
    QListWidget, QMainWindow, QMenu, QMessageBox, QSizePolicy, QTabBar,
    QTabWidget, QToolBar, QToolButton, QVBoxLayout, QWidget,
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
                <title>Новая вкладка</title>
                <style>
                *{box-sizing:border-box}html,body{min-height:100%;margin:0}body{font:15px/1.5 Inter,"Segoe UI",system-ui,sans-serif;color:#f4f3fa;background:#35363a;overflow:auto}
                body:before{content:"";position:fixed;inset:0;pointer-events:none;background:radial-gradient(ellipse 48% 38% at 50% 25%,#52627a20,transparent 78%)}
                main{position:relative;min-height:100vh;display:flex;flex-direction:column;align-items:center;padding:clamp(70px,14vh,150px) 24px 42px}
                .mark{display:grid;place-items:center;width:58px;height:58px;border:1px solid #ffffff22;border-radius:20px;background:linear-gradient(145deg,#79869a,#525967 58%,#3e4249);box-shadow:0 12px 42px #0003;color:white;font-size:29px}
                h1{margin:19px 0 2px;font-size:34px;letter-spacing:.19em;font-weight:600}.sub{margin:0;color:#c0c1c5;font-size:14px}
                form{display:flex;align-items:center;gap:12px;width:min(620px,100%);height:58px;margin:35px 0 34px;padding:6px 8px 6px 19px;border:1px solid #ffffff1b;border-radius:30px;background:#292a2d;box-shadow:0 2px 8px #0003;transition:.2s}
                form:focus-within{border-color:#a9c7ff;background:#292a2d;box-shadow:0 0 0 2px #a9c7ff44,0 4px 12px #0003}form span{color:#c2c3c7;font-size:20px}
                input{flex:1;min-width:0;border:0;outline:0;background:transparent;color:#f5f4fb;font:inherit;font-size:15px}input::placeholder{color:#8e8b9f}
                button{height:42px;padding:0 17px;border:0;border-radius:22px;background:#4b4c50;color:white;font:600 13px/1 "Segoe UI",sans-serif;cursor:pointer}button:hover{background:#5a5b60}
                .section{width:min(620px,100%)}.section-title{margin:0 0 13px;color:#858497;font-size:11px;font-weight:600;letter-spacing:.14em;text-transform:uppercase}
                .shortcuts{display:grid;grid-template-columns:repeat(4,1fr);gap:10px}.shortcut{display:flex;flex-direction:column;align-items:center;gap:9px;min-width:0;padding:9px 6px;border:1px solid transparent;border-radius:12px;background:transparent;color:#e8e8ea;text-decoration:none;transition:.18s}
                .shortcut:hover{background:#ffffff12}.icon{display:grid;place-items:center;flex:0 0 48px;width:48px;height:48px;border:1px solid #ffffff14;border-radius:50%;background:#45464a;color:#e3e4e7;font-size:18px;font-weight:600}.label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px}
                .foot{margin-top:auto;padding-top:56px;color:#777587;font-size:12px}.private{color:#c8b8ff}
                @media(max-width:560px){.shortcuts{grid-template-columns:repeat(2,1fr)}main{padding-top:12vh}h1{font-size:29px}}
                </style></head><body><main>
                <div class="mark">✦</div><h1>AURORA</h1><p class="sub">Всё начинается с поиска</p>
                <form action="https://duckduckgo.com/" method="get"><span>⌕</span><input name="q" autofocus autocomplete="off" placeholder="Поиск в интернете" aria-label="Поиск в интернете"><button type="submit">Найти</button></form>
                <section class="section"><p class="section-title">Быстрый доступ</p><div class="shortcuts">
                <a class="shortcut" href="https://wikipedia.org"><span class="icon">W</span><span class="label">Wikipedia</span></a>
                <a class="shortcut" href="https://youtube.com"><span class="icon">▶</span><span class="label">YouTube</span></a>
                <a class="shortcut" href="https://github.com"><span class="icon">⌘</span><span class="label">GitHub</span></a>
                <a class="shortcut" href="https://duckduckgo.com"><span class="icon">⌕</span><span class="label">DuckDuckGo</span></a>
                </div></section>
                <div class="foot">Aurora — ваш браузер</div></main></body></html>
            """, QUrl("about:blank"))
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.view)


class AuroraWindow(QMainWindow):
    def __init__(self, private_mode: bool = False) -> None:
        super().__init__()
        self.private_mode = private_mode
        self.setWindowTitle("Aurora — Private Window" if private_mode else "Aurora")
        self.setWindowFlag(Qt.WindowType.FramelessWindowHint, True)
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
        self.tabs.tabBar().hide()
        self.tabs.setTabsClosable(False)
        self.tabs.tabCloseRequested.connect(self.close_tab)
        self.tabs.currentChanged.connect(self._current_tab_changed)
        self.setCentralWidget(self.tabs)

        tab_strip = QToolBar("Tabs", self)
        tab_strip.setMovable(False)
        tab_strip.setObjectName("tabStrip")
        self.tab_strip = tab_strip
        self.addToolBar(Qt.ToolBarArea.TopToolBarArea, tab_strip)
        self.chrome_tabs = QTabBar(self)
        self.chrome_tabs.setObjectName("chromeTabs")
        self.chrome_tabs.setExpanding(False)
        self.chrome_tabs.setMovable(True)
        self.chrome_tabs.setTabsClosable(True)
        self.chrome_tabs.setUsesScrollButtons(True)
        self.chrome_tabs.setElideMode(Qt.TextElideMode.ElideRight)
        self.chrome_tabs.setSizePolicy(QSizePolicy.Policy.Preferred, QSizePolicy.Policy.Fixed)
        self.chrome_tabs.currentChanged.connect(self._chrome_tab_selected)
        self.chrome_tabs.tabMoved.connect(self._chrome_tab_moved)
        self.chrome_tabs.tabCloseRequested.connect(self.close_tab)
        tab_strip.addWidget(self.chrome_tabs)
        self._button("+", "Новая вкладка (Ctrl+T)", tab_strip, self.new_tab)
        self.tab_spacer = QWidget(self)
        self.tab_spacer.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        tab_strip.addWidget(self.tab_spacer)
        self.minimize_button = self._button("—", "Свернуть", tab_strip, self.showMinimized)
        self.maximize_button = self._button("□", "Развернуть", tab_strip, self._toggle_maximize)
        self.close_button = self._button("×", "Закрыть окно", tab_strip, self.close)
        self.close_button.setObjectName("windowCloseButton")
        tab_strip.installEventFilter(self)
        self.tab_spacer.installEventFilter(self)

        bar = QToolBar("Navigation", self)
        bar.setMovable(False)
        bar.setObjectName("navigation")
        self.addToolBar(bar)
        self.back_button = self._button("←", "Назад", bar, lambda: self._with_view("back"))
        self.forward_button = self._button("→", "Вперёд", bar, lambda: self._with_view("forward"))
        self.reload_button = self._button("⟳", "Обновить", bar, lambda: self._with_view("reload"))
        self.address = QLineEdit(self)
        self.address.setObjectName("address")
        self.address.setPlaceholderText("Поиск или адрес сайта")
        self.address.setClearButtonEnabled(True)
        self.address.setMinimumWidth(200)
        self.address.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self.address.returnPressed.connect(self.navigate)
        bar.addWidget(self.address)
        self._button("☆", "Добавить в закладки (Ctrl+D)", bar, self.bookmark_current)
        self.menu_button = self._button("⋮", "Меню Aurora", bar, lambda: None)
        self.menu_button.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
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
        QApplication.instance().installEventFilter(self)
        self.new_tab()

    def _toggle_maximize(self) -> None:
        if self.isMaximized():
            self.showNormal()
        else:
            self.showMaximized()

    def changeEvent(self, event) -> None:
        super().changeEvent(event)
        if event.type() == QEvent.Type.WindowStateChange and hasattr(self, "maximize_button"):
            maximized = self.isMaximized()
            self.maximize_button.setText("❐" if maximized else "□")
            self.maximize_button.setToolTip("Восстановить" if maximized else "Развернуть")

    def eventFilter(self, watched, event) -> bool:
        if isinstance(watched, QWidget) and (watched is self or self.isAncestorOf(watched)):
            event_type = event.type()
            if event_type in (QEvent.Type.MouseButtonPress, QEvent.Type.MouseButtonDblClick):
                if isinstance(event, QMouseEvent) and event.button() == Qt.MouseButton.LeftButton:
                    point = event.globalPosition().toPoint()
                    bounds = self.frameGeometry()
                    margin = 7
                    edges = []
                    if point.x() <= bounds.left() + margin:
                        edges.append(Qt.Edge.LeftEdge)
                    elif point.x() >= bounds.right() - margin:
                        edges.append(Qt.Edge.RightEdge)
                    if point.y() <= bounds.top() + margin:
                        edges.append(Qt.Edge.TopEdge)
                    elif point.y() >= bounds.bottom() - margin:
                        edges.append(Qt.Edge.BottomEdge)
                    if edges and not self.isMaximized() and event_type == QEvent.Type.MouseButtonPress:
                        combined = edges[0]
                        for edge in edges[1:]:
                            combined |= edge
                        handle = self.windowHandle()
                        if handle is not None and handle.startSystemResize(combined):
                            return True
                    if watched in (self.tab_strip, self.tab_spacer):
                        if event_type == QEvent.Type.MouseButtonDblClick:
                            self._toggle_maximize()
                            return True
                        if event_type == QEvent.Type.MouseButtonPress:
                            handle = self.windowHandle()
                            if handle is not None and handle.startSystemMove():
                                return True
        return super().eventFilter(watched, event)

    def _button(self, text: str, tooltip: str, toolbar: QToolBar, callback) -> QToolButton:
        button = QToolButton(self)
        button.setText(text)
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
        self.menuBar().hide()
        bookmarks_menu = QMenu("Закладки и списки", self)
        add = bookmarks_menu.addAction("Добавить текущую страницу")
        add.setShortcut("Ctrl+D")
        add.triggered.connect(self.bookmark_current)
        show = bookmarks_menu.addAction("Все закладки…")
        show.triggered.connect(self.show_bookmarks)

        history_menu = QMenu("История", self)
        show_history = history_menu.addAction("Недавно посещённые…")
        show_history.triggered.connect(self.show_history)
        clear_history = history_menu.addAction("Очистить историю Aurora…")
        clear_history.triggered.connect(self.clear_history)

        privacy_menu = QMenu("Конфиденциальность", self)
        self.tracker_action = privacy_menu.addAction("Блокировать известные трекеры")
        self.tracker_action.setCheckable(True)
        self.tracker_action.setChecked(self.tracker_blocking_enabled)
        self.tracker_action.toggled.connect(self._set_tracker_blocking)
        privacy_info = privacy_menu.addAction("О защите от трекеров…")
        privacy_info.triggered.connect(lambda: QMessageBox.information(
            self, "Tracker protection",
            "Aurora uses Brave's adblock-rust engine with a small built-in set of third-party tracking-domain rules. "
            "This list is not comprehensive and is not updated automatically. "
            "It may affect some website features. The setting applies to this app session."))
        if self.private_mode:
            bookmarks_menu.setEnabled(False)
            history_menu.setEnabled(False)

        self.profiles_menu = QMenu("Профили", self)
        self.profiles_menu.aboutToShow.connect(self._refresh_profile_menu)
        self.profiles_menu.setEnabled(not self.private_mode)
        self._refresh_profile_menu()

        self.browser_menu = QMenu(self)
        new_tab_action = self.browser_menu.addAction("Новая вкладка")
        new_tab_action.setShortcut("Ctrl+T")
        new_tab_action.triggered.connect(self.new_tab)
        new_private_action = self.browser_menu.addAction("Новое окно в режиме инкогнито")
        new_private_action.setShortcut("Ctrl+Shift+N")
        new_private_action.triggered.connect(self.open_private_window)
        self.browser_menu.addSeparator()
        self.browser_menu.addMenu(bookmarks_menu)
        self.browser_menu.addMenu(history_menu)
        self.browser_menu.addMenu(privacy_menu)
        self.browser_menu.addMenu(self.profiles_menu)
        self.browser_menu.addSeparator()
        find_action = self.browser_menu.addAction("Найти на этой странице…")
        find_action.setShortcut("Ctrl+F")
        find_action.triggered.connect(self.open_find_bar)
        self.menu_button.setMenu(self.browser_menu)

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
        self.chrome_tabs.blockSignals(True)
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
            self.chrome_tabs.blockSignals(False)
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
            self.chrome_tabs.removeTab(index)
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
            QMainWindow { background: #35363a; color: #f3f1fa; }
            QMenuBar { background: #171720; color: #aaa8b8; border: 0; padding: 3px 8px; }
            QMenuBar::item { background: transparent; padding: 7px 11px; border-radius: 7px; }
            QMenuBar::item:selected { color: #f6f3ff; background: #ffffff12; }
            QMenu { background: #292a2d; color: #f0eef8; border: 1px solid #ffffff1c; padding: 6px; }
            QMenu::item { padding: 8px 28px 8px 12px; border-radius: 6px; }
            QMenu::item:selected { background: #4b4c50; color: white; }
            QMenu::separator { height: 1px; background: #ffffff19; margin: 5px 8px; }
            QToolBar#tabStrip { background: #202124; border: 0; spacing: 3px; padding: 0 7px; min-height: 42px; }
            QToolBar#navigation { background: #35363a; border: 0; spacing: 7px; padding: 7px 12px 10px; }
            QToolBar#navigation::separator { width: 1px; background: #ffffff1c; margin: 6px 7px; }
            QLabel#brand { color: #e8e4f5; font-size: 12px; font-weight: 700; letter-spacing: 1px; padding: 0 8px 0 1px; }
            QLineEdit { background: #292a2d; color: #f5f3fb; border: 1px solid #ffffff18;
                border-radius: 13px; padding: 9px 14px; selection-background-color: #8971dc; }
            QLineEdit#address { min-height: 22px; border-radius: 19px; padding: 10px 17px; font-size: 13px; }
            QLineEdit#address:focus { border: 1px solid #a9c7ff; background: #292a2d; }
            QToolButton#navButton { color: #cfccda; background: transparent; border: 1px solid transparent;
                border-radius: 12px; min-width: 36px; min-height: 36px; font-size: 18px; }
            QToolButton#navButton:hover { color: white; background: #ffffff12; border-color: #ffffff12; }
            QToolButton#navButton:disabled { color: #5d5b68; }
            QToolBar#tabStrip QToolButton#navButton { min-width: 30px; max-width: 38px; min-height: 30px;
                max-height: 34px; border-radius: 8px; color: #c9cace; }
            QToolBar#tabStrip QToolButton#windowCloseButton:hover { color: white; background: #c42b1c; }
            QToolBar#navigation QToolButton#navButton { min-width: 30px; max-width: 38px; }
            QToolButton::menu-indicator { image: none; }
            QTabWidget::pane { border: 0; background: #35363a; }
            QTabBar#chromeTabs { background: transparent; qproperty-drawBase: 0; }
            QTabBar#chromeTabs::tab { color: #c2c3c7; background: #2b2c30; border: 0;
                border-radius: 9px 9px 0 0; padding: 8px 13px; margin: 5px 2px 0; min-width: 170px; max-width: 240px; }
            QTabBar#chromeTabs::tab:hover { color: #f0f0f2; background: #35363a; }
            QTabBar#chromeTabs::tab:selected { color: #f4f4f5; background: #35363a; }
            QStatusBar { background: #292a2d; color: #b8b9bd; border-top: 1px solid #ffffff0c; font-size: 11px; }
            QStatusBar::item { border: 0; }
            QToolTip { color: #f5f3fa; background: #282733; border: 1px solid #ffffff24; padding: 5px 8px; }
            QDialog, QMessageBox, QInputDialog { background: #1b1a24; color: #f0eef8; }
            QPushButton { background: #2b2936; color: #e9e6f2; border: 1px solid #ffffff18;
                border-radius: 9px; padding: 8px 14px; }
            QPushButton:hover { background: #393646; border-color: #ffffff30; }
            QPushButton:default { background: #8971dc; color: white; border-color: #a28bec; }
            QListWidget { background: #22212c; color: #e9e7f0; border: 1px solid #ffffff18; border-radius: 10px; padding: 5px; }
            QListWidget::item { padding: 8px; border-radius: 6px; }
            QListWidget::item:selected { background: #8971dc55; }
        """)

    def current_tab(self) -> BrowserTab | None:
        tab = self.tabs.currentWidget()
        return tab if isinstance(tab, BrowserTab) else None

    def _chrome_tab_selected(self, index: int) -> None:
        if index >= 0 and index != self.tabs.currentIndex():
            self.tabs.setCurrentIndex(index)

    def _chrome_tab_moved(self, source: int, destination: int) -> None:
        tab = self.tabs.widget(source)
        title = self.tabs.tabText(source)
        self.tabs.blockSignals(True)
        self.tabs.removeTab(source)
        self.tabs.insertTab(destination, tab, title)
        self.tabs.setCurrentIndex(self.chrome_tabs.currentIndex())
        self.tabs.blockSignals(False)
        self._current_tab_changed(self.tabs.currentIndex())

    def _with_view(self, method: str) -> None:
        tab = self.current_tab()
        if tab:
            getattr(tab.view, method)()

    def new_tab(self, address: str = "") -> None:
        tab = BrowserTab(self.profile, address)
        tab.view.titleChanged.connect(lambda title, page=tab: self._title_changed(page, title))
        tab.view.iconChanged.connect(lambda icon, page=tab: self._icon_changed(page, icon))
        tab.view.urlChanged.connect(lambda url, page=tab: self._url_changed(page, url))
        tab.view.loadFinished.connect(lambda ok, page=tab: self._page_loaded(page, ok))
        tab.page.permissionRequested.connect(self._handle_permission_request)
        tab.view.loadProgress.connect(lambda progress: self.statusBar().showMessage(f"Loading… {progress}%", 1500))
        index = self.tabs.addTab(tab, "Новая вкладка")
        self.chrome_tabs.addTab("Новая вкладка")
        self.tabs.setCurrentIndex(index)
        self.chrome_tabs.setCurrentIndex(index)
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
        self.chrome_tabs.removeTab(index)
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
            self.tabs.setTabText(index, short or "Новая вкладка")
            self.chrome_tabs.setTabText(index, short or "Новая вкладка")
            if index == self.tabs.currentIndex():
                self.setWindowTitle(f"{title or 'Новая вкладка'} — Aurora")

    def _icon_changed(self, tab: BrowserTab, icon) -> None:
        index = self.tabs.indexOf(tab)
        if index >= 0:
            self.chrome_tabs.setTabIcon(index, icon)

    def _url_changed(self, tab: BrowserTab, url: QUrl) -> None:
        if tab is self.current_tab():
            self.address.setText(url.toString() if url.scheme() != "about" else "")
        self.back_button.setEnabled(tab.view.history().canGoBack())
        self.forward_button.setEnabled(tab.view.history().canGoForward())

    def _current_tab_changed(self, _index: int) -> None:
        if _index >= 0 and self.chrome_tabs.currentIndex() != _index:
            self.chrome_tabs.setCurrentIndex(_index)
        tab = self.current_tab()
        if tab:
            url = tab.view.url()
            self.address.setText(url.toString() if url.scheme() != "about" else "")
            self.setWindowTitle(f"{tab.view.title() or 'Новая вкладка'} — Aurora")
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
